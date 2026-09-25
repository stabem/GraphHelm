//! The scrubbed process primitive: argv-only spawning inside a workspace, with an allowlist
//! environment, deadline kill, and output caps. Every Tier 1 execution in this crate funnels
//! through [`run_in_workspace`]; there is no second spawn path to keep honest.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct CancelState {
    raised: std::sync::atomic::AtomicBool,
    /// Spawns currently inside the poll loop with this signal attached.
    in_flight: std::sync::Mutex<usize>,
    reaped: std::sync::Condvar,
    /// Every spawn this signal has seen, in order, each bound to a durable identity (#621).
    ///
    /// **Drained when each call ends**, not append-only. Holding every entry for the life of the
    /// host kept one OS handle per completed child — a `pidfd` each on Linux, until `Command::spawn`
    /// fails with `EMFILE` (Codex, on #680). A caller that already took an entry keeps it alive
    /// through its own `Arc`, which is how the cancellation cell can still ask after the call has
    /// let go.
    observed: std::sync::Mutex<Vec<std::sync::Arc<ObservedSpawn>>>,
}

/// A stop condition the caller can raise AFTER the child is running (#180).
///
/// A PARAMETER and not a field of [`ProcessLimits`], deliberately. Limits are configuration a
/// caller fixes before starting; this is control exercised during. Keeping it in the signature
/// means every spawn path has to SAY whether it is cancellable -- and a cancellation that
/// silently reached nothing is the whole of #180.
///
/// The deadline already proves the shape works: the poll loop owns the child and knows how to
/// kill and reap it. This gives that loop a SECOND reason to do exactly what it already does.
///
/// # What it does NOT reach, and the reasons are not the same
///
/// **Descendants are REACHED now (#618), and this paragraph used to say the opposite.** The kill
/// goes through `graphhelm_process_tree`: a process group on Unix, a kill-on-close job object on
/// Windows, with the child started suspended there so the job is assigned before it can spawn
/// anything outside it. `Child::kill` ended the direct process and left everything it started
/// reparented and running — and a descendant that inherited stdout or stderr held those pipes open,
/// so the reader joins below blocked after the direct child was already reaped. The leak was the
/// visible half; the wedged caller was the one that mattered.
///
/// **On Unix that sentence needs a qualifier, and a caller reading only this page would not know it**
/// (#717, #748). The two containers are not equally strong: a job object holds everything its
/// members create, while a process group is left by one `setsid` or `setpgid` call — no privileges,
/// nothing observable. So the Unix guarantee is "the tree, MINUS anything that deliberately left the
/// group", and it fails toward a false GREEN: the escapee survives, and if it redirected its streams
/// the readers below see a clean EOF, so the capture looks normal while the record is wrong. Treat a
/// clean Unix capture as evidence about the CHILD rather than about the tree. `adapters/process-tree`
/// carries the full comparison; #748 is the containment work that would remove the qualifier.
///
/// The claim is a LINK: delete the calls to that crate from `run_in_workspace` and
/// `the_stop_kills_the_whole_tree_and_not_only_the_direct_child` fails, while the crate's own tests
/// keep passing. (That cell was named `the_deadline_…` until #703 moved its trigger from the deadline
/// to a `CancelSignal`; this page cited the dead name until #735 — an instruction for falsifying a
/// claim is worth nothing if the reader runs it and finds nothing.)
///
/// An earlier version of this comment also said nothing in this repository reached the gap, because
/// `fake_tool` spawned nothing and the builtin tools run `git` directly. That was a measurement of
/// the FIXTURES wearing the clothes of a claim about the FUNNEL: `validate_program_name` checks the
/// shape of a name only, and `ToolCall::Tests` runs `config.tests_runner`, an operator-supplied
/// program whose whole job is to spawn other processes.
///
/// **Workspace management.** `workspace.rs` runs `git` directly rather than through
/// [`run_in_workspace`], for the reason written there: provisioning has no workspace to run inside
/// yet. Those spawns are outside this signal. Provisioning runs BEFORE the call a caller would
/// cancel, so a cancellation reaching it arrived before the tool ran at all — but **cleanup runs
/// AFTER the tool child is killed**, which is exactly when a caller that just cancelled is waiting
/// on this signal to return. That second one is the reachable half, and the first version of this
/// paragraph described only the first. Tracked as #617.
pub struct CancelSignal(std::sync::Arc<CancelState>);

impl CancelSignal {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Raise it and WAIT until every child attached to it has been killed and reaped.
    ///
    /// Signalling and guaranteeing are different promises, and #180's criterion is the second one:
    /// *a cancelled execution leaves no live child*. A `cancel` that only stored a flag would let
    /// the caller observe "cancelled" while a child is still running for up to one poll interval,
    /// so the caller could not act on the return at all -- it would have to invent its own wait,
    /// which is the bookkeeping this type exists to hold. (Codex, #609.)
    ///
    /// The wait is bounded rather than open: the loop it waits on polls every 50 ms and kills on
    /// sight, so a spawn cannot outlive the signal by more than that plus its own reap. The bound
    /// below exists for the case where a spawn never registered its exit -- a harness fault, not a
    /// slow child -- and it returns rather than blocking a caller forever.
    pub fn cancel(&self) {
        self.0
            .raised
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let Ok(guard) = self.0.in_flight.lock() else {
            return;
        };
        let _ =
            self.0
                .reaped
                .wait_timeout_while(guard, std::time::Duration::from_secs(30), |count| {
                    *count > 0
                });
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.raised.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Every spawn under this signal, in order, each bound to an identity the reap cannot
    /// invalidate (#621).
    ///
    /// **A testability seam, and it is here because the alternative is worse.** The cancellation
    /// cells used to prove "the child is gone" by sampling a trace file across a wall-clock window,
    /// which `AGENTS.md:121` forbids and which cannot in principle work: a child that is alive but
    /// DESCHEDULED for the whole window writes nothing and looks exactly like a dead one. Both
    /// produce zero bytes, so no oracle built on that file can separate them.
    ///
    /// The first version of this returned bare ids, and a bare id is not identity — it may be
    /// reassigned after the reap, so a check could observe an unrelated process and go red on a
    /// correct cancellation (Codex, on #680). Each entry now carries a Windows handle or a Linux
    /// `pidfd`, which the OS binds to the process rather than to the number.
    #[must_use]
    pub fn spawned_processes(&self) -> Vec<std::sync::Arc<ObservedSpawn>> {
        self.0
            .observed
            .lock()
            .map(|observed| observed.clone())
            .unwrap_or_default()
    }

    /// Register a spawn that has NOT happened yet, or refuse because cancellation already decided
    /// the count was zero.
    ///
    /// The check and the increment are one critical section on purpose. `cancel` stores `raised`
    /// and only then takes this lock, so a caller reaching the lock first either increments before
    /// `cancel` reads the count — and `cancel` then waits for it — or reads the flag `cancel`
    /// already stored and refuses. There is no interleaving that lets `cancel` return claiming the
    /// reap is done while a child appears afterwards. (Codex, #609, second round.)
    ///
    /// A poisoned lock refuses rather than proceeding uncounted. Fail-closed is the right side
    /// here: an uncounted spawn is invisible to `cancel`, which is the exact defect this guards.
    fn attach(&self) -> Option<AttachedSpawn<'_>> {
        let mut count = self.0.in_flight.lock().ok()?;
        if self.0.raised.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        *count += 1;
        Some(AttachedSpawn {
            signal: self,
            recorded: std::cell::RefCell::new(None),
        })
    }

    fn leave(&self) {
        if let Ok(mut count) = self.0.in_flight.lock() {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.0.reaped.notify_all();
            }
        }
    }

    /// Register a SPAN the cancellation must wait out, rather than a single spawn (#617).
    ///
    /// `attach` counts one child from spawn to reap. That is the right unit for
    /// [`run_in_workspace`] and the wrong one for a workspace, whose teardown runs AFTER the
    /// tool child has been killed -- which is exactly when a caller that just cancelled is
    /// waiting on `cancel`. Counted per child, the count reaches zero when the tool child is
    /// reaped, `cancel` returns, and the `git worktree remove` starts afterwards: the promise
    /// "a cancelled execution leaves no live child" is kept by the letter and broken by the
    /// clock.
    ///
    /// A hold spans provision-to-removal, so the count never reaches zero in that window and
    /// `cancel` waits for the teardown it caused. It shares `attach`'s critical section and its
    /// fail-closed refusal for the same reason: a span that begins after `cancel` read the count
    /// would be invisible to the caller `cancel` already answered.
    pub(crate) fn hold(&self) -> Option<CancelHold> {
        let mut count = self.0.in_flight.lock().ok()?;
        if self.0.raised.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
        *count += 1;
        drop(count);
        Some(CancelHold {
            signal: self.clone(),
        })
    }
}

/// One counted span, released when it is dropped.
///
/// Owns a clone of the signal rather than borrowing it, because the span outlives every
/// individual call frame that could hold a reference: it is created inside `provision` and
/// released inside `remove`.
pub(crate) struct CancelHold {
    signal: CancelSignal,
}

impl std::fmt::Debug for CancelHold {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("CancelHold")
    }
}

impl Drop for CancelHold {
    fn drop(&mut self) {
        self.signal.leave();
    }
}

/// How a supervised spawn ended.
#[derive(Debug)]
pub(crate) enum SupervisedOutcome {
    Finished(std::process::ExitStatus),
    /// The cancellation reached it and the tree was killed and reaped.
    Cancelled,
}

/// Spawn, then poll -- so a blocking `Command::status` is not the only thing between a
/// cancellation and the process it is meant to reach (#617).
///
/// `Command::status` blocks, which is why `workspace.rs` had no place to notice a signal. The
/// shape here is [`run_in_workspace`]'s, reduced to what a status-only spawn needs: the same
/// suspended-start-then-job-object preparation, the same 50 ms poll, the same kill-as-a-TREE.
///
/// **`interruptible` is a policy the caller owns, and the two callers want opposite answers.**
/// Provisioning may be killed: it runs before the tool does, and a workspace nobody will use is
/// waste. Removal may NOT: killing a `git worktree remove` halfway leaves the tree on disk, and
/// this module's own contract calls a leaked workspace a leaked write capability. So removal is
/// COUNTED but never cut -- `cancel` waits for it instead of stopping it, which is the whole
/// difference between the two words.
pub(crate) fn run_supervised(
    command: &mut std::process::Command,
    cancel: Option<&CancelSignal>,
    interruptible: bool,
) -> Result<SupervisedOutcome, HostError> {
    graphhelm_process_tree::configure(command);
    let mut child = command
        .spawn()
        .map_err(|source| HostError::Spawn { source })?;

    // Same refusal as `run_in_workspace`: on Windows this call is also what RESUMES the suspended
    // child, so a failure here leaves a child that cannot be killed as a tree. Refuse rather than
    // proceed with a weaker guarantee, and reap on the way out.
    let mut group = match graphhelm_process_tree::create(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HostError::ProcessGroup {
                rule: match error {
                    graphhelm_process_tree::ProcessTreeError::JobSetup => "job_setup",
                    graphhelm_process_tree::ProcessTreeError::ProcessResume => "process_resume",
                    // #878: the child reached `create` unsuspended, so it ran before the job
                    // could contain it and anything it started is outside. The funnel here DOES
                    // call `configure`, so this arm exists to keep the compiler forcing the
                    // decision at every call site rather than because this one can reach it.
                    graphhelm_process_tree::ProcessTreeError::ChildNotSuspended => {
                        "child_not_suspended"
                    }
                },
            });
        }
    };

    loop {
        // NOT `try_wait` (#714). `try_wait` reaps, and on Unix the group `close` releases is named
        // by the leader's pid -- so reaping before the release hands that number back to the
        // kernel and the release can land on someone else's job. This asks without reaping; the
        // reap is below, after `close`.
        match graphhelm_process_tree::leader_exited(&mut child) {
            Ok(true) => {
                graphhelm_process_tree::close(&mut group);
                // The anchor is let go HERE, and not before. `leader_exited` already saw the exit,
                // so this returns the status the child really had rather than waiting for one.
                let status = child.wait().map_err(|source| HostError::Spawn { source })?;
                return Ok(SupervisedOutcome::Finished(status));
            }
            Ok(false) => {
                if interruptible && cancel.is_some_and(CancelSignal::is_cancelled) {
                    // STILL DROPPED BY NAME, and #748 did not change that here -- what changed
                    // is the reason. `run_in_workspace` now keeps this answer, because it builds a
                    // `CapturedProcess` to put it in. THIS function does not: `supervise` returns
                    // `SupervisedOutcome`, and its only consumer (`workspace.rs`) turns a
                    // cancellation into `HostError::Cancelled` -- an error with no room for a
                    // measurement and no reader for one.
                    //
                    // Carrying it here would mean widening `HostError`, and a value nothing reads
                    // is not containment, it is a field to maintain. Provisioning CAN leave an
                    // escapee the same way a tool call can, so the gap is real and FILED rather
                    // than fixed by a value with no consumer: issue 868.
                    //
                    // Not folded into `readers_abandoned` there either, which was the cheap move
                    // available: that field means "something escaped the process group", and
                    // `reader_lost` was added BESIDE it rather than folded in, for the reason its
                    // own doc gives -- two causes under one name cannot be told apart afterwards.
                    // Copy the identity before the helper mutably borrows the group to close it.
                    let process_id = child.id();
                    let termination_group = graphhelm_process_tree::for_thread(group);
                    terminate_close_reap(
                        || {
                            let _ =
                                graphhelm_process_tree::terminate(process_id, termination_group);
                        },
                        || graphhelm_process_tree::close(&mut group),
                        || {
                            let _ = child.wait();
                        },
                    );
                    return Ok(SupervisedOutcome::Cancelled);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(source) => {
                // KILLED AND REAPED, not merely released. `close` is destructive on both
                // platforms: on Windows the job carries `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
                // and on Unix it signals the cached process group while the leader remains the
                // unreaped identity anchor. The first version of this arm returned an error
                // having left the child running, closed the only handle `terminate` could still
                // have reached it through, and left nothing able to reap it. Neither platform
                // got the `wait`, so unix also kept a zombie.
                //
                // The two sibling arms both clean up -- the create-failure arm twelve lines up
                // kills and waits, and `run_in_workspace`'s equivalent falls through to the common
                // reap instead of returning. This was the only exit that returned with a live
                // child, and it looked correct because `close` appears in the success arm three
                // lines away, where the child has already exited. (Found by a peer reviewing
                // PR #779.)
                //
                // Reachability of a `try_wait` error is NOT established, and this is not fixed on
                // the strength of it: it is fixed because the correct version is adjacent and the
                // next reader would take the wrong one for coverage.
                // Dropped by name, for the reason at the first call site.
                // Keep the same release-before-reap ordering as the cancellation arm. In
                // particular, `close` is the owning operation for the Windows job handle, so
                // returning on a poll error must not leak it.
                let process_id = child.id();
                let termination_group = graphhelm_process_tree::for_thread(group);
                terminate_close_reap(
                    || {
                        let _ = graphhelm_process_tree::terminate(process_id, termination_group);
                    },
                    || graphhelm_process_tree::close(&mut group),
                    || {
                        let _ = child.wait();
                    },
                );
                return Err(HostError::Spawn { source });
            }
        }
    }
}

/// Run the common tree cleanup sequence while keeping its ordering explicit and testable.
///
/// The group must be terminated before it is closed, and the leader must be reaped only after
/// close has released the identity anchor. On Windows, close also releases the owning job handle.
fn terminate_close_reap<Terminate, Close, Reap>(terminate: Terminate, close: Close, reap: Reap)
where
    Terminate: FnOnce(),
    Close: FnOnce(),
    Reap: FnOnce(),
{
    terminate();
    close();
    reap();
}

impl Clone for CancelSignal {
    fn clone(&self) -> Self {
        Self(std::sync::Arc::clone(&self.0))
    }
}

impl Default for CancelSignal {
    fn default() -> Self {
        Self(std::sync::Arc::new(CancelState::default()))
    }
}

impl std::fmt::Debug for CancelSignal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CancelSignal")
            .field("raised", &self.is_cancelled())
            .finish_non_exhaustive()
    }
}

/// One spawn, bound to an identity the reap cannot invalidate (#621, #680).
///
/// Not a bare id. An id may be reassigned once its process is reaped, so a check against one can
/// observe an unrelated process and read it as the child still running — a FALSE RED, and a false
/// red on an authoritative gate is still a nondeterministic gate. Holding a Windows handle or a
/// Linux `pidfd` binds the question to the process rather than to the number.
///
/// **A capture that failed is RECORDED as failed**, never replaced by the id. A caller that could
/// not ask must be told it could not ask; substituting the number would be the `unwrap_or` that
/// turns a known gap into silent drift.
#[derive(Debug)]
pub struct ObservedSpawn {
    process_id: u32,
    identity: Result<
        graphhelm_process_tree::ProcessIdentity,
        graphhelm_process_tree::IdentityUnavailable,
    >,
}

impl ObservedSpawn {
    /// The id this spawn was given. For diagnostics — never the thing to check.
    #[must_use]
    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    /// Whether this exact process is still running.
    ///
    /// # Errors
    /// [`graphhelm_process_tree::IdentityUnavailable`] when the identity could not
    /// be taken or the query failed. That is "I could not ask", and a caller must treat it as a
    /// broken instrument rather than as an answer in either direction.
    pub fn is_running(&self) -> Result<bool, graphhelm_process_tree::IdentityUnavailable> {
        self.identity.as_ref().map_or_else(
            |error| Err(*error),
            graphhelm_process_tree::ProcessIdentity::is_running,
        )
    }
}

pub struct ProcessLimits {
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

#[derive(Debug)]
pub struct CapturedProcess {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// Whether the STDOUT cap was reached, on its own.
    ///
    /// #177: this and `stderr_truncated` were computed inside `run_in_workspace` and then thrown
    /// away — the struct carried only their OR, so a stage whose stderr was cut and whose stdout
    /// was whole read identically to the reverse. Nothing new is measured here; the boundary
    /// simply stopped discarding what the reader already knew. `ProcessResult` in
    /// `adapters/postgres-event-store/src/backup.rs` has carried the unfused pair all along, so
    /// this is the repository agreeing with itself rather than a new convention.
    pub stdout_truncated: bool,
    /// Whether the STDERR cap was reached, on its own. See `stdout_truncated`.
    pub stderr_truncated: bool,
    /// The OR of the two above, kept so existing readers do not change meaning under them.
    ///
    /// DERIVED, not a third fact: it stays exactly as informative as it always was, which is the
    /// point — anything that needs to know WHICH stream was cut must read the per-stream fields,
    /// and anything that only asks "was anything cut" keeps working unchanged.
    pub truncated: bool,
    pub timed_out: bool,
    /// Whether the capture gave up waiting for a reader (#618).
    ///
    /// SEPARATE from `truncated`, deliberately. Truncation means a cap was reached and the bytes
    /// past it were dropped on purpose; this means the bytes could not be READ AT ALL, because
    /// something other than the child still held the pipe. Folding the two into one flag would make
    /// a deliberate cut indistinguishable from a lost capture, which is the shape #177 existed to
    /// undo one field over.
    ///
    /// It should never be true once the kill reaches the whole tree. If it is, something escaped
    /// the process group or the job object, and the flag is the only place that says so.
    pub readers_abandoned: bool,
    /// A reader thread ended WITHOUT answering, so this call cannot account for what it held.
    ///
    /// **A third state, and it exists because widening either of the other two would be a lie**
    /// (#790). `truncated` means bytes were read and then dropped -- this reader may have read
    /// nothing, or everything, and there is no way to tell. `readers_abandoned` means SOMETHING
    /// ESCAPED THE TREE KILL, which `tests/process_isolation.rs` asserts against as a containment
    /// claim; a dead reader thread escaped nothing, so setting that flag would make a containment
    /// assertion fail for a reason that has nothing to do with containment.
    ///
    /// **What it separates:** "the tool printed nothing" from "this call's reader died holding
    /// what the tool printed". Before this field those were the same observation -- an empty
    /// buffer, `truncated` false, `readers_abandoned` false -- and the second is a successful tool
    /// call whose output silently vanished. The consequence is worst where output IS the point: a
    /// `ToolCall::Tests` whose runner printed a hundred failing assertions and whose reader then
    /// died reports as a clean run with no output, and the operator reads the exit code.
    ///
    /// **Reachability, measured rather than assumed:** the reader closure sends unconditionally at
    /// the end of its loop, and that is its ONLY send site. So a receiver seeing `Disconnected`
    /// saw the sender dropped without a send, which happens only if the thread unwound before
    /// reaching it -- an allocation failure under memory pressure, or a panic inside the head/tail
    /// elision. Rare, and the rarity is exactly why it would never be noticed.
    pub reader_lost: bool,
    /// What the tree kill could actually PROVE, or `None` when no kill was attempted (#748).
    ///
    /// **`None` is not `Complete`.** A child that exited on its own was never swept, and answering
    /// `Complete` there would put a measurement that never happened into the record. They are
    /// different questions and this field keeps them apart.
    ///
    /// **Why the whole outcome and not a bool.** `BoundReached` means the sweep KNOWS descendants
    /// were still appearing when it ran out of passes; `SweepUnavailable` means it could not look
    /// at all. A bool folds "I know something survived" into "I cannot tell", and those two call
    /// for opposite responses -- the first is a containment failure to act on, the second is a
    /// missing instrument. That is the fold `reader_lost` was added BESIDE `readers_abandoned` to
    /// avoid (#790), one field above this one.
    ///
    /// This is the half PR #805 named and did not finish. `terminate` has answered since
    /// `e0df54f1` and every call site in this file dropped the answer by name; while it was
    /// dropped a `BoundReached` read exactly like a clean tree. That is the false GREEN #748
    /// exists to close, and it closes at the RECORD rather than at the kill.
    pub tree_kill: Option<graphhelm_process_tree::TerminationOutcome>,
    /// Whether a raised [`CancelSignal`] is what stopped this child (#609, Codex).
    ///
    /// `timed_out = expired && !cancelled` removes the WRONG cause from the record; it does not
    /// put the right one there. Without this field the disposition falls through to the exit code,
    /// and a killed child has one: on Windows a cancelled call was recorded as
    /// `Completed { exit_code: 1 }` — a tool that ran and failed — which
    /// `ToolFailureSemantics::RetryEligible` maps to `RetryableFailure`. A cancellation dressed as
    /// a retryable failure is worse than a misnamed one, because a consumer may act on it.
    pub cancelled: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("the program failed to spawn")]
    Spawn { source: std::io::Error },
    #[error("a declared environment name is refused: {name}")]
    ExtraEnvDenied { name: String },
    #[error("the workspace could not be prepared")]
    Prepare { source: std::io::Error },
    /// A workspace-configuration rule was violated. `rule` is a fixed rule name, never a path
    /// or caller content.
    #[error("workspace configuration refused: {rule}")]
    Config { rule: &'static str },
    /// A resolved path crossed a link or junction — containment refuses it.
    #[error("the path escapes the workspace through a link")]
    Escape,
    /// The routing layer refused a plan whose tier cannot carry its effect — defense in
    /// depth: `authorize` can never produce the shape, and the host refuses it anyway.
    #[error("the plan's tier cannot carry its effect")]
    TierViolation,
    /// The program is not an identity the broker can verify (#540): a relative name would be
    /// resolved by the OS's search order, which is how one name runs two binaries.
    #[error("the executable is not pinned: {rule}")]
    ExecutableNotPinned { rule: &'static str },
    /// The bytes at the pinned path do not hash to the expected value (#540). Carries both so
    /// the operator can decide between re-pinning and investigating; never falls back to
    /// running.
    #[error("the executable does not match its pin")]
    ExecutableMismatch { expected: String, actual: String },
    /// The child could not be placed in a killable process group (#618). `rule` is a fixed name,
    /// never OS text.
    ///
    /// Refused rather than run: a child outside its group cannot be killed as a tree, and returning
    /// a record for it would advertise a guarantee this call could not keep.
    #[error("the child could not be placed in a process group: {rule}")]
    ProcessGroup { rule: &'static str },
    /// The host's cancellation signal was already raised when this call asked to spawn (#609).
    /// Refused rather than spawned-then-killed: `cancel` waits only for the children it knows
    /// about, so one created after it returned would outlive the guarantee it had just given.
    #[error("the host was cancelled before this call could spawn")]
    Cancelled,
    /// The index-snapshot copy no longer hashes to its recorded generation (#539). Carries both
    /// values — re-pin or investigate, never read anyway.
    #[error("the index snapshot does not match its pin")]
    SnapshotMismatch { expected: String, actual: String },
    /// The capture gave up on a reader, so the bytes this call would report were never read
    /// (#618). `rule` is a fixed name, never OS text.
    ///
    /// Refused rather than returned, and ONLY on the seams that have no way to say it otherwise.
    /// `ToolHost::invoke` reports the same condition as `GHTOOL013_CAPTURE_LOST`, because a
    /// disposition is exactly the vocabulary for "the call happened and here is what we know".
    /// A consumer holding a bare `CapturedProcess` has no such vocabulary: it reads `exit_code`,
    /// hashes the bytes, and an unread `stderr` hashes to the digest of zero bytes -- evidence
    /// that the tool printed nothing, rather than the absence it actually is.
    #[error("the capture could not be read: {rule}")]
    CaptureLost { rule: &'static str },
    /// The probe for `refs/graphhelm/executions/<id>` did not answer (#1086, Codex P1 on #1092):
    /// the scratch directory could not be made, `git rev-parse` timed out or exited with something
    /// other than 0 (present) or 1 (verified absent). Distinct from "absent" so a consumer never
    /// falls back to the project checkout when the answer is unknown. `rule` is a fixed name.
    #[error("the execution ref could not be probed: {rule}")]
    RefProbe { rule: &'static str },
}

/// Refuse a capture whose readers were abandoned; pass every other capture through unchanged.
///
/// The one place this decision is made for consumers that receive a `CapturedProcess` directly.
/// It is a FUNCTION rather than an inline `if` because the condition it discriminates cannot be
/// produced on a correct build -- `readers_abandoned` is only ever true when something escaped
/// the process group -- so the only way to test the decision without sabotaging the tree kill is
/// to hand it the value.
///
/// # Errors
/// [`HostError::CaptureLost`] when the capture reports abandoned readers.
pub fn reject_lost_capture(captured: CapturedProcess) -> Result<CapturedProcess, HostError> {
    if captured.readers_abandoned {
        return Err(HostError::CaptureLost {
            rule: "a reader was abandoned, so these bytes were never read",
        });
    }
    // THE THIRD SITE, and the one where the consequence is worst (#790). This seam's own doc
    // explains why it refuses rather than reports: its consumers HASH THE BYTES, and an empty
    // stream that was never read hashes to the digest of zero bytes, which is indistinguishable
    // from a tool that printed nothing. That argument does not care HOW the bytes went missing --
    // an escaped descendant or a reader thread that died holding them produce the same untrusted
    // digest -- and the guard checked only the first cause.
    if captured.reader_lost {
        return Err(HostError::CaptureLost {
            rule: "a reader ended without answering, so these bytes cannot be accounted for",
        });
    }
    Ok(captured)
}

/// The fixed inheritance allowlist. Everything else the parent holds — passphrases, tokens,
/// profile paths — is structurally absent from the child. `HOME`/`USERPROFILE`/`TEMP`/`TMP`
/// are not inherited but REDIRECTED into the workspace so git and tools that insist on a home
/// write inside the sandbox and read no host config (`GIT_CONFIG_NOSYSTEM` closes the /etc
/// side). Note the contrast with the 05b gateway's allowlist, which deliberately keeps the
/// host `HOME`/`APPDATA` because official CLIs own their own auth: a tool workspace has no
/// auth of its own to keep, so Tier 1 is stricter by design.
/// The shortest drain a loaded host can still be expected to finish in.
///
/// A floor, not a target. Without one, a child that ran all the way to its deadline would leave zero
/// drain, and every timed-out call would start reporting capture losses caused by nothing but the
/// arithmetic.
const MINIMUM_DRAIN: Duration = Duration::from_secs(1);

/// When the readers must stop draining: the invocation's OWN deadline, floored.
///
/// Two wrong versions came before this one, and the second was mine (Codex, on #703, twice).
///
/// A fixed twenty seconds was wrong in both directions at once: a two-second call could overshoot
/// its deadline by twenty, and a two-minute call would cut a descendant still legitimately writing
/// at twenty. Deriving the budget from `limits.timeout` fixed those and kept the deeper error --
/// it started a FRESH clock, so a child that ran for nearly its whole deadline and then left a pipe
/// held could take almost TWICE the timeout to return, and the escape was reported only after the
/// caller's entire budget with no ceiling above it.
///
/// Reusing the deadline removes both. The drain ends when the invocation was always going to end,
/// so the total is bounded by the timeout the caller chose plus the floor -- never a multiple of it.
///
/// This is the CEILING, and it is no longer the only bound. The gap a clock cannot close -- a drain
/// that ends while bytes are still arriving cuts a legitimate capture no matter which instant is
/// chosen -- is closed by `drain_readers` above, which ends its wait on SILENCE and reaches this
/// deadline only when a stream never goes quiet. The readers report progress for that (#708), which
/// is also the protocol #726 needs; this function still answers the different question of how long
/// the whole drain may take.
#[must_use]
fn drain_deadline(invocation_deadline: Instant, now: Instant) -> Instant {
    invocation_deadline.max(now + MINIMUM_DRAIN)
}

/// How long a still-unfinished stream may report NOTHING before the drain stops waiting on it.
///
/// This is the "expires on silence rather than on elapsed time" half of #708, and it is the only
/// part of that design that trades anything away, so the trade is written here rather than left to
/// be discovered.
///
/// It is reachable only after the direct child has been reaped. A reader still blocked at that
/// point means a descendant outlived the kill and is holding the pipe. Two things it could be
/// doing: writing -- progress keeps arriving, the grace keeps resetting, and nothing is cut -- or
/// producing nothing. Five seconds of nothing is the line, and it is drawn well past any
/// scheduling hiccup a loaded host produces: the failure this whole path exists for took 22
/// seconds to report, so the resolution needed here is seconds, not milliseconds.
///
/// **The known limit, and it is a real one:** a descendant can be legitimately silent. A tests
/// runner's worker that spends thirty seconds compiling before it prints is exactly that, and this
/// grace cuts it at five. What that costs is bounded, and it is NOT a regression against today: on
/// this path today's code waits out the whole drain budget and then discards the entire capture, so
/// the bytes this cuts short are bytes today loses anyway unless the descendant happens to close
/// the pipe before the deadline. What it buys is the rest of the capture arriving at all, and the
/// call returning in seconds instead of in a full timeout. The hard deadline still bounds
/// everything above it; this only ever ends the wait EARLIER.
const READER_SILENCE_GRACE: Duration = Duration::from_secs(5);

/// How often the drain samples the progress counters while it waits.
///
/// The same 50 ms the child poll uses. It is a sampling rate, not a timeout: nothing is decided by
/// it, and halving or doubling it changes only how promptly a silence is noticed.
const DRAIN_POLL: Duration = Duration::from_millis(50);

/// How long the readers get to answer once the group has been released.
///
/// Releasing the job closes the descendant's ends of the pipes, so a reader that was blocked in
/// `read` returns at once and sends what it had. This is the wait for that answer, and it is short
/// because it is not waiting for work -- only for a thread to be scheduled after an EOF it has
/// already been handed.
const POST_RELEASE_GRACE: Duration = Duration::from_secs(2);

/// How often a reader thread checks whether its capture has been abandoned, between attempts to
/// read (#726).
///
/// This is the poll's timeout, never the capture's: a reader that is neither abandoned nor has
/// data yet just loops again, exactly as if it were still blocked in a plain `read`. Shortening
/// or lengthening this changes only how promptly an ALREADY-abandoned reader notices and ends --
/// it decides nothing about a live capture, which is bounded by `READER_SILENCE_GRACE` and
/// `POST_RELEASE_GRACE` above, several layers removed from this thread.
const READER_POLL: Duration = Duration::from_millis(20);

/// A raw OS handle to one end of a pipe, cheap to copy into the reader thread alongside the
/// `Box<dyn Read>` that owns the real handle -- this is a second, non-owning view of the SAME
/// handle, used only to ask the OS "is there anything to read, or has every writer end closed"
/// without blocking.
///
/// A bare `RawHandle` is `*mut c_void` and is not `Send`, even though moving the opaque VALUE
/// (never dereferencing it as a pointer) between threads is exactly what every `AsRawHandle`
/// consumer already does. Wrapped rather than transmuted past the check: the real owner of the
/// handle is the boxed `Read` moved into the same thread alongside it, and this is never used to
/// close or duplicate the handle -- only to ask about it.
#[derive(Clone, Copy)]
struct RawPipeHandle(RawPipeHandleInner);
#[cfg(windows)]
type RawPipeHandleInner = std::os::windows::io::RawHandle;
#[cfg(unix)]
type RawPipeHandleInner = std::os::unix::io::RawFd;
unsafe impl Send for RawPipeHandle {}

/// Whether a pipe end has bytes waiting, is at EOF (every writer closed), or has neither yet.
enum Readiness {
    HasBytes,
    Eof,
    NotYet,
}

/// Windows: `PeekNamedPipe` reports how many bytes are buffered without consuming them and
/// without blocking. A broken pipe (every write handle closed) fails the call with
/// `ERROR_BROKEN_PIPE` -- any other failure is treated the same way, because a handle this
/// function cannot even ask about cannot be read from either, and the difference does not change
/// what the reader does next.
#[cfg(windows)]
fn readiness(raw: RawPipeHandle) -> Readiness {
    use windows_sys::Win32::Foundation::HANDLE;
    let mut available: u32 = 0;
    let ok = unsafe {
        windows_sys::Win32::System::Pipes::PeekNamedPipe(
            raw.0 as HANDLE,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::addr_of_mut!(available),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Readiness::Eof;
    }
    if available > 0 {
        Readiness::HasBytes
    } else {
        Readiness::NotYet
    }
}

/// Unix: `poll` with a short timeout on the raw fd. `POLLIN` fires both when data is buffered and
/// when the peer has hung up with nothing left to deliver -- the subsequent real `read` tells
/// those two apart on its own (a positive count, or `Ok(0)`), the same as it always has.
#[cfg(unix)]
fn readiness(raw: RawPipeHandle) -> Readiness {
    let mut fds = [libc::pollfd {
        fd: raw.0,
        events: libc::POLLIN,
        revents: 0,
    }];
    let timeout_ms = i32::try_from(READER_POLL.as_millis()).unwrap_or(i32::MAX);
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), 1, timeout_ms) };
    if ready < 0 {
        return Readiness::Eof;
    }
    if ready == 0 {
        return Readiness::NotYet;
    }
    if fds[0].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
        Readiness::HasBytes
    } else {
        Readiness::NotYet
    }
}

/// What a reader thread hands back to its caller: the answer channel, the byte counter #708
/// samples, and the give-up signal #726 sets once the drain has genuinely abandoned this stream --
/// never on a merely-quiet one.
///
/// Named `give_up`, not `abandoned`, to stay distinct from `DrainedReaders::abandoned` just below
/// -- that field is the PAST-TENSE fact a finished drain reports outward ("this call's capture was
/// abandoned"); this one is the signal a still-running reader thread watches for, in the other
/// direction. The two are related but never the same value at the same time: this is set only
/// once that becomes true.
struct ReaderHandle {
    receiver: Receiver<(Vec<u8>, bool)>,
    progress: Arc<AtomicU64>,
    give_up: Arc<AtomicBool>,
}

/// Spawn one reader thread over `pipe`. ANSWERS THROUGH A CHANNEL rather than a join handle, so
/// the wait for it can be bounded from outside -- and, as of #726, the thread's OWN loop is
/// bounded too, so a caller that gives up is not left with a thread it can never reap.
///
/// `raw` is a second, non-owning view of the exact handle `pipe` owns, used only to poll
/// readiness (#726) -- `pipe` itself remains the single owner and is what actually reads and, on
/// drop, closes its end.
/// TEST-ONLY SEAM (#726): how many reader threads are alive right now, process-wide. A private
/// counter rather than an OS thread-table snapshot, deliberately -- `cargo test` runs many cells
/// concurrently, each free to spawn its own unrelated threads, so a global OS count would be
/// noisy in exactly the way this counter is not: it counts only what `spawn_reader` itself spawns.
#[cfg(test)]
static LIVE_READER_THREADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
struct ReaderThreadGuard;

#[cfg(test)]
impl ReaderThreadGuard {
    fn new() -> Self {
        LIVE_READER_THREADS.fetch_add(1, Ordering::Relaxed);
        Self
    }
}

#[cfg(test)]
impl Drop for ReaderThreadGuard {
    fn drop(&mut self) {
        LIVE_READER_THREADS.fetch_sub(1, Ordering::Relaxed);
    }
}

fn spawn_reader(mut pipe: Box<dyn Read + Send>, raw: RawPipeHandle, cap: usize) -> ReaderHandle {
    let (sender, receiver) = std::sync::mpsc::channel();
    // The reader answers ONCE, at EOF. That is what makes "still busy" and "wedged" the same
    // observation from outside, and #708 is the consequence: a drain that can only wait, and
    // on expiry throws the whole capture away. The counter is the second observable -- bytes
    // seen so far -- and it is what lets the drain below tell a stream that is producing from
    // one that has gone quiet.
    //
    // A counter rather than a progress MESSAGE per chunk, and the difference is a leak: this
    // channel is unbounded and nobody drains it until the wait starts, so a chatty stream
    // would queue one allocation per 8 KiB read for the whole run. An atomic that the reader
    // stores into and the drain samples costs one word and cannot grow.
    let progress = Arc::new(AtomicU64::new(0));
    let reported = Arc::clone(&progress);
    let give_up = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&give_up);
    std::thread::spawn(move || {
        // TEST-ONLY SEAM (#726), in the style of `off_reactor_witness` (#854): a live-count of
        // reader threads, so a cell can ask "did N abandoned captures leave N threads behind"
        // directly rather than by inference. The guard's `Drop` runs on every exit from this
        // closure -- the normal EOF break, the abandonment break, and the read-error break alike
        // -- so a cell cannot pass by covering only one of the three.
        #[cfg(test)]
        let _reader_thread_guard = ReaderThreadGuard::new();
        // #177 tail half: keep the HEAD *and* the TAIL, eliding the middle.
        //
        // Keeping only the head is biased against the reason anyone opens the log. In a red
        // suite the failing assertion and the `test result: FAILED` line are at the END, so a
        // head-only capture reliably discards the one part a triager needs. Both ends carry
        // real failures, each with a live instance from this repository's own work: a
        // toolchain fault (`invalid metadata for crate core`) prints as the build STARTS, and
        // the assertion that failed prints LAST. Tail-only would just move the blind spot.
        //
        // Memory stays bounded by `cap`: the head stops at `head_cap`, and the tail is a ring
        // holding at most `tail_cap`. The pipe is still drained to EOF either way -- dropping
        // bytes must never mean leaving them in the pipe, which is what would deadlock the
        // child.
        let head_cap = cap / 2;
        let tail_cap = cap - head_cap;
        let mut head = Vec::new();
        let mut tail: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
        let mut elided: u64 = 0;
        let mut chunk = [0_u8; 8192];
        'outer: loop {
            // #726: poll readiness before the real read, rather than calling `pipe.read`
            // directly. A writer end that never closes -- an escaped descendant holding the
            // pipe open -- blocks a plain `read` forever, and a thread blocked in `read` cannot
            // be interrupted from outside in safe Rust. This loop CAN be interrupted, between
            // polls, by `signal`: the timeout belongs to the poll, never to the capture, so a
            // stream that is genuinely producing (or genuinely silent but not yet abandoned)
            // is never cut here -- it just polls again. Only `signal` being set, which nothing
            // sets except the drain having already exhausted its own silence grace AND its
            // post-release grace, ends the loop early.
            loop {
                if signal.load(Ordering::Relaxed) {
                    break 'outer;
                }
                match readiness(raw) {
                    Readiness::HasBytes | Readiness::Eof => break,
                    Readiness::NotYet => std::thread::sleep(READER_POLL),
                }
            }
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    // Published BEFORE the bytes are filed, so the drain's view of "something
                    // arrived" can never lag the work. `Relaxed` is right: the only consumer
                    // asks whether the value CHANGED, never what it orders against.
                    reported.fetch_add(count as u64, Ordering::Relaxed);
                    let mut rest = &chunk[..count];
                    let room = head_cap.saturating_sub(head.len());
                    if room > 0 {
                        let take = rest.len().min(room);
                        head.extend_from_slice(&rest[..take]);
                        rest = &rest[take..];
                    }
                    if rest.is_empty() {
                        continue;
                    }
                    // NOT `truncated = true` here (D's finding 1 on #582). Bytes past the head
                    // are not lost -- they go to the tail. Setting the flag here made every
                    // stream over `head_cap` claim data loss even when every byte survived,
                    // which under the production cap is every stream over 4 MiB. The flag is
                    // derived from `elided` below, where loss is actually known.
                    tail.extend(rest.iter().copied());
                    while tail.len() > tail_cap {
                        tail.pop_front();
                        elided = elided.saturating_add(1);
                    }
                }
            }
        }
        let kept = if elided == 0 {
            // Everything after the head still fit: the original bytes, unmarked.
            head.extend(tail.iter().copied());
            head
        } else {
            // The marker is paid for out of the TAIL so the capture still fits `cap`.
            //
            // D's finding 2 on #582: the previous version formatted the marker TWICE and
            // budgeted with the first length. Popping a byte raises `elided`, which can carry
            // it across a power of ten and make the second marker one byte longer -- measured
            // at `cap = 7_388_638`, one byte over. The same defect in a worse shape: when the
            // cap is smaller than the marker itself, the loop exited on `!tail.is_empty()` and
            // appended the whole marker anyway -- 52 bytes under a 40-byte cap.
            //
            // One cause: the budgeted length and the emitted length came from different values
            // of `elided`. So the loop now re-formats each round and exits only when the marker
            // it will actually emit fits beside the tail it will actually keep.
            let mut marker = elision_marker(elided);
            while head.len() + marker.len() + tail.len() > cap && !tail.is_empty() {
                tail.pop_front();
                elided = elided.saturating_add(1);
                marker = elision_marker(elided);
            }
            if head.len() + marker.len() + tail.len() > cap {
                // The tail is empty and the marker alone still does not fit: the cap is smaller
                // than the sentence. Shorten the marker rather than exceed the budget -- the
                // capture's size is a promise to the caller, the marker's completeness is not.
                marker.truncate(cap.saturating_sub(head.len()));
            }
            head.extend_from_slice(&marker);
            head.extend(tail.iter().copied());
            debug_assert!(head.len() <= cap, "the capture must never exceed its cap");
            head
        };
        let truncated = elided > 0;
        // A closed receiver means the caller stopped listening -- it reached the reader deadline
        // and moved on. Nothing to report and nothing to fail: this thread is the abandoned
        // one, and it ends here rather than outliving the call with an answer nobody wants.
        let _ = sender.send((kept, truncated));
    });
    ReaderHandle {
        receiver,
        progress,
        give_up,
    }
}

/// What the drain came back with.
struct DrainedReaders {
    stdout: (Vec<u8>, bool),
    stderr: (Vec<u8>, bool),
    /// A reader had to be forced to EOF by releasing the group, whether or not it then answered.
    ///
    /// The flag keeps the meaning it had before #708 -- SOMETHING ESCAPED THE KILL -- rather than
    /// narrowing to "we captured nothing". Those are different facts and only one of them is about
    /// this code: a partial capture recovered after a forced release is still an escape, and a
    /// caller reading this flag is asking about containment, not about byte counts.
    abandoned: bool,
    /// Whether the drain already released the group, so the caller does not release it twice.
    released: bool,
    /// A reader's sender was dropped without an answer -- see `CapturedProcess::reader_lost`.
    reader_lost: bool,
}

/// Wait for both readers, and on expiry force EOF and keep whatever had drained.
///
/// **The shape #708 describes, and the reason the old one lost data.** A reader answers only at
/// EOF, and EOF needs every writer end closed. A descendant that outlived the tree kill holds one
/// open, so the wait expires -- and the previous code then substituted an EMPTY buffer for that
/// stream and released the group afterwards. The bytes existed; the reader was holding them; the
/// only thing missing was a second wait after the release that would have collected them.
///
/// So: wait, and if the wait ends with a stream still pending, release the group FIRST and then
/// wait again, briefly. The release is what turns a held pipe into an EOF, which is what turns a
/// blocked reader into an answer.
///
/// **Why the release cannot simply happen sooner.** The job object carries `KILL_ON_JOB_CLOSE`, so
/// releasing it kills whatever still holds the pipe. Doing that while a descendant is mid-write
/// truncates a capture that was arriving. That is precisely why the first wait ends on SILENCE and
/// not on a clock: a stream that has produced nothing for `silence_grace` is one where releasing
/// costs nothing, and a stream still producing keeps resetting its own grace and is never cut.
///
/// `release` is a callback rather than the group itself so this function can be exercised with
/// hand-driven channels. The escape it exists to handle cannot be staged on a platform whose job
/// object has no breakaway (#717), so the seam is how the mechanism gets a red at all.
///
/// **DECLARED GAP, measured rather than reasoned: the TREE half of the kill above has no cell at
/// this site.** Downgrading `terminate(child.id(), for_thread(group))` to `child.kill()` -- #618's
/// original defect, at a new site -- leaves all sixteen `graphhelm-tool-host` suites green. The
/// reason is the arrangement, not the assertions: neither caller here produces a descendant.
/// Provisioning runs `git worktree add` with `core.hooksPath` pointed at an empty directory
/// precisely so it cannot, and removal runs `git worktree remove`. So the property is real, and
/// nothing at THIS site can currently observe it.
///
/// Where it IS observed: `run_in_workspace`, through
/// `tests/process_isolation.rs::the_stop_kills_the_whole_tree_and_not_only_the_direct_child`,
/// whose fixture spawns a grandchild on purpose. Both functions now perform the same three calls,
/// so a regression in the crate's tree-kill discipline still reddens there -- but a regression in
/// THIS copy of it would not. (Predicted by a peer reviewing PR #779; the prediction was run and
/// held.)
fn drain_readers(
    stdout: ReaderHandle,
    stderr: ReaderHandle,
    hard_deadline: Instant,
    silence_grace: Duration,
    poll: Duration,
    post_release_grace: Duration,
    release: &mut dyn FnMut(),
) -> DrainedReaders {
    let mut pending = [Some(stdout), Some(stderr)];
    let mut answers: [Option<(Vec<u8>, bool)>; 2] = [None, None];
    let mut reader_lost = false;
    let mut seen = [0_u64; 2];
    let mut last_change = [Instant::now(); 2];

    loop {
        for index in 0..pending.len() {
            let Some(handle) = pending[index].as_ref() else {
                continue;
            };
            let (receiver, progress) = (&handle.receiver, &handle.progress);
            match receiver.recv_timeout(poll) {
                Ok(answer) => {
                    answers[index] = Some(answer);
                    pending[index] = None;
                }
                // A sender dropped without answering means the reader thread is gone and no
                // answer is coming. Stop waiting on it. It is not pending, and it is NOT AN
                // ESCAPE, so it must not set `abandoned` -- that flag is a containment claim and
                // a dead reader escaped nothing.
                //
                // But it is not nothing either, and saying nothing was the defect (#790): the
                // caller would receive an empty buffer indistinguishable from a tool that printed
                // nothing. `reader_lost` is the third state, set here and nowhere else.
                Err(RecvTimeoutError::Disconnected) => {
                    pending[index] = None;
                    reader_lost = true;
                }
                Err(RecvTimeoutError::Timeout) => {
                    let bytes = progress.load(Ordering::Relaxed);
                    if bytes != seen[index] {
                        seen[index] = bytes;
                        last_change[index] = Instant::now();
                    }
                }
            }
        }
        if pending.iter().all(Option::is_none) {
            break;
        }
        let now = Instant::now();
        if now >= hard_deadline {
            break;
        }
        // EVERY still-pending stream, not any one of them. stderr is silent for most of a normal
        // run, so an any-of test would release the group on a healthy call whose stdout is still
        // streaming -- the exact truncation this grace exists to avoid, arrived at from the other
        // direction.
        let all_silent = pending
            .iter()
            .enumerate()
            .filter(|(_, slot)| slot.is_some())
            .all(|(index, _)| now.duration_since(last_change[index]) >= silence_grace);
        if all_silent {
            break;
        }
    }

    let mut abandoned = false;
    let mut released = false;
    if pending.iter().any(Option::is_some) {
        // The escape is already established here: a reader is still blocked after the child was
        // reaped. Flag it NOW rather than after the second wait, so a recovered partial capture
        // still reports the containment failure that produced it.
        abandoned = true;
        released = true;
        release();
        for index in 0..pending.len() {
            let Some(handle) = pending[index].take() else {
                continue;
            };
            let receiver = &handle.receiver;
            // PER STREAM, not one window shared by both (#788). Computed once outside this loop,
            // a first stream that used the whole grace left the second with a zero timeout -- so
            // its answer was discarded even though the release had already turned its held pipe
            // into an EOF and its reader was on the way to send. That is #708's own defect at the
            // last step: the capture existed, was recoverable, and the drain stopped listening.
            //
            // It lands on the asymmetric case this design already worries about, because stdout is
            // the big stream and is waited on first.
            //
            // The cost is a worst case of 2x `post_release_grace` instead of 1x -- four seconds,
            // on a path only reached after the invocation deadline has already passed, against
            // losing a capture that was in hand. (Found by a peer reviewing PR #770, after it
            // merged.)
            //
            // **A SHARED BUDGET IS NOT A DEFECT BY ITSELF, and the line is worth drawing here so
            // the next reader does not "fix" the other side of it** (#793). The class was swept
            // rather than the instance: this crate has one other `recv_timeout`, a per-iteration
            // poll above, and one real structural twin -- `OperationDeadline` in `backup.rs`, which
            // shares ONE budget across its steps deliberately, says so, and spells `step()` as
            // `remaining().min(ceiling)`.
            //
            // The rule that separates them: INDEPENDENT CAPTURES GET THEIR OWN WINDOW; STEPS OF ONE
            // OPERATION SHARE ONE. Two readers of two pipes are independent -- neither's answer is
            // the other's input, and one being slow says nothing about the other. A sequence of
            // steps bounded by a caller's single deadline is not.
            if let Ok(answer) = receiver.recv_timeout(post_release_grace) {
                answers[index] = Some(answer);
            } else {
                // #726: every wait this function offers is now exhausted for this stream --
                // the silence grace, the hard deadline, the release, and the post-release
                // grace all passed with no answer. Only NOW is it safe to say so to the reader
                // itself: it stops polling and ends within one more `READER_POLL` tick instead
                // of leaking for the life of the process. Setting this here, never earlier,
                // is what keeps a merely-slow stream from ever being told to give up.
                handle.give_up.store(true, Ordering::Relaxed);
            }
        }
    }

    let [stdout, stderr] = answers;
    DrainedReaders {
        // An unanswered stream is empty and NOT marked truncated: `truncated` means the capture
        // dropped bytes it had read, and a reader that never answered read nothing this call can
        // account for. `abandoned` is the field that says the capture is incomplete.
        stdout: stdout.unwrap_or_else(|| (Vec::new(), false)),
        stderr: stderr.unwrap_or_else(|| (Vec::new(), false)),
        abandoned,
        released,
        reader_lost,
    }
}

/// Clear a command's inherited environment and re-admit only the allowlist.
///
/// **The same list the tool child uses, shared rather than copied** (#491). A second copy of an
/// allowlist is a second oracle: it drifts, and the drift is silent because both spellings keep
/// working until the day one of them is short a name. So `workspace.rs`'s provisioning and removal
/// spawns call this instead of listing names again.
///
/// **What it does NOT do, and the difference is deliberate.** `run_in_workspace` composes `PATH`
/// from the caller's prepend directories before admitting it, because a tool may need to find a
/// pinned executable. Provisioning needs `git` findable and nothing else, so the ambient `PATH` is
/// admitted unchanged. Anything a caller wants set beyond the allowlist is set AFTER this returns
/// and wins, which is how the git-specific variables at the call sites survive.
pub(crate) fn scrub_environment(command: &mut std::process::Command) {
    command.env_clear();
    for name in INHERITED {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

const INHERITED: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "COMSPEC",
    "WINDIR",
];

/// The names the host redirects into the workspace rather than inheriting.
///
/// `CBM_CACHE_DIR` is #538 (D-042's last clause): a broker-run index provider's cache is confined
/// to the sandbox exactly the way HOME and TEMP are -- set by the host to a workspace path, denied
/// in `extra_env`, and structurally absent from inheritance via `env_clear`. Confinement by the
/// same mechanism as its siblings, so one sweep of this list answers "what does Tier 1 redirect".
const REDIRECTED: &[&str] = &[
    "HOME",
    "USERPROFILE",
    "TEMP",
    "TMP",
    "CBM_CACHE_DIR",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_KEY_0",
    "GIT_CONFIG_VALUE_0",
];

/// The one address a broker-run index provider reads its store from: the funnel sets
/// `CBM_CACHE_DIR` here (#538), so anything that must be VISIBLE to the provider — the pinned
/// snapshot's serving copy — must be placed here, by the session, before the spawn.
pub(crate) fn cbm_cache_dir(root: &Path) -> PathBuf {
    root.join(".cbm-cache")
}

/// The three directories the funnel creates INSIDE the workspace root for every spawn: the
/// child's redirected `HOME`/`USERPROFILE`, its `TEMP`/`TMP`, and the index provider's cache.
/// Inside an execution the root is the execution's own worktree, so whatever a shell or tests
/// call writes there — a `.cargo/credentials` under HOME, a build's temp files — sits beside the
/// tracked tree, and a `git add -A` would carry it into `refs/graphhelm/executions/<id>`
/// (#1073). [`RepositoryTool::commit`](crate::tools::RepositoryTool::commit) excludes exactly
/// these names by pathspec; keep the list and the `create_dir_all` calls below in step.
pub(crate) const WORKSPACE_SCRATCH_NAMES: [&str; 3] = [".home", ".tmp", ".cbm-cache"];

/// The fixed git posture: no system config, no prompts, no optional locks, and a synthetic
/// commit identity — `env_clear` plus an empty redirected HOME leaves git with no
/// `user.name`/`user.email` anywhere, and `git commit` would refuse with "Please tell me who
/// you are"; a fixed identity in the environment is the config-free, deterministic answer
/// (review finding 1 on this plan).
const FIXED_GIT: &[(&str, &str)] = &[
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_AUTHOR_NAME", "GraphHelm Tool Broker"),
    ("GIT_AUTHOR_EMAIL", "tools@graphhelm.invalid"),
    ("GIT_COMMITTER_NAME", "GraphHelm Tool Broker"),
    ("GIT_COMMITTER_EMAIL", "tools@graphhelm.invalid"),
];

/// Whether `name` may appear in `extra_env`. Case-insensitive: Windows environment names are
/// case-insensitive, so `Path` would shadow `PATH` just as surely. Refused classes: the
/// host's own passphrases (`GRAPHHELM_*`), and every name the host itself defines — an extra
/// `PATH` would swap program resolution out from under the lease's allowlist, an extra `HOME`
/// would undo the redirect (review finding 6).
fn extra_env_name_allowed(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if upper.starts_with("GRAPHHELM_") {
        return false;
    }
    if INHERITED.contains(&upper.as_str()) || REDIRECTED.contains(&upper.as_str()) {
        return false;
    }
    !FIXED_GIT.iter().any(|(fixed, _)| *fixed == upper)
}

/// Spawn a [`crate::verified::VerifiedExecutable`] in the workspace (#540): the program the
/// child runs IS the absolute path the verification hashed. Everything else — scrubbed
/// environment, deadline, caps — is [`run_in_workspace`], unchanged: one spawn funnel, one
/// verified doorway into it.
///
/// # Errors
/// Exactly [`run_in_workspace`]'s.
#[allow(clippy::too_many_arguments)]
pub fn run_verified_in_workspace(
    root: &Path,
    verified: &crate::verified::VerifiedExecutable,
    arguments: &[String],
    extra_env: &BTreeMap<String, String>,
    path_prepend: &[PathBuf],
    stdin_bytes: Option<&[u8]>,
    limits: &ProcessLimits,
    cancel: Option<&CancelSignal>,
) -> Result<CapturedProcess, HostError> {
    run_in_workspace(
        root,
        &verified.path().display().to_string(),
        arguments,
        extra_env,
        path_prepend,
        stdin_bytes,
        limits,
        cancel,
    )
}

/// Spawns `program` with `arguments` (argv only — never a shell) in `root`, with the scrubbed
/// environment, and captures its output under the limits.
///
/// Output beyond `max_output_bytes` per stream is discarded but the pipe is always drained —
/// a full pipe with no reader deadlocks the child, so the readers never stop reading, they
/// only stop keeping. On deadline the child is killed and reaped; the readers then see EOF.
/// (A grandchild holding the pipe open past the kill would delay that EOF — no builtin tool
/// spawns one and Task 8's proof exercises the real tools; the 05b runtime adapter's
/// detach-on-timeout pattern is the documented escalation if one ever appears.)
///
/// # Errors
/// [`HostError::ExtraEnvDenied`] before anything runs; [`HostError::Prepare`]/
/// [`HostError::Spawn`] if the workspace dirs or the process cannot be created.
/// Decrements the in-flight count however this spawn ends -- normal return, early error, or panic.
///
/// A manual decrement at the end of the function would be wrong on every path that returns before
/// it, and those paths exist: an unspawnable program, a broken pipe. A cancel waiting on a count
/// that a failed spawn never decremented would block for the full bound.
struct AttachedSpawn<'signal> {
    signal: &'signal CancelSignal,
    /// The entry this registration put in `observed`, so the SAME entry can be taken out again.
    ///
    /// A cell, because `record` runs behind `&self` -- the guard is held immutably for the life of
    /// the call. Single-threaded: only this call touches it.
    recorded: std::cell::RefCell<Option<std::sync::Arc<ObservedSpawn>>>,
}

impl AttachedSpawn<'_> {
    /// Record the child this registration was taken out for (#621).
    ///
    /// Separate from `attach` because the registration happens BEFORE the spawn — deliberately, so
    /// a cancellation cannot decide the count is zero while a child is being created (#609) — and
    /// the child does not exist until after it. Two steps because the ordering that makes the count
    /// correct is the ordering that makes the identity late.
    fn record(&self, process_id: u32) {
        // The identity is taken HERE, while the child is certainly alive, because that is the only
        // moment at which the binding can be made -- after the reap there is nothing left to bind
        // to. A capture that fails is recorded as a failure rather than replaced by the id.
        let identity = graphhelm_process_tree::ProcessIdentity::capture(process_id);
        let entry = std::sync::Arc::new(ObservedSpawn {
            process_id,
            identity,
        });
        if let Ok(mut observed) = self.signal.0.observed.lock() {
            observed.push(std::sync::Arc::clone(&entry));
        }
        *self.recorded.borrow_mut() = Some(entry);
    }
}

impl Drop for AttachedSpawn<'_> {
    fn drop(&mut self) {
        // The signal RELEASES its own reference when the call ends (Codex, on #680). The first
        // version kept every entry for the life of the host, so a long drive accumulated one OS
        // handle per completed child -- on Linux one pidfd each, until `spawn` fails with EMFILE.
        // The mechanism that made the question answerable was making the answer permanent.
        //
        // A caller that already took the entry keeps it alive through its own `Arc`, which is
        // exactly what the cancellation cell does: it clones before `cancel`, so it can still ask
        // after the call has finished and let go.
        if let Some(entry) = self.recorded.borrow_mut().take()
            && let Ok(mut observed) = self.signal.0.observed.lock()
        {
            observed.retain(|held| !std::sync::Arc::ptr_eq(held, &entry));
        }
        self.signal.leave();
    }
}

/// The hook-free directory one spawn points `core.hooksPath` at: a fresh sibling of the
/// workspace root, so it is under the staging area and outside the tree the child owns, named
/// by this process's id, a per-process random token and a process-wide counter so two hosts
/// sharing one staging area — or two calls of one host — never name the same directory.
/// Created with `create_dir`, which fails when the name already exists: a directory planted at a
/// guessed name refuses the call instead of supplying hooks to it. Removed on drop, that is,
/// when the call returns by any path.
struct NoHooksDir(PathBuf);

impl NoHooksDir {
    fn create(root: &Path) -> Result<Self, HostError> {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        static TOKEN: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
        let token = TOKEN.get_or_init(|| {
            use std::hash::{BuildHasher, Hasher};
            // `RandomState` is seeded per process from the OS; the hash of nothing is that seed
            // made visible, which is all this needs — unguessable across processes, no new dep.
            std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish()
        });
        let absolute = std::path::absolute(root).map_err(|source| HostError::Prepare { source })?;
        let parent = absolute.parent().ok_or_else(|| HostError::Prepare {
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a workspace root must have a parent to hold its hook-free directory",
            ),
        })?;
        let path = parent.join(format!(
            "ghtool-nohooks-{}-{token:016x}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).map_err(|source| HostError::Prepare { source })?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for NoHooksDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The eighth argument, and the alternative that was rejected (#180).
///
/// Folding `cancel` into [`ProcessLimits`] would keep the arity at seven and is coherent on its
/// face -- the timeout living there is a stop condition too, and `CancelSignal` is `Arc`-backed so
/// clones share rather than diverge. It was still rejected, for a cost that shows at the call
/// sites: `ProcessLimits` also travels to `RepositoryTool::read_file` and `list_files`, which are
/// Tier 0 and spawn NO child. Those paths would then carry a cancellation they structurally cannot
/// honour, and a signal that reaches a path unable to act on it is how #180 started.
///
/// So the parameter stays a parameter, every spawn path says whether it is cancellable, and the
/// lint is allowed here with that reason rather than silenced.
#[allow(clippy::too_many_arguments)]
pub fn run_in_workspace(
    root: &Path,
    program: &str,
    arguments: &[String],
    extra_env: &BTreeMap<String, String>,
    path_prepend: &[PathBuf],
    stdin_bytes: Option<&[u8]>,
    limits: &ProcessLimits,
    cancel: Option<&CancelSignal>,
) -> Result<CapturedProcess, HostError> {
    for name in extra_env.keys() {
        if !extra_env_name_allowed(name) {
            return Err(HostError::ExtraEnvDenied { name: name.clone() });
        }
    }

    let [home_name, tmp_name, _] = WORKSPACE_SCRATCH_NAMES;
    let home = root.join(home_name);
    let tmp = root.join(tmp_name);
    let cbm_cache = cbm_cache_dir(root);
    std::fs::create_dir_all(&home).map_err(|source| HostError::Prepare { source })?;
    std::fs::create_dir_all(&tmp).map_err(|source| HostError::Prepare { source })?;
    std::fs::create_dir_all(&cbm_cache).map_err(|source| HostError::Prepare { source })?;

    // Child PATH = path_prepend (host configuration, never caller input) ahead of the
    // parent's PATH, joined the OS way.
    let parent_path = std::env::var_os("PATH").unwrap_or_default();
    let composed_path = std::env::join_paths(
        path_prepend
            .iter()
            .map(PathBuf::as_path)
            .map(Path::to_path_buf)
            .chain(std::env::split_paths(&parent_path)),
    )
    // A join failure is a preparation failure, not a refused name (Task 5 review nit).
    .map_err(|_| HostError::Prepare {
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "unjoinable PATH entry"),
    })?;

    let mut command = std::process::Command::new(program);
    command
        .args(arguments)
        .current_dir(root)
        .env_clear()
        .stdin(if stdin_bytes.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in INHERITED {
        if *name == "PATH" {
            command.env("PATH", &composed_path);
        } else if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command.env("HOME", &home).env("USERPROFILE", &home);
    command.env("TEMP", &tmp).env("TMP", &tmp);
    command.env("CBM_CACHE_DIR", &cbm_cache);
    // Hooks disabled on EVERY spawn through this funnel, not only at provisioning (Codex, on
    // #1073, measured: the project's `pre-commit` ran on `git commit` and its
    // `reference-transaction` hook ran on `update-ref`). `core.hooksPath` is pointed at a
    // hook-free directory through the `GIT_CONFIG_*` environment triple git reads ahead of
    // every config file, so it applies to git however it is reached: directly, as the tests
    // runner, or from a program the allowlist admits that shells out to git. A non-git child
    // ignores the three names. The names are reserved from `extra_env` (`REDIRECTED`), so no
    // caller can point hooks back at the repository.
    //
    // The directory is OUTSIDE the workspace (Codex, on #1073, second pass, reproduced on git
    // 2.43): the first shape pointed it at the redirected HOME, which is `<root>/.home` — a
    // directory every untrusted child of the execution writes to and that persists across
    // calls, so a shell call planting an executable `.home/pre-commit` had it run by the next
    // brokered `git commit`. Now it is a fresh sibling of the root (under staging, never under
    // the tree a child owns), created for THIS spawn with `create_dir` — a name that already
    // exists refuses the call rather than trusting whatever is there — and removed when the
    // call returns. Handed to git in `git_safe` form: the root is canonical (`\?\` on
    // Windows) and git refuses that prefix.
    let no_hooks = NoHooksDir::create(root)?;
    command
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env(
            "GIT_CONFIG_VALUE_0",
            crate::workspace::git_safe(no_hooks.path()),
        );
    for (name, value) in FIXED_GIT {
        command.env(name, value);
    }
    command.envs(extra_env);

    // Registered BEFORE the spawn, so cancellation cannot decide the count is zero in the window
    // between a child existing and this function admitting it exists (Codex, #609). The first
    // version registered nineteen lines lower, after `spawn` — a cancel landing in that window saw
    // `in_flight == 0`, returned claiming the reap was done, and the child attached afterwards and
    // ran until the next poll. A hole at the entrance of the exact guarantee this type gives.
    //
    // Refusal rather than a killed-on-arrival record, because the same window has a second shape:
    // `RepositoryTool::commit` calls this twice, and between the two the count is legitimately
    // zero. Without the refusal the second `git` starts after `cancel` has already returned.
    let _attached = match cancel {
        Some(signal) => match signal.attach() {
            Some(guard) => Some(guard),
            None => return Err(HostError::Cancelled),
        },
        None => None,
    };

    // #618: the child is prepared to be killed as a TREE, not alone. On Unix this puts it in its
    // own process group; on Windows it starts SUSPENDED so the job object can be assigned before it
    // runs — a child that ran first could spawn descendants outside the job, which is the hole.
    graphhelm_process_tree::configure(&mut command);

    let mut child = command
        .spawn()
        .map_err(|source| HostError::Spawn { source })?;

    // Assigned immediately, and on Windows this also RESUMES the suspended child. Failing here
    // leaves a child that cannot be killed as a tree, so the call refuses rather than proceeding
    // with a weaker guarantee than it advertises — and the child is reaped on the way out.
    let mut group = match graphhelm_process_tree::create(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HostError::ProcessGroup {
                rule: match error {
                    graphhelm_process_tree::ProcessTreeError::JobSetup => "job_setup",
                    graphhelm_process_tree::ProcessTreeError::ProcessResume => "process_resume",
                    // #878: the child reached `create` unsuspended, so it ran before the job
                    // could contain it and anything it started is outside. The funnel here DOES
                    // call `configure`, so this arm exists to keep the compiler forcing the
                    // decision at every call site rather than because this one can reach it.
                    graphhelm_process_tree::ProcessTreeError::ChildNotSuspended => {
                        "child_not_suspended"
                    }
                },
            });
        }
    };

    // The id, recorded the moment it exists, so a cell can ask about LIVENESS instead of sampling
    // bytes across a wall-clock window (#621). Nothing in production reads it; the seam exists
    // because the property "no live child" has no other observer from outside this function.
    if let Some(attached) = _attached.as_ref() {
        attached.record(child.id());
    }

    // Stdin is the one per-tool exception to null (a patch travels here). The write runs on
    // its own thread so an input larger than the OS pipe buffer can never wedge this thread
    // past the deadline; a kill mid-write surfaces as BrokenPipe, which is exactly the
    // uninteresting outcome — ignore it and let the disposition speak.
    let stdin_writer = stdin_bytes.map(|bytes| {
        let mut pipe = child.stdin.take().expect("stdin was piped");
        let owned = bytes.to_vec();
        std::thread::spawn(move || {
            use std::io::Write as _;
            let _ = pipe.write_all(&owned);
        })
    });

    let cap = limits.max_output_bytes;
    let stdout_pipe = child.stdout.take().expect("stdout was piped");
    let stderr_pipe = child.stderr.take().expect("stderr was piped");
    #[cfg(windows)]
    let (stdout_raw, stderr_raw) = (
        RawPipeHandle(std::os::windows::io::AsRawHandle::as_raw_handle(
            &stdout_pipe,
        )),
        RawPipeHandle(std::os::windows::io::AsRawHandle::as_raw_handle(
            &stderr_pipe,
        )),
    );
    #[cfg(unix)]
    let (stdout_raw, stderr_raw) = (
        RawPipeHandle(std::os::unix::io::AsRawFd::as_raw_fd(&stdout_pipe)),
        RawPipeHandle(std::os::unix::io::AsRawFd::as_raw_fd(&stderr_pipe)),
    );
    let stdout_reader = spawn_reader(Box::new(stdout_pipe), stdout_raw, cap);
    let stderr_reader = spawn_reader(Box::new(stderr_pipe), stderr_raw, cap);

    // The 05b runtime-adapter pattern: poll every 50 ms against the deadline; on expiry kill
    // and reap, never leaving a zombie.
    let deadline = Instant::now() + limits.timeout;
    let mut timed_out = false;
    let mut was_cancelled = false;
    // `None` until a kill is ATTEMPTED. A child that exits on its own is never swept, and starting
    // this at `Complete` would report a measurement that never ran (#748).
    let mut tree_kill = None;
    // Set when the poll itself failed, which is the only way this loop leaves without having seen
    // the child exit. It decides whether the reap far below is a MEASUREMENT or merely cleanup:
    // the record must not report an exit code from a wait that answered a question nobody asked.
    let mut poll_failed = false;
    // NOT `try_wait`, and the whole of #714's identity half is in that substitution. `try_wait`
    // reaps; the group released at the end of this function is named by the leader's pid; a reaped
    // pid is one the kernel may hand to any other job of the same user. Between the exit and the
    // release sits the writer join and the entire drain -- five seconds of READER_SILENCE_GRACE on
    // the path this was measured on -- and a `SIGKILL` at the end of it. So the leader is held as
    // an unreaped anchor for that whole span and reaped only after `close` has run.
    loop {
        match graphhelm_process_tree::leader_exited(&mut child) {
            Ok(true) => break,
            Ok(false) => {
                let expired = Instant::now() >= deadline;
                let cancelled = cancel.is_some_and(CancelSignal::is_cancelled);
                // #180: cancellation kills through the SAME reaping path as the deadline. The
                // two differ only in `timed_out`, because a cancelled call did not run out of
                // time -- recording it as a timeout would put a false cause in the record.
                if expired || cancelled {
                    // #618: the TREE, not the child. `Child::kill` ends the direct process and
                    // leaves anything it spawned reparented and running — and a descendant that
                    // inherited stdout or stderr holds those pipes open, so the reader joins below
                    // block after the direct child is already reaped. The leak was the visible
                    // half; the wedged caller was the one that mattered.
                    //
                    // #748: KEPT, not dropped. This is the only `terminate` in this crate whose
                    // caller has a record to put the answer in, and until it did, a
                    // `BoundReached` -- descendants still appearing when the sweep ran out of
                    // passes -- reached the caller as a `CapturedProcess` indistinguishable from
                    // a clean tree. The sweep landed in #805; this is where its conclusion stops
                    // being thrown away.
                    tree_kill = Some(graphhelm_process_tree::terminate(
                        child.id(),
                        graphhelm_process_tree::for_thread(group),
                    ));
                    // Waited out WITHOUT reaping, for the reason above: `terminate` has signalled
                    // the tree, but the release at the end of this function still has to reach
                    // whatever `terminate`'s subtree sweep could not, and it needs the pgid to
                    // still be this child's.
                    let _ = graphhelm_process_tree::await_leader_exit(&mut child);
                    // Cancellation WINS when both are true (Codex, #609). A cancel raised inside
                    // the last poll interval before the deadline leaves both conditions true at
                    // the same poll, and the two mistakes are not equally bad: blaming the clock
                    // invents a fault nobody committed, while crediting the cancel names an act
                    // that certainly happened. Ordering them would take a second clock, so the
                    // record takes the side that cannot fabricate.
                    timed_out = expired && !cancelled;
                    // And the cause is CARRIED, not merely withheld (Codex, #609). Clearing
                    // `timed_out` takes the false cause out of the record; without this the
                    // disposition then falls through to the exit code, which a killed child has.
                    was_cancelled = cancelled;
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => {
                poll_failed = true;
                break;
            }
        }
    }

    if let Some(writer) = stdin_writer {
        let _ = writer.join();
    }
    // BOUNDED, and the bound is a backstop rather than a normal path (#618). A reader thread blocks
    // in `read` until every writer end of the pipe closes -- and on Windows a descendant receives
    // the parent's pipe handles even when its own stdio is null, because `Command` spawns with
    // `bInheritHandles = TRUE`. So a descendant that escaped the kill holds the pipe open and this
    // join never returns: the call wedges, and a wedged call is worse than a failed one because
    // nothing reports it.
    //
    // With the tree kill above, that should not happen. This deadline firing is therefore a SIGNAL
    // that something escaped the job, and `readers_abandoned` carries it rather than being folded
    // into `truncated` -- fusing two causes into one flag is the defect #177 was about.
    let reader_deadline = drain_deadline(deadline, Instant::now());
    // The group is released by the DRAIN, not here, and the order is the whole safety of it. On
    // Windows the job object carries `KILL_ON_JOB_CLOSE`, so closing it while a descendant still
    // holds a pipe kills that descendant out from under a reader that is mid-read. The drain
    // releases only once every still-pending stream has gone silent -- which is exactly the state
    // in which killing the holder costs nothing -- or once the deadline above has passed anyway.
    //
    // #708: and once it has released, it waits AGAIN, briefly. Forcing the EOF is what lets a
    // blocked reader answer at all, and the previous code did it in the other order: it gave up on
    // the reader, substituted an empty buffer, and only then released the group. The bytes existed
    // and the reader was holding them.
    let drained = {
        let mut release = || graphhelm_process_tree::close(&mut group);
        drain_readers(
            stdout_reader,
            stderr_reader,
            reader_deadline,
            READER_SILENCE_GRACE,
            DRAIN_POLL,
            POST_RELEASE_GRACE,
            &mut release,
        )
    };
    if !drained.released {
        graphhelm_process_tree::close(&mut group);
    }
    // THE REAP, and its position is the fix (#714). Every `close` above has run, so the pgid they
    // signalled was still this child's pid throughout; only now is that number given back to the
    // kernel. Moving this line earlier -- which is what `try_wait` in the poll loop used to do
    // implicitly -- reopens the window in which the release can kill an unrelated process.
    //
    // `poll_failed` keeps the old record: that path never established an exit, and the wait here
    // is cleanup rather than an answer, so it must not become an exit code.
    let reaped = child.wait().ok();
    let exit_status = if poll_failed { None } else { reaped };
    let (stdout, stdout_truncated) = drained.stdout;
    let (stderr, stderr_truncated) = drained.stderr;
    let readers_abandoned = drained.abandoned;
    let reader_lost = drained.reader_lost;

    Ok(CapturedProcess {
        exit_code: exit_status.and_then(|status| status.code()),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        truncated: stdout_truncated || stderr_truncated,
        timed_out,
        cancelled: was_cancelled,
        readers_abandoned,
        reader_lost,
        tree_kill,
    })
}

/// The elision sentence, built in ONE place so the budgeted length and the emitted length can never
/// be computed from different values of `elided` (D's finding 2 on #582).
fn elision_marker(elided: u64) -> Vec<u8> {
    format!(
        "
[... {elided} bytes elided ...]
"
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::{
        CancelSignal, CapturedProcess, DrainedReaders, HostError, LIVE_READER_THREADS,
        MINIMUM_DRAIN, ReaderHandle, drain_deadline, drain_readers, reject_lost_capture,
        terminate_close_reap,
    };
    use std::io::Read;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{Receiver, Sender, channel};
    use std::time::{Duration, Instant};

    /// A stand-in for one pipe reader: the channel it answers on, and the counter it bumps.
    ///
    /// Hand-driven on purpose. The state `drain_readers` exists for -- a descendant that outlived
    /// the tree kill and is holding a pipe open -- cannot be staged on Windows, whose job object
    /// has no breakaway (#717), and staging it on Unix means sabotaging the kill. So the mechanism
    /// is exercised at the seam instead of through the escape, and what these cells measure is the
    /// WAIT PROTOCOL, not the containment failure that reaches it. `tests/process_isolation.rs`
    /// owns that end.
    struct FakeReader {
        sender: Sender<(Vec<u8>, bool)>,
        receiver: Option<Receiver<(Vec<u8>, bool)>>,
        progress: Arc<AtomicU64>,
        give_up: Arc<AtomicBool>,
    }

    impl FakeReader {
        fn new() -> Self {
            let (sender, receiver) = channel();
            Self {
                sender,
                receiver: Some(receiver),
                progress: Arc::new(AtomicU64::new(0)),
                give_up: Arc::new(AtomicBool::new(false)),
            }
        }

        fn stream(&mut self) -> ReaderHandle {
            ReaderHandle {
                receiver: self.receiver.take().expect("the stream is taken once"),
                progress: Arc::clone(&self.progress),
                give_up: Arc::clone(&self.give_up),
            }
        }

        fn answer(&self, bytes: &[u8], truncated: bool) {
            self.sender
                .send((bytes.to_vec(), truncated))
                .expect("the drain is still listening");
        }

        /// Whether `drain_readers` told this reader's (fake) thread to give up (#726) -- the
        /// signal a real `spawn_reader` would use to end its poll loop.
        fn was_told_to_abandon(&self) -> bool {
            self.give_up.load(Ordering::Relaxed)
        }
    }

    /// Short, because every assertion below is about the OUTCOME rather than about how long it
    /// took. A slow host makes these cells slower, never redder.
    const GRACE: Duration = Duration::from_millis(200);
    const POLL: Duration = Duration::from_millis(10);
    const POST: Duration = Duration::from_secs(2);

    #[test]
    fn tree_cleanup_terminates_closes_then_reaps() {
        let operations = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let terminate_operations = std::rc::Rc::clone(&operations);
        let close_operations = std::rc::Rc::clone(&operations);
        let reap_operations = std::rc::Rc::clone(&operations);
        terminate_close_reap(
            || terminate_operations.borrow_mut().push("terminate"),
            || close_operations.borrow_mut().push("close"),
            || reap_operations.borrow_mut().push("reap"),
        );
        assert_eq!(*operations.borrow(), ["terminate", "close", "reap"]);
    }

    fn far_deadline() -> Instant {
        Instant::now() + Duration::from_secs(120)
    }

    /// A raised signal refuses a HOLD, and this cell exists because the integration one does not
    /// separate the two ways a provision can end in `Cancelled`.
    ///
    /// Measured, not supposed: deleting the raised-flag check from `hold` leaves
    /// `a_raised_signal_refuses_a_new_provision` GREEN. With the check gone the span is granted,
    /// `provision` goes on to spawn, and `run_supervised` -- interruptible, signal already raised
    /// -- kills it on the first poll and returns `Cancelled` anyway. Same error, different
    /// mechanism, and an assertion on the error cannot tell them apart. So the refusal gets a cell
    /// whose subject is `hold` itself.
    #[test]
    fn a_raised_signal_refuses_a_hold_and_an_unraised_one_grants_it() {
        let signal = CancelSignal::new();
        assert!(
            signal.hold().is_some(),
            "CONTROL: an unraised signal must grant the span, or the refusal below is a function that never grants anything"
        );

        signal.cancel();

        assert!(
            signal.hold().is_none(),
            "a span begun after the cancellation read the count is invisible to the caller cancel already answered"
        );
    }

    /// The normal path: both readers answer, nothing is forced, the group is not released here.
    ///
    /// CONTROL for every cell below. Without it, a `drain_readers` that released on every call
    /// would satisfy the forced-EOF cell and look like the fix.
    #[test]
    fn two_readers_that_answer_are_collected_without_releasing_the_group() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());
        out.answer(b"stdout bytes", false);
        err.answer(b"stderr bytes", true);

        let mut released = 0_u32;
        let drained = drain_readers(
            streams.0,
            streams.1,
            far_deadline(),
            GRACE,
            POLL,
            POST,
            &mut || released += 1,
        );

        assert_eq!(
            released, 0,
            "a healthy call must not release the group early"
        );
        assert!(
            !drained.released && !drained.abandoned,
            "a healthy call reports neither a release nor an escape"
        );
        assert_eq!(drained.stdout, (b"stdout bytes".to_vec(), false));
        assert_eq!(drained.stderr, (b"stderr bytes".to_vec(), true));
    }

    /// #708, the byte-keeping half: a stream that answers only once the group is released still
    /// has its capture KEPT.
    ///
    /// Before this change the drain gave up on such a reader, substituted an EMPTY buffer, and
    /// released the group afterwards -- so the bytes the reader was holding died with the call.
    /// The release callback here sends the partial, which is what closing the job really does:
    /// it shuts the descendant's end, the blocked `read` returns 0, and the reader sends what it
    /// had.
    #[test]
    fn a_silent_stream_is_forced_to_eof_and_its_partial_capture_survives() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());
        err.answer(b"stderr finished", false);
        // It read something, then stalled: the capture that today would be thrown away.
        out.progress.store(9, Ordering::Relaxed);

        let drained = drain_readers(
            streams.0,
            streams.1,
            far_deadline(),
            GRACE,
            POLL,
            POST,
            &mut || out.answer(b"partial!!", false),
        );

        assert!(
            drained.released,
            "a stream still pending after its silence grace must have the group released for it"
        );
        assert_eq!(
            drained.stdout,
            (b"partial!!".to_vec(), false),
            "the bytes the reader was holding must survive the forced EOF; an empty buffer here is \
             the defect #708 names"
        );
        assert_eq!(
            drained.stderr,
            (b"stderr finished".to_vec(), false),
            "the stream that answered normally is untouched by the other one being forced"
        );
        assert!(
            drained.abandoned,
            "a recovered PARTIAL capture is still an escape: the flag reports containment, not \
             byte counts"
        );
    }

    /// #708's refinement: the grace expires on SILENCE, not on elapsed time.
    ///
    /// A descendant that is legitimately still writing must never be cut, and a flat grace cuts
    /// it. This reader says nothing on its channel for well over the grace while its counter keeps
    /// moving, exactly as a tests runner's worker does, and only then answers.
    ///
    /// The cell can be voided by the host rather than by the code: if the bumper thread is
    /// descheduled for longer than the grace, the drain is RIGHT to release and the run has
    /// measured the scheduler. It says so in its own words instead of failing, because a false red
    /// here would read as "the silence reset does not work".
    #[test]
    fn a_stream_that_keeps_producing_is_never_cut_by_the_silence_grace() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());
        err.answer(b"", false);

        let progress = Arc::clone(&out.progress);
        let widest_gap = Arc::new(AtomicU64::new(0));
        let observed = Arc::clone(&widest_gap);
        let answer = out.sender.clone();
        let busy = std::thread::spawn(move || {
            let mut last = Instant::now();
            // Comfortably longer than GRACE, so a drain that ignored the counter would have
            // released several times over by the end of it.
            let until = last + GRACE * 4;
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
                let now = Instant::now();
                let gap = u64::try_from(now.duration_since(last).as_millis()).unwrap_or(u64::MAX);
                observed.fetch_max(gap, Ordering::Relaxed);
                last = now;
                progress.fetch_add(4096, Ordering::Relaxed);
            }
            // It finishes on its own, the way a descendant that was merely SLOW does. Without
            // this the stream falls silent at the end and the release that follows is correct --
            // the cell would then be measuring its own arrangement rather than the reset.
            let _ = answer.send((b"finished on its own".to_vec(), false));
        });

        let mut released = 0_u32;
        let drained = drain_readers(
            streams.0,
            streams.1,
            far_deadline(),
            GRACE,
            POLL,
            POST,
            &mut || released += 1,
        );
        busy.join().expect("the producing thread returns");

        let gap = widest_gap.load(Ordering::Relaxed);
        let grace_ms = u64::try_from(GRACE.as_millis()).unwrap_or(u64::MAX);
        if released > 0 && gap >= grace_ms {
            // Not a verdict: the producer really did go silent for longer than the grace, so
            // releasing was correct and this run measured the host, not the mechanism.
            eprintln!(
                "HARNESS-BROKE: the producing thread was descheduled for {gap}ms against a {grace_ms}ms \
                 grace, so this run decides nothing about the silence reset"
            );
            return;
        }
        assert_eq!(
            released, 0,
            "a stream whose byte counter kept moving was cut anyway: the grace is expiring on \
             elapsed time rather than on silence (widest observed gap {gap}ms against a \
             {grace_ms}ms grace)"
        );
        assert!(
            !drained.abandoned,
            "nothing escaped, so nothing is abandoned"
        );
        assert_eq!(
            drained.stdout,
            (b"finished on its own".to_vec(), false),
            "the slow stream's own answer must be the one kept"
        );
    }

    /// The hard deadline still bounds a stream that never goes silent.
    ///
    /// The silence grace only ever ends the wait EARLIER. Without this, a descendant writing
    /// forever would hold the call open past the caller's own budget -- the defect
    /// `drain_deadline` was written to close, reintroduced one layer up.
    #[test]
    fn a_stream_that_never_goes_silent_is_still_bounded_by_the_deadline() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());
        err.answer(b"", false);

        let progress = Arc::clone(&out.progress);
        let stop = Arc::new(AtomicU64::new(0));
        let watch = Arc::clone(&stop);
        let busy = std::thread::spawn(move || {
            while watch.load(Ordering::Relaxed) == 0 {
                progress.fetch_add(1, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(5));
            }
        });

        let started = Instant::now();
        let deadline = started + Duration::from_millis(300);
        let drained = drain_readers(
            streams.0,
            streams.1,
            deadline,
            // A grace it can never reach, so ONLY the deadline can end this wait.
            Duration::from_secs(3600),
            POLL,
            Duration::from_millis(50),
            &mut || {},
        );
        stop.store(1, Ordering::Relaxed);
        busy.join().expect("the producing thread returns");

        assert!(
            drained.released && drained.abandoned,
            "the deadline must end the wait and force the EOF, or a forever-writing descendant \
             holds the call open past the caller's budget"
        );
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the drain ran for {:?}, which is not bounded by its deadline",
            started.elapsed()
        );
    }

    /// One silent stream must not release the group while the OTHER one is still producing.
    ///
    /// This is the `all` in the silence test rather than an `any`, and it needs its own cell
    /// because the two spellings agree on every arrangement where only one stream is pending.
    /// stderr is quiet for most of a healthy run: an any-of test would release the group -- and on
    /// Windows kill whatever holds the pipe -- while stdout was mid-stream, which is the exact
    /// truncation the silence grace exists to prevent, reached from the opposite direction.
    #[test]
    fn a_quiet_stream_does_not_release_the_group_while_the_other_is_still_producing() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());

        let progress = Arc::clone(&out.progress);
        let noisy = out.sender.clone();
        let quiet = err.sender.clone();
        let busy = std::thread::spawn(move || {
            let until = Instant::now() + GRACE * 4;
            while Instant::now() < until {
                std::thread::sleep(Duration::from_millis(10));
                progress.fetch_add(4096, Ordering::Relaxed);
            }
            // Both end on their own. stderr never said a word until here -- it was pending and
            // silent for the whole run, which is what an any-of test would have released on.
            let _ = noisy.send((b"stdout finished".to_vec(), false));
            let _ = quiet.send((Vec::new(), false));
        });

        let mut released = 0_u32;
        let drained = drain_readers(
            streams.0,
            streams.1,
            far_deadline(),
            GRACE,
            POLL,
            POST,
            &mut || released += 1,
        );
        busy.join().expect("the producing thread returns");

        assert_eq!(
            released, 0,
            "a stream that was silent the whole time released the group while the other was \
             still producing: the silence test is an any-of where it must be an all-of"
        );
        assert!(!drained.abandoned);
        assert_eq!(drained.stdout, (b"stdout finished".to_vec(), false));
    }

    /// Each pending stream gets its OWN post-release grace, not a share of one window (#788).
    ///
    /// The window used to be computed once, outside the collect loop. A first stream that used
    /// most of it left the second with a near-zero timeout, so the second's answer was discarded
    /// even though the release had already turned its held pipe into an EOF and its reader was on
    /// the way to send. That is #708's own defect at the last step, and it lands on the asymmetric
    /// case this design worries about, because stdout is the big stream and is waited on first.
    ///
    /// The arrangement makes the two answers arrive at 3/4 and 5/4 of the grace. Under one shared
    /// window the second is 1/4 of a grace past the shared expiry and is lost; under a per-stream
    /// one it is comfortably inside its own.
    ///
    /// **The two assertions do NOT have equal slack, and the FIRST is the one load will break**
    /// (#793 -- the first version of this comment said the opposite and would have sent someone
    /// debugging a load-induced red to the wrong place). stdout is sent at 3/4 of the grace against
    /// its own full grace, so it has 1/4 of one -- 100 ms -- to spare; stderr is sent at 5/4 against
    /// a window that starts when its wait does, so it has 200 ms. A slow host reddens the stdout
    /// assertion first.
    ///
    /// That costs the cell nothing, and saying why is the point: BOTH failure directions are red.
    /// A host too slow for stdout reds the first assertion; a shared window reds the second. There
    /// is no host speed at which the sabotaged shape passes, because under it stderr's budget is at
    /// most 1/4 of a grace while it is always sent a full 1/2 grace after stdout.
    #[test]
    fn each_stream_gets_its_own_post_release_grace_not_a_share_of_one() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());

        let grace = Duration::from_millis(400);
        // Cloned INSIDE the closure so it stays `FnMut`: `drain_readers` takes the callback by
        // `&mut dyn FnMut()` because a caller could in principle release more than once, and a
        // closure that moves its captures is `FnOnce`.
        let mut release = || {
            let slow_stdout = out.sender.clone();
            let later_stderr = err.sender.clone();
            // Releasing the group is what turns both held pipes into an EOF. The readers then
            // answer on their own schedule -- here, one after the other.
            std::thread::spawn(move || {
                std::thread::sleep(grace * 3 / 4);
                let _ = slow_stdout.send((b"stdout, late".to_vec(), false));
                std::thread::sleep(grace / 2);
                let _ = later_stderr.send((b"stderr, later".to_vec(), false));
            });
        };

        let drained = drain_readers(
            streams.0,
            streams.1,
            Instant::now(),
            GRACE,
            POLL,
            grace,
            &mut release,
        );

        assert_eq!(
            drained.stdout,
            (b"stdout, late".to_vec(), false),
            "the first stream answered inside the grace and must be kept"
        );
        assert_eq!(
            drained.stderr,
            (b"stderr, later".to_vec(), false),
            "the second stream answered inside ITS OWN grace and was still lost: the window is shared, so the first stream spent it"
        );
        assert!(drained.released && drained.abandoned);
    }

    /// The session seam REFUSES a capture whose reader was lost, and not only one whose readers
    /// were abandoned (#790).
    ///
    /// **This is the boundary cell, and the producer cell below cannot stand in for it.** That one
    /// asserts on `drained.reader_lost` -- a FIELD -- which stays true whether or not anything
    /// reads it. Deleting this arm today reddens nothing, measured: with all three consumers
    /// removed and the producer left correct, the whole crate is green.
    ///
    /// This seam is the one that matters most of the three, by its own doc's argument: its
    /// consumers HASH THE BYTES, and an empty stream that was never read hashes to the digest of
    /// zero bytes -- indistinguishable from a tool that printed nothing. A lost capture reaching
    /// them is a fabricated digest presented as a real one.
    #[test]
    fn the_session_seam_refuses_a_capture_whose_reader_was_lost() {
        let lost = CapturedProcess {
            exit_code: Some(0),
            stdout: b"bytes a consumer would have hashed".to_vec(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            truncated: false,
            timed_out: false,
            cancelled: false,
            readers_abandoned: false,
            reader_lost: true,
            tree_kill: None,
        };
        assert!(
            matches!(
                reject_lost_capture(lost),
                Err(HostError::CaptureLost { .. })
            ),
            "a capture whose reader ended without answering must be refused at the seam, not \
             handed to a consumer that will hash it"
        );
    }

    /// CONTROL for the cell above: an ORDINARY capture still passes through.
    ///
    /// Without it, a `reject_lost_capture` that refused every input would satisfy the refusal
    /// above while making the seam useless -- and `readers_abandoned` must remain a separate
    /// reason rather than being folded in.
    #[test]
    fn the_session_seam_passes_an_ordinary_capture_through() {
        let ordinary = CapturedProcess {
            exit_code: Some(0),
            stdout: b"ordinary output".to_vec(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            truncated: false,
            timed_out: false,
            cancelled: false,
            readers_abandoned: false,
            reader_lost: false,
            tree_kill: None,
        };
        let passed = reject_lost_capture(ordinary).expect("an ordinary capture is not refused");
        assert_eq!(passed.stdout, b"ordinary output".to_vec());
    }

    /// A reader whose sender is dropped WITHOUT an answer is reported as lost, and is not
    /// confused with a tool that printed nothing (#790).
    ///
    /// The sender is dropped rather than a panic staged, which is the same choice every other
    /// cell here makes: the protocol is what is under test, and a real panic inside a reader
    /// thread would be a fixture about the unwinder rather than about the drain.
    ///
    /// Four assertions and only the first is the new behaviour. The other three are the ticket's
    /// own acceptance criteria written as controls: `abandoned` must stay FALSE, because it is a
    /// containment claim that `tests/process_isolation.rs` asserts against and a dead reader
    /// escaped nothing; the buffer must stay empty and UNTRUNCATED, because those defaults are
    /// right for a genuinely empty stream and the new state must be the only thing that changed;
    /// and the healthy stream beside it must come through whole.
    #[test]
    fn a_reader_whose_sender_is_dropped_without_answering_is_reported_as_lost() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());
        err.answer(b"stderr answered normally", false);
        // The reader thread ended before its single send site. Its receiver now sees
        // `Disconnected` rather than a timeout, which is the ONLY way that arm is reached.
        drop(out);

        let drained = drain_readers(
            streams.0,
            streams.1,
            far_deadline(),
            GRACE,
            POLL,
            POST,
            &mut || {},
        );

        assert!(
            drained.reader_lost,
            "a sender dropped without an answer must be reported: without this the caller cannot tell an empty capture from one whose reader died holding the output"
        );
        assert!(
            !drained.abandoned,
            "a dead reader escaped NOTHING -- `abandoned` is a containment claim and widening it here would make process_isolation.rs fail for a reason unrelated to containment"
        );
        assert_eq!(
            drained.stdout,
            (Vec::new(), false),
            "empty and NOT truncated: those defaults are correct for a genuinely empty stream, so the new state must be the only thing this changes"
        );
        assert_eq!(
            drained.stderr,
            (b"stderr answered normally".to_vec(), false),
            "the healthy stream beside it is untouched"
        );
    }

    /// A reader that answers NEITHER before nor after the release is reported, not guessed at.
    ///
    /// This is the outcome the old code produced for every abandoned stream, and it stays
    /// reachable: an empty capture, `truncated` false because no bytes were dropped that this call
    /// can account for, and `abandoned` true because the capture is incomplete.
    #[test]
    fn a_reader_that_never_answers_yields_an_empty_capture_flagged_as_abandoned() {
        let mut out = FakeReader::new();
        let mut err = FakeReader::new();
        let streams = (out.stream(), err.stream());
        err.answer(b"", false);

        let DrainedReaders {
            stdout,
            abandoned,
            released,
            ..
        } = drain_readers(
            streams.0,
            streams.1,
            far_deadline(),
            GRACE,
            POLL,
            Duration::from_millis(50),
            &mut || {},
        );

        assert!(released && abandoned);
        assert_eq!(
            stdout,
            (Vec::new(), false),
            "an unanswered stream is empty and NOT marked truncated: truncated means bytes were \
             read and dropped, and none were"
        );
        // #726: the drain does not just RECORD the escape, it now TELLS the stuck reader so its
        // own thread can end. Only the stream that never answered is told -- the healthy one
        // beside it must not be, or a real reader would be cut off mid-flight for no reason.
        assert!(
            out.was_told_to_abandon(),
            "the stream that never answered, even after the release and its grace, must be \
             signalled so its thread can end"
        );
        assert!(
            !err.was_told_to_abandon(),
            "a stream that answered normally must never be told to abandon"
        );
    }

    /// The defect this replaced: the drain used to start a fresh clock, so a call that spent almost
    /// all of its budget and then hit a held pipe could take nearly TWICE its timeout to return.
    /// Reusing the invocation deadline bounds the total at the deadline itself.
    #[test]
    fn a_child_that_exits_early_drains_only_until_the_original_deadline() {
        let now = Instant::now();
        let deadline = now + Duration::from_secs(120);
        // The child exited at once, so `now` here is near the start of the budget.
        let drain_ends = drain_deadline(deadline, now);
        assert_eq!(
            drain_ends, deadline,
            "the drain ran past the deadline the caller chose"
        );
    }

    /// And the floor, which is what stops the reuse from degenerating: a child that ran ALL the way
    /// to its deadline leaves zero remaining budget, and without the floor every timed-out call
    /// would report a capture loss produced by the arithmetic rather than by an escaped process.
    #[test]
    fn a_child_that_used_its_whole_budget_still_gets_the_floor() {
        let deadline = Instant::now();
        let now = deadline + Duration::from_millis(5);
        assert_eq!(drain_deadline(deadline, now), now + MINIMUM_DRAIN);
    }

    /// The bound the reviewer asked for, stated as the property rather than as an example: the
    /// total can never be a MULTIPLE of the caller's timeout, only the timeout plus the floor.
    #[test]
    fn the_total_never_exceeds_the_timeout_plus_the_floor() {
        let start = Instant::now();
        for timeout in [
            Duration::from_millis(50),
            Duration::from_secs(2),
            Duration::from_secs(120),
        ] {
            let deadline = start + timeout;
            // Worst case for the total: the child exits at the very last instant of its budget.
            let drain_ends = drain_deadline(deadline, deadline);
            assert!(
                drain_ends <= deadline + MINIMUM_DRAIN,
                "a {timeout:?} call could run until {:?} past its deadline",
                drain_ends - deadline
            );
        }
    }

    /// A synthetic pipe pair, in place of a real escaped descendant. Neither platform has a way to
    /// stage the real thing reliably from inside this suite: Windows has no breakaway from this
    /// job at all (#717, per the comment on `FakeReader` above), and on Unix a descendant that
    /// escapes via `setsid` is now reaped by #805's subreaper before it can hold the pipe open --
    /// measured directly (`fake_tool`'s own `spawn-escaping-grandchild`, run under WSL2, inherits
    /// no copy of the pipe: `Stdio::null()` on the grandchild's own 0/1/2 leaves nothing else
    /// pointing at it, unlike Windows' `bInheritHandles = TRUE`). So on both platforms the
    /// mechanism under test -- does the READER THREAD itself end once told to give up -- is
    /// exercised directly: the test owns both ends and controls exactly when, if ever, the write
    /// end closes.
    #[cfg(windows)]
    fn synthetic_pipe() -> (Box<dyn Read + Send>, super::RawPipeHandle, std::fs::File) {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;

        let mut read_handle: HANDLE = std::ptr::null_mut();
        let mut write_handle: HANDLE = std::ptr::null_mut();
        let ok = unsafe {
            windows_sys::Win32::System::Pipes::CreatePipe(
                std::ptr::addr_of_mut!(read_handle),
                std::ptr::addr_of_mut!(write_handle),
                std::ptr::null(),
                0,
            )
        };
        assert_ne!(
            ok, 0,
            "CreatePipe must succeed for the test to mean anything"
        );
        let raw = super::RawPipeHandle(read_handle as std::os::windows::io::RawHandle);
        // SAFETY: both handles came from `CreatePipe` above and are owned exactly once each --
        // `read_file` by the `File` that will be boxed into the reader, `write_file` by the one
        // returned to the caller. Neither handle is used or closed anywhere else.
        let read_file = unsafe {
            std::fs::File::from_raw_handle(read_handle as std::os::windows::io::RawHandle)
        };
        let write_file = unsafe {
            std::fs::File::from_raw_handle(write_handle as std::os::windows::io::RawHandle)
        };
        (Box::new(read_file), raw, write_file)
    }

    #[cfg(unix)]
    fn synthetic_pipe() -> (Box<dyn Read + Send>, super::RawPipeHandle, std::fs::File) {
        use std::os::unix::io::FromRawFd;

        let mut fds: [std::os::unix::io::RawFd; 2] = [0; 2];
        let ok = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(ok, 0, "pipe(2) must succeed for the test to mean anything");
        let [read_fd, write_fd] = fds;
        let raw = super::RawPipeHandle(read_fd);
        // SAFETY: both fds came from `pipe(2)` above and are owned exactly once each -- `read_file`
        // by the `File` that will be boxed into the reader, `write_file` by the one returned to the
        // caller. Neither fd is used or closed anywhere else.
        let read_file = unsafe { std::fs::File::from_raw_fd(read_fd) };
        let write_file = unsafe { std::fs::File::from_raw_fd(write_fd) };
        (Box::new(read_file), raw, write_file)
    }

    /// Serializes the two `LIVE_READER_THREADS` cells against EACH OTHER, never against the rest
    /// of the suite. `cargo test` runs cells concurrently by default, and this counter is
    /// process-wide: two of these cells racing would read each other's threads in their own
    /// before/after snapshot. No other cell in this file touches `spawn_reader`, so this pair is
    /// the counter's only possible source of cross-cell noise.
    static READER_COUNT_CELLS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// CONTROL, mandatory before the abandonment cell below means anything: a LIVE capture whose
    /// writer goes silent for well past what a real caller's grace would be, and THEN writes, must
    /// still receive the byte it sent -- the reader's poll loop must never treat "nothing yet" as
    /// "abandoned" on its own. Nothing in `spawn_reader` ever sets `give_up`; only a caller does,
    /// and this cell never calls one.
    #[test]
    fn a_live_reader_survives_silence_and_still_delivers_what_arrives_after_it() {
        let _serialized = READER_COUNT_CELLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = LIVE_READER_THREADS.load(Ordering::Relaxed);
        let (pipe, raw, mut write_end) = synthetic_pipe();
        let handle = super::spawn_reader(pipe, raw, 4096);

        // Well past several `READER_POLL` ticks (20 ms) -- long enough that a reader which gave
        // up on silence alone would already have done so.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            handle
                .receiver
                .recv_timeout(Duration::from_millis(10))
                .is_err(),
            "the reader answered before EOF or abandonment -- it must not have"
        );
        assert!(
            !handle.give_up.load(Ordering::Relaxed),
            "nothing signalled abandonment, so the flag must still read false"
        );

        use std::io::Write as _;
        write_end
            .write_all(b"still here")
            .expect("the write end is open");
        drop(write_end); // the only writer; dropping it is what makes EOF happen

        let (bytes, truncated) = handle
            .receiver
            .recv_timeout(Duration::from_secs(2))
            .expect("a live capture must answer once its one writer closes");
        assert_eq!(bytes, b"still here");
        assert!(!truncated);

        // The thread answered via ordinary EOF, not via the abandonment path -- give it a moment
        // to actually return from the closure (the guard's `Drop` runs after `sender.send`) and
        // confirm it left no count behind.
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            LIVE_READER_THREADS.load(Ordering::Relaxed),
            before,
            "a normally-ending reader must not leave its count incremented"
        );
    }

    /// THE CELL #726 EXISTS FOR. A pipe whose write end the test keeps open throughout -- exactly
    /// what an escaped descendant's inherited handle looks like from the reader's side, with no
    /// process, escape or job object needed to produce it. Before the fix this hangs (the reader
    /// blocks in `read` forever); confirmed red-first by hand: commenting out the
    /// `signal.load(...)` check in `spawn_reader`'s poll loop makes this cell's `recv_timeout`
    /// below time out and the assertion fail, rather than the whole run hanging -- the bounded
    /// wait is deliberate so a regression here is a red, never a stuck gate.
    ///
    /// Run N times: the closing criterion is that abandoning N captures leaves the reader-thread
    /// count where it started, not growing by N.
    #[test]
    fn an_abandoned_reader_ends_within_the_grace_and_leaves_no_thread_behind() {
        let _serialized = READER_COUNT_CELLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        const REPETITIONS: usize = 5;
        let before = LIVE_READER_THREADS.load(Ordering::Relaxed);
        // Held for the whole test: nothing ever closes it, which is the one property an escaped
        // descendant's inherited handle has that this fixture must reproduce.
        let mut writers_never_closed = Vec::new();

        for _ in 0..REPETITIONS {
            let (pipe, raw, write_end) = synthetic_pipe();
            writers_never_closed.push(write_end);
            let handle = super::spawn_reader(pipe, raw, 4096);

            assert!(
                handle
                    .receiver
                    .recv_timeout(Duration::from_millis(50))
                    .is_err(),
                "nothing was written and the pipe is open -- there is no answer yet"
            );

            // The one and only mechanism that ends this reader (#726): the drain has already
            // exhausted its own silence grace, hard deadline, release and post-release grace,
            // and only THEN says so.
            handle.give_up.store(true, Ordering::Relaxed);

            let (bytes, truncated) = handle.receiver.recv_timeout(Duration::from_secs(2)).expect(
                "an abandoned reader must end within a small multiple of READER_POLL, not \
                     leak for the life of the process",
            );
            assert_eq!(bytes, Vec::<u8>::new(), "nothing was ever written to it");
            assert!(!truncated);
        }

        // Give the last thread's guard a moment to run past `sender.send` before reading the
        // count.
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            LIVE_READER_THREADS.load(Ordering::Relaxed),
            before,
            "{REPETITIONS} abandoned captures must not leave {REPETITIONS} threads behind"
        );
        // The write ends outlive every reader in this test, on purpose -- dropping them here,
        // after every assertion, so a premature EOF (from an early drop) can never be mistaken
        // for the abandonment path actually being exercised.
        drop(writers_never_closed);
    }
}
