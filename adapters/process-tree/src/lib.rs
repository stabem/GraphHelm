//! Killing a process AND everything it spawned, on both platforms.
//!
//! An adapter to the operating system's process API, which is why it lives beside the other
//! adapters rather than in `core/`: every `core/` crate is domain, and putting `libc` and
//! `windows-sys` there would point the domain layer at the OS.
//!
//! # Why this is a crate rather than a copy
//!
//! It was not new code. `adapters/postgres-event-store` has carried the whole thing since the
//! evidence-store milestone — Unix process groups, Windows kill-on-close job objects, and a
//! liveness check — private to `backup.rs` and serving one caller. Meanwhile
//! `adapters/tool-host`'s spawn funnel, the one **every** Tier 1 execution passes through, kills
//! the direct child only (#612, #618).
//!
//! So the capability existed, was documented as delivered, and the place that most needed it did
//! not call it. A second implementation is how two implementations drift, and platform code is the
//! worst kind to have two of. Extracted rather than copied, and the move preserves behaviour: the
//! bodies below are the event store's, unchanged except for the error type.
//!
//! # What it does NOT do
//!
//! It does not carry `ProcessWatchdog`. That type is a child plus a deadline plus a cancellation
//! flag, and `run_in_workspace` already owns an equivalent loop with different semantics — a
//! refusal when the signal is already raised, per-stream output caps, a disposition to record.
//! Moving it would be extraction with no second consumer, which is generality bought on
//! speculation.
//!
//! # The two platforms are NOT equally strong, and this crate used to imply they were (#717)
//!
//! Everything below is written as one guarantee with two spellings: [`configure`], [`create`],
//! [`terminate`], [`close`]. The containment underneath them is not symmetric, and a caller reading
//! the signatures would reasonably assume it is.
//!
//! | | Unix | Windows |
//! |---|---|---|
//! | the container | a process group | a job object |
//! | can a descendant leave it? | **YES** — one `setsid` or `setpgid` call | **no** — breakaway needs `CREATE_BREAKAWAY_FROM_JOB` *and* a job that permits it, and this job does not set `JOB_OBJECT_LIMIT_BREAKAWAY_OK` |
//! | what `close` does | nothing; there is no handle to release | kills the job's remaining MEMBERS (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) |
//!
//! **One caveat on the Windows column, because the table would otherwise overstate it** (Codex, on
//! #746). "Cannot leave" is about breakaway, and there is a second way to be outside a job: never
//! having joined it. Nothing in this API forces [`configure`] to run before the spawn, and a child
//! spawned without it runs immediately — so anything IT starts before [`create`] assigns the job is
//! outside the job, and neither [`terminate`] nor [`close`] reaches it. That is why the suspension in
//! [`configure`] is load-bearing rather than tidiness, and why `adapters/tool-host`'s funnel calls it.
//! The Windows advantage is real and it is conditional on that call.
//!
//! **The Unix hole fails toward a false GREEN, which is the worse direction.** A descendant that
//! leaves the group survives `terminate`, and if it also redirects its streams the reader backstop
//! sees a clean EOF — so the capture looks normal and the record says a tree is gone while it is
//! not. Tracked as #717, and the code fix is platform work (a PID namespace, a cgroup, or a
//! supervisor that reaps by subtree) rather than a line.
//!
//! ## The inequality has produced two defects that look nothing like each other
//!
//! Both found on #703, and they are the argument for declaring it rather than leaving it implicit —
//! a reader cannot rediscover from the API that these have a common cause:
//!
//! 1. **A descendant that escapes the kill** and outlives the call (#717 itself).
//! 2. **A reader thread that never returns** (#726). When a capture gives up on its readers, the
//!    caller still runs [`close`]. On Windows that kills the job's remaining MEMBERS — which, when
//!    [`configure`] ran before the spawn, includes every descendant — so the inherited pipe closes,
//!    and the blocked reader returns microseconds later; on Unix `close` does nothing, so the
//!    descendant lives, keeps the pipe, and the thread blocks forever — two stranded threads and
//!    their buffers per invocation.
//!
//! **A doc gap that generates dissimilar defects is a defect generator, not a formatting task.**
//! Neither of those was findable by reading `terminate`; both were findable by asking what `close`
//! means on each platform, which is a question the API never invited.
//!
//! ## What a caller should therefore assume
//!
//! Treat the guarantee as **"the tree is killed unless a descendant deliberately left the group"**,
//! and on Unix treat a clean capture as evidence about the CHILD rather than about the tree. See
//! also #715: on Unix a killed-but-unreaped descendant still reads as running, so the liveness
//! answer has its own asymmetry one layer down.

/// Why a process group could not be established.
///
/// Deliberately NOT the caller's error type. `create` used to return `BackupError`, so the module
/// could only ever live in the crate that owned that enum; each caller now maps these two into its
/// own vocabulary at the call site, and the mapping is the only place a caller's meaning is
/// restated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessTreeError {
    /// The job object could not be created, configured, or assigned (Windows only).
    JobSetup,
    /// The suspended child could not be resumed after assignment (Windows only).
    ProcessResume,
}

impl std::fmt::Display for ProcessTreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JobSetup => formatter.write_str("the process job object could not be set up"),
            Self::ProcessResume => {
                formatter.write_str("the suspended process could not be resumed")
            }
        }
    }
}

impl std::error::Error for ProcessTreeError {}

/// A process could not be bound to an identity the reap cannot invalidate, so the caller COULD NOT
/// ASK rather than got an answer (#621, #680).
///
/// **Its own type rather than a third variant of [`ProcessTreeError`]**, and the reason is a
/// measurement: widening that enum broke `backup.rs`'s exhaustive match, in an extraction whose
/// contract is that the event store is untouched. Group setup and identity capture are different
/// concerns with different callers, and one of them had a consumer that proved it.
///
/// Never silently downgraded to a bare id: a fallback would let a caller believe it held identity
/// while holding a number, which is the drift this type exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityUnavailable;

impl std::fmt::Display for IdentityUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the process could not be bound to a durable identity")
    }
}

impl std::error::Error for IdentityUnavailable {}

/// A handle to the group a child and its descendants belong to.
///
/// Opaque, and one type name on both platforms so callers need no `cfg` of their own. On Unix the
/// group is the child's own process group and there is nothing to hold; on Windows it is a job
/// object handle, which is why [`close`] exists at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessGroup(GroupHandle);

#[cfg(unix)]
type GroupHandle = ();

#[cfg(windows)]
type GroupHandle = usize;

#[cfg(unix)]
impl ProcessGroup {
    const EMPTY: Self = Self(());
}

#[cfg(windows)]
impl ProcessGroup {
    const EMPTY: Self = Self(0);
}

/// Prepare `command` so the child it spawns can be grouped.
///
/// On Unix this puts the child in its own process group, and on Linux additionally asks the kernel
/// to `SIGKILL` it if this process dies — with a `getppid` check inside `pre_exec` closing the race
/// where the parent dies between fork and the `prctl`.
///
/// **The Unix grouping is escapable and the Windows one is not** (#717): a descendant calling
/// `setsid` or `setpgid` leaves the group, while leaving a job object needs a creation flag the job
/// can refuse. The `PR_SET_PDEATHSIG` above narrows nothing here either — it is set on the DIRECT
/// child and does not extend to what that child spawns.
///
/// On Windows the child starts SUSPENDED, because a job object can only be assigned after the
/// process exists and a child that ran first could spawn descendants outside the job. [`create`]
/// resumes it once the assignment holds.
#[cfg(unix)]
pub fn configure(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    // ONCE, and before any child exists (#748). `PR_SET_CHILD_SUBREAPER` makes an orphaned
    // descendant reparent to THIS process rather than to pid 1, which is what keeps the `/proc`
    // ancestry chain intact for `sweep_subtree` when the middle process dies first. Measured: the
    // call returns 0 with no privileges, and a `setsid` grandchild whose parent was killed came
    // back with our pid as its `ppid` instead of 1.
    //
    // Failure is deliberately ignored rather than propagated: the subreaper narrows a hole in the
    // sweep and its absence degrades the sweep, which `TerminationOutcome` reports. Refusing to
    // spawn because a hardening prctl failed would trade a narrowed hole for a dead product.
    #[cfg(target_os = "linux")]
    {
        static SUBREAPER: std::sync::Once = std::sync::Once::new();
        SUBREAPER.call_once(|| unsafe {
            libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0);
        });
    }
    command.process_group(0);
    #[cfg(target_os = "linux")]
    let expected_parent = unsafe { libc::getpid() };
    #[cfg(target_os = "linux")]
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::getppid() != expected_parent {
                return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
            }
            Ok(())
        });
    }
}

/// Establish the group for an already-spawned `child`.
///
/// # Errors
/// [`ProcessTreeError::JobSetup`] when the job object cannot be created, configured or assigned;
/// [`ProcessTreeError::ProcessResume`] when the assigned child cannot be resumed. Never fails on
/// Unix, where the grouping was settled by [`configure`] before the spawn.
#[cfg(unix)]
#[allow(clippy::missing_errors_doc, clippy::unnecessary_wraps)]
pub fn create(_child: &std::process::Child) -> Result<ProcessGroup, ProcessTreeError> {
    Ok(ProcessGroup::EMPTY)
}

/// The same group, usable from another thread.
#[must_use]
pub fn for_thread(group: ProcessGroup) -> ProcessGroup {
    group
}

/// Release the group's handle. A no-op on Unix, which holds none.
///
/// **The no-op is not the same act as the Windows one, and the difference is load-bearing** (#717).
/// There the job carries `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so closing it KILLS whatever is still
/// inside; here there is nothing to close and nothing dies. A caller that treats `close` as cleanup
/// gets cleanup on one platform and a comment on the other.
///
/// The measured consequence is #726: when a capture abandons its readers, `close` still runs. On
/// Windows the escaped descendant dies, its inherited pipe handle closes, and the blocked reader
/// thread returns; on Unix it lives, keeps the pipe, and that thread blocks forever.
#[cfg(unix)]
pub fn close(_group: &mut ProcessGroup) {}

/// What a [`terminate`] actually achieved, because "it returned" is not the same as "the tree is
/// gone" (#748).
///
/// A sweep that stops after N passes and reports nothing has bounded its ITERATIONS and not the
/// property it exists to provide — #796's class, and the reason this is a value rather than a
/// silent `()`. A caller that cannot tell `Complete` from `BoundReached` will report a tree as
/// stopped while something it spawned is still running.
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a sweep that hit its bound left descendants running; dropping this says the tree is gone when it may not be"]
pub enum TerminationOutcome {
    /// Signalled everything reachable, and a final pass found nothing new.
    Complete,
    /// The sweep hit its pass limit while descendants were still appearing. `remaining` is what
    /// the last pass saw; there may be more.
    BoundReached { passes: u32, remaining: usize },
    /// The group signal was sent and the SUBTREE sweep did not run, so a descendant that left the
    /// group survives. This is what the crate says on a Unix without `/proc` or without
    /// `PR_SET_CHILD_SUBREAPER` rather than claiming a property it cannot deliver — the same
    /// posture [`ProcessIdentity::capture`] already takes.
    ///
    /// THE SIGNAL WAS SENT. Every path that returns this has already called `kill(-pgid)`, and that
    /// sentence is what a consumer is entitled to read off it -- see [`Self::NotAttempted`] for the
    /// case where nothing was sent at all, which this variant used to carry too (#815).
    SweepUnavailable,
    /// NOTHING WAS SIGNALLED. The call returned before `kill`, so the leader and every descendant
    /// are untouched -- not "the sweep could not run", but "the termination never started".
    ///
    /// Split out of [`Self::SweepUnavailable`] (#815), which was returned by two paths meaning
    /// opposite things: the platform limit above, and the pid conversion below it that returns
    /// before the group signal. A consumer reading the documented meaning -- signal sent, sweep
    /// skipped -- would classify a call that terminated NOTHING as a success, and one did.
    ///
    /// The producing path is effectively unreachable today: `i32::try_from(u32)` fails only above
    /// 2_147_483_647 and Linux caps `pid_max` at 4_194_304. That is the argument for fixing it
    /// cheaply rather than urgently, and it is also exactly why an overloaded variant survives every
    /// review it passes through: nothing will ever produce a red here. The defect was in what the
    /// TYPE permitted a consumer to conclude.
    NotAttempted,
}

/// The negative pid `kill` needs, or the outcome to return instead.
///
/// EXTRACTED SO IT CAN BE TESTED WHERE THE TESTS RUN. Both producers of the overloaded variant are
/// `#[cfg(unix)]`, so no cell on a Windows host could reach either -- #815 said as much and expected
/// a Unix-gated cell or an argument at the construction site. A pure conversion has no platform in
/// it, so the decision that used to hide inside a `let ... else` is a value this crate's own suite
/// pins on any host, and the unix arm below simply asks it.
///
/// It returns the OUTCOME rather than a bool, because the caller's only honest answer for a pid it
/// cannot signal is to say what it did: nothing.
/// DEAD ON NON-UNIX, DELIBERATELY, AND THE ALLOW SAYS WHICH PLATFORM. The only production caller is
/// the `#[cfg(unix)]` `terminate` below; the function itself carries no platform so that the cell can
/// run on this host, which is the whole reason it exists as a function at all. Scoping the allow to
/// `not(unix)` keeps it dead-code-checked where it does have a caller -- a bare `allow(dead_code)`
/// would hide the day the unix arm stops calling it.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn group_signal_target(process_id: u32) -> Result<i32, TerminationOutcome> {
    match i32::try_from(process_id) {
        Ok(signed) => Ok(signed),
        Err(_) => Err(TerminationOutcome::NotAttempted),
    }
}

/// Kill the process and everything it spawned **that is still in its process group** (#717).
///
/// The qualifier is the whole of the Unix/Windows difference. `kill(-pgid)` reaches the group, and a
/// descendant that called `setsid` or `setpgid` is no longer in it — one syscall, no privileges
/// required, and nothing here can observe that it happened. The Windows body has no equivalent hole:
/// a job object holds everything its members create unless the job itself permits breakaway, and
/// this one does not.
///
/// **It fails toward a false GREEN.** The escapee survives; if it also redirected its streams the
/// readers see a clean EOF, and the record then says the tree is gone while it is not. That is the
/// dangerous direction, and it is why the gap is declared here rather than left for a reader to
/// infer from the absence of a comment.
#[cfg(unix)]
pub fn terminate(process_id: u32, _group: ProcessGroup) -> TerminationOutcome {
    // #815: NOT `SweepUnavailable`. This returns before `subtree_snapshot` and before `kill`, so
    // nothing is signalled and the tree is untouched -- the opposite of what that variant's own
    // documentation promises a reader.
    let signed = match group_signal_target(process_id) {
        Ok(signed) => signed,
        Err(outcome) => return outcome,
    };
    // The group signal FIRST, unchanged: it reaches everything that did not escape, in one
    // syscall, and the sweep below then has almost nothing to find on the ordinary path.
    //
    // NEGATIVE pid: the signal goes to the whole process group, which is the difference between
    // this and `Child::kill`.
    // THE SNAPSHOT COMES FIRST, AND THE ORDER IS THE WHOLE FIX (measured, #748).
    //
    // The obvious sequence -- signal the group, then walk for escapees -- does not work, and the
    // reason is the subreaper that makes the walk possible at all. The group signal kills the
    // CHILD; the escapee is then reparented to US; and its ancestry no longer reaches the child's
    // pid, so a walk rooted there finds nothing. The kill that precedes the sweep is what breaks
    // the chain the sweep needs.
    //
    // Measured: with the sweep after the signal, the escaping-grandchild cell still failed at 30s
    // with the descendant alive. Snapshotting before the signal makes it pass.
    let condemned = subtree_snapshot(process_id);
    unsafe {
        libc::kill(-signed, libc::SIGKILL);
    }
    sweep_subtree(process_id, condemned)
}

/// Kill what the group signal could not reach: descendants that left it (#748).
///
/// **Why this is possible at all, measured rather than assumed.** `setsid` changes a process's
/// session and group and leaves its PARENT link untouched, so `/proc` ancestry still reaches the
/// escapee. Measured on Linux 6.6: a grandchild that called `setsid` had `sid` and `pgid` of its
/// own and `ppid` still naming its parent.
///
/// **The walk's one hole is closed by the subreaper.** If the middle process dies first the
/// escapee is reparented, classically to pid 1, where nothing distinguishes it from any other
/// stray. [`configure`] sets `PR_SET_CHILD_SUBREAPER` so orphaned descendants reparent to US
/// instead — measured: `prctl` returns 0 with no privileges, and after killing the middle process
/// the grandchild's `ppid` became this process rather than 1.
///
/// **The kill is by IDENTITY, not by number.** Between reading a pid's ancestry and signalling it,
/// that pid can exit and be recycled onto an unrelated process — and this runs with whatever reach
/// the runner has. Every candidate is re-read immediately before the signal and its start time
/// compared: same pid with a different start time is a DIFFERENT process, and is left alone. That
/// is #624's slot-liveness comparison, pointed at a subtree.
#[cfg(all(unix, target_os = "linux"))]
fn sweep_subtree(root: u32, condemned: Vec<(u32, u64)>) -> TerminationOutcome {
    /// Deliberately small. Each pass signals everything it found, so a tree that is merely deep
    /// converges in a pass or two; only a process spawning faster than the sweep reaches this,
    /// and for that the honest answer is `BoundReached` rather than a bigger number.
    const MAX_PASSES: u32 = 8;

    let mut passes = 0;
    let mut condemned = condemned;
    loop {
        // The snapshot taken before the signal, plus anything that appeared since and is STILL
        // traceable to the root -- a late spawn whose parent had not yet died. Both are needed:
        // the snapshot survives the reparenting, and the walk catches what the snapshot missed.
        let mut descendants = descendants_of(root);
        descendants.append(&mut condemned);
        descendants.sort_unstable();
        descendants.dedup();
        descendants.retain(|(pid, started_at)| process_start_time(*pid) == Some(*started_at));
        if descendants.is_empty() {
            return TerminationOutcome::Complete;
        }
        if passes >= MAX_PASSES {
            return TerminationOutcome::BoundReached {
                passes,
                remaining: descendants.len(),
            };
        }
        for (pid, started_at) in &descendants {
            // Re-read at the moment of the signal. A candidate that exited between the walk and
            // here is either gone or has been replaced by a stranger, and a stranger must not be
            // killed because it inherited a number.
            if process_start_time(*pid) != Some(*started_at) {
                continue;
            }
            if let Ok(signed) = i32::try_from(*pid) {
                unsafe {
                    libc::kill(signed, libc::SIGKILL);
                }
            }
        }
        passes += 1;
    }
}

/// The sweep is Linux-shaped: it needs `/proc` ancestry and `PR_SET_CHILD_SUBREAPER`, and neither
/// exists on macOS or the BSDs. Saying so is the point — a fallback that walked something weaker
/// would report `Complete` on a platform where the escape still works.
#[cfg(all(unix, not(target_os = "linux")))]
fn sweep_subtree(_root: u32, _condemned: Vec<(u32, u64)>) -> TerminationOutcome {
    TerminationOutcome::SweepUnavailable
}

/// The subtree as it stands RIGHT NOW, captured before anything is signalled.
///
/// Separate from [`descendants_of`] only in name: the distinction it carries is WHEN it is called,
/// and that is the load-bearing part of the fix.
#[cfg(all(unix, target_os = "linux"))]
fn subtree_snapshot(root: u32) -> Vec<(u32, u64)> {
    descendants_of(root)
}

/// The snapshot is not available where the sweep is not.
#[cfg(all(unix, not(target_os = "linux")))]
fn subtree_snapshot(_root: u32) -> Vec<(u32, u64)> {
    Vec::new()
}

/// `(pid, start time)` for every live process whose ancestry reaches `root`, `root` excluded.
#[cfg(all(unix, target_os = "linux"))]
fn descendants_of(root: u32) -> Vec<(u32, u64)> {
    let mut parents: std::collections::BTreeMap<u32, (u32, u64)> =
        std::collections::BTreeMap::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if let Some(record) = read_stat(pid) {
            parents.insert(pid, (record.parent, record.started_at));
        }
    }

    let mut found = Vec::new();
    for (&pid, &(_, started_at)) in &parents {
        if pid == root {
            continue;
        }
        // Walk up, bounded by the map's own size: a cycle cannot exist in a real process table,
        // but this reads one, and a loop here would hang the kill path.
        let mut cursor = pid;
        for _ in 0..parents.len().saturating_add(1) {
            let Some(&(parent, _)) = parents.get(&cursor) else {
                break;
            };
            if parent == root {
                found.push((pid, started_at));
                break;
            }
            if parent <= 1 {
                break;
            }
            cursor = parent;
        }
    }
    found
}

/// `(ppid, start time)` from `/proc/<pid>/stat`.
///
/// The fields are read AFTER the last `)`, because a process name can contain spaces and
/// parentheses and splitting the whole line would put the parse at the mercy of whatever a child
/// called itself.
/// What `/proc/<pid>/stat` says: the state letter, the parent, and when it began.
///
/// ONE reader for all three. The sweep wants the parent and the start time;
/// [`process_is_running`] wants the state -- and the state was already being SKIPPED here
/// before anything needed it (#715). Two readers of one file drift; this is the file agreeing
/// with itself.
#[cfg(all(unix, target_os = "linux"))]
struct ProcStat {
    state: char,
    parent: u32,
    started_at: u64,
}

#[cfg(all(unix, target_os = "linux"))]
fn read_stat(pid: u32) -> Option<ProcStat> {
    let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let tail = raw.get(raw.rfind(')')? + 2..)?;
    let mut fields = tail.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let parent = fields.next()?.parse().ok()?;
    // `starttime` is field 22 of the whole line; state and ppid are consumed above, so it sits
    // at offset 17 from here.
    let started_at = fields.nth(17)?.parse().ok()?;
    Some(ProcStat {
        state,
        parent,
        started_at,
    })
}

#[cfg(all(unix, target_os = "linux"))]
fn process_start_time(pid: u32) -> Option<u64> {
    read_stat(pid).map(|stat| stat.started_at)
}

/// Start the child suspended, so that `create` can put it in a job before it runs.
///
/// **The suspension is load-bearing, not a tidiness flag.** A job can only be assigned to a process
/// that already exists, so there is necessarily a window between spawn and
/// `AssignProcessToJobObject`. A child that runs during that window can spawn descendants of its
/// own, and those descendants are **outside** the job: `terminate` will not reach them, which is
/// the entire property this crate exists to provide. `CREATE_SUSPENDED` closes the window, and
/// `create` reopens it deliberately by resuming the process only after the assignment succeeds.
///
/// Removing this flag looks harmless — the child still starts, the tests that count processes still
/// pass — and leaves the job with silent holes. (Mechanism named by D while working #618, which
/// consumes this crate from the tool host.)
#[cfg(windows)]
pub fn configure(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
    command.creation_flags(CREATE_SUSPENDED);
}

/// Put the suspended child in a kill-on-close job object, then resume it.
///
/// # Handle ownership, which is only visible by tracing control flow
///
/// Two handles are opened here and they are owned differently. Neither is an RAII type, so the
/// discipline lives in the exits rather than in a `Drop`.
///
/// **`job` — owned by this function until, and only until, the `Ok`.** There are four ways out
/// after `CreateJobObjectW` is called:
///
/// | exit | `job` |
/// |---|---|
/// | the create returned null | never existed; nothing to close |
/// | `SetInformationJobObject` or `AssignProcessToJobObject` failed | closed here |
/// | `resume_suspended_process` failed | closed here |
/// | success | **NOT closed** — it is handed to the caller inside `ProcessGroup` |
///
/// **The handing over is not RAII, and the difference is the caller's problem.** `ProcessGroup` is
/// `Copy` and has no `Drop`: dropping one closes nothing, and copying one does not track anything.
/// The handle is released only when the caller calls [`close`], which is why that function exists.
/// A caller that drops the value and moves on leaks the job handle — and because of the flag below,
/// leaking it means the process tree it was meant to bound **stays alive**, which is the opposite
/// failure from the one this crate is for.
///
/// So the success path is the one that looks like a leak and is not: closing `job` there would
/// *kill the process tree immediately*, at the exact moment the caller was told the group was
/// ready, and leave the caller's later [`close`] closing a handle that is no longer theirs.
///
/// **`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the handle's lifetime the kill policy.** The job
/// dies when its last handle closes, and everything in it dies with it. So `job` is not
/// bookkeeping that can be tidied up: holding it open IS the process group, and closing it IS
/// `terminate`. A future refactor that adds an early `CloseHandle(job)` "for symmetry" would be
/// killing the tree, not freeing a resource.
///
/// **`process` — a second handle this function OWNS, closed as soon as the assignment is done.**
/// `OpenProcess` creates a new handle; assigning the process to the job gives the *job* its own
/// reference to the process object, and that does **not** demote this one to a borrow. The
/// `CloseHandle` here is therefore mandatory rather than tidy: without it every group creation
/// leaks one process handle. It is closed immediately because nothing after the assignment needs
/// it — the job holds the process, not us.
///
/// Its close sits **above** every error branch, so by the time any exit is reached it has already
/// been released; and when `OpenProcess` returns null the `&&` short-circuits, so `assigned` is
/// false and the assignment is never attempted.
///
/// The bound this crate provides stops at the job. A descendant that escaped before assignment is
/// not in it — see `configure` for why the child is started suspended.
#[cfg(windows)]
#[allow(clippy::missing_errors_doc)]
pub fn create(child: &std::process::Child) -> Result<ProcessGroup, ProcessTreeError> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject,
            },
            Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE},
        },
    };
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(ProcessTreeError::JobSetup);
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            u32::try_from(std::mem::size_of_val(&limits)).unwrap(),
        )
    } != 0;
    let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, child.id()) };
    let assigned = !process.is_null() && unsafe { AssignProcessToJobObject(job, process) } != 0;
    if !process.is_null() {
        unsafe { CloseHandle(process) };
    }
    if !configured || !assigned {
        unsafe { CloseHandle(job) };
        return Err(ProcessTreeError::JobSetup);
    }
    if let Err(error) = resume_suspended_process(child.id()) {
        unsafe { CloseHandle(job) };
        return Err(error);
    }
    Ok(ProcessGroup(job as usize))
}

#[cfg(windows)]
fn resume_suspended_process(process_id: u32) -> Result<(), ProcessTreeError> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First,
                Thread32Next,
            },
            Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(ProcessTreeError::ProcessResume);
    }
    let mut entry = THREADENTRY32 {
        dwSize: u32::try_from(std::mem::size_of::<THREADENTRY32>()).unwrap(),
        ..THREADENTRY32::default()
    };
    let mut found = unsafe { Thread32First(snapshot, std::ptr::addr_of_mut!(entry)) } != 0;
    let mut resumed = false;
    while found {
        if entry.th32OwnerProcessID == process_id {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                resumed = unsafe { ResumeThread(thread) } != u32::MAX;
                unsafe { CloseHandle(thread) };
                if resumed {
                    break;
                }
            }
        }
        found = unsafe { Thread32Next(snapshot, std::ptr::addr_of_mut!(entry)) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    if resumed {
        Ok(())
    } else {
        Err(ProcessTreeError::ProcessResume)
    }
}

/// Release the job handle — **which KILLS the job's remaining members**.
///
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the handle's lifetime the kill policy, so this is not
/// merely cleanup: it is a second kill, and callers order it after their readers for that reason.
///
/// **The Unix counterpart does none of that** (#717). There a process group has no handle to
/// release and `close` is a no-op, so a descendant that escaped `terminate` survives this call
/// instead of dying to it. Written on BOTH bodies deliberately: each is invisible in the other's
/// rendered documentation, and the reader who most needs to know the two differ is the one reading
/// only the platform they are on.
///
/// The measured consequence is #726: when a capture abandons its readers, `close` still runs. Here
/// the job's remaining members die, their inherited pipe handles close, and the blocked reader thread
/// returns within microseconds. On Unix that thread blocks forever.
///
/// **"Members", not "everything that ran" — the distinction is real and this doc overstated it once**
/// (Codex, on #746). Nothing in this API forces [`configure`] to be called before the spawn, and a
/// child spawned without it runs immediately: anything IT starts before [`create`] assigns the job is
/// outside the job, and `KILL_ON_JOB_CLOSE` never touches it. Such a process can keep an inherited
/// pipe end and its reader will not return here either. The funnel in `adapters/tool-host` does call
/// [`configure`], which is what closes that window for the case #726 measured.
#[cfg(windows)]
pub fn close(group: &mut ProcessGroup) {
    if group.0 != 0 {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(group.0 as _) };
        *group = ProcessGroup::EMPTY;
    }
}

/// Kill the process and everything it spawned.
///
/// **No qualifier is needed on this platform, and that is the asymmetry** (#717). A job object holds
/// every process its members create; leaving one requires `CREATE_BREAKAWAY_FROM_JOB` *and* a job
/// that permits breakaway, and this job does not set `JOB_OBJECT_LIMIT_BREAKAWAY_OK`.
///
/// **The Unix counterpart is escapable**: it sends `SIGKILL` to a process group, and one `setsid` or
/// `setpgid` call takes a descendant out of it — no privileges, and nothing observable. Its
/// guarantee is therefore "everything it spawned THAT IS STILL IN THE GROUP", and it fails toward a
/// false GREEN. Said here as well as there because neither body appears in the other's rendered
/// documentation.
///
/// Always [`TerminationOutcome::Complete`], and the signature exists so the two platforms answer
/// the same QUESTION rather than so this one has something to say. A job object holds every
/// process its members create, so there is no subtree left over to sweep and no bound to hit --
/// the outcome the Unix arm has to work for is what this one gets from the kernel.
#[cfg(windows)]
pub fn terminate(process_id: u32, group: ProcessGroup) -> TerminationOutcome {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::TerminateJobObject,
            Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
        },
    };
    if group.0 != 0 {
        unsafe { TerminateJobObject(group.0 as _, 1) };
        return TerminationOutcome::Complete;
    }
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, process_id) };
    if !handle.is_null() {
        unsafe {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
    TerminationOutcome::Complete
}

/// `SYNCHRONIZE` (`0x0010_0000`): the access right that permits WAITING on a handle.
///
/// Declared here rather than imported: `windows-sys` exposes it only under
/// `Win32_Storage_FileSystem`, as a FILE access right, and pulling that feature in for one constant
/// would widen this crate's surface for no other reason. The value is the documented Win32 one.
#[cfg(windows)]
const SYNCHRONIZE: u32 = 0x0010_0000;

/// `WAIT_OBJECT_0`: the handle is SIGNALED, which for a process handle means it has exited.
const WAIT_SIGNALED: u32 = 0;
/// `WAIT_TIMEOUT`: nothing happened in the interval, so the process is still running.
const WAIT_STILL_RUNNING: u32 = 258;

/// What a zero-timeout wait on a process handle says about liveness, or `None` when the wait failed.
///
/// **This replaces reading the exit code, and the reason is a real ambiguity rather than tidiness**
/// (Codex, on #680). `GetExitCodeProcess` returns 259 both for a process that has NOT exited and for
/// one that exited WITH 259 — so a child legitimately exiting with that value reads as running
/// forever. Guarding `fake_tool` against 259 protected that fixture and nothing else: this crate
/// binds arbitrary executables, and the operator picks the tests runner.
///
/// A handle's signaled state has no such overlap. No exit code can imitate it.
#[must_use]
fn liveness_from_wait(waited: u32) -> Option<bool> {
    match waited {
        WAIT_SIGNALED => Some(false),
        WAIT_STILL_RUNNING => Some(true),
        _ => None,
    }
}

/// A `WaitForSingleObject` timeout that can never be `INFINITE`.
///
/// `0xFFFFFFFF` is not "the longest wait", it is **no bound at all** -- so a saturating conversion
/// turns a very large patience into a wait that never returns (Codex, on #703). That is the third
/// colour this change exists to remove: neither pass nor fail, and no `cargo test` timeout to catch
/// it. A caller asking for an absurd bound gets the longest FINITE one instead, because being one
/// millisecond short of a 49-day wait cannot matter to anyone, and hanging forever can.
#[cfg_attr(not(windows), allow(dead_code))]
#[must_use]
fn windows_wait_milliseconds(patience: std::time::Duration) -> u32 {
    const INFINITE: u32 = u32::MAX;
    u32::try_from(patience.as_millis())
        .unwrap_or(INFINITE)
        .min(INFINITE - 1)
}

/// The decision the Unix query feeds.
///
/// **`EPERM` means the process EXISTS**: the signal was refused rather than undelivered, which is
/// only possible against something that is running. `sent` alone would answer NOT-RUNNING for a
/// process owned by another user — the same false-dead direction as the Windows case, on the
/// platform that comment did not name.
#[cfg_attr(not(unix), allow(dead_code))]
#[must_use]
fn decide_liveness_from_signal(sent: bool, permission_denied: bool) -> bool {
    sent || permission_denied
}

/// Whether a process id belongs to something still running.
///
/// **Public, and it was the second-most duplicated thing in this move.** It lived in `backup.rs`
/// behind `cfg(test)`, and `adapters/tool-host` grew an identical pair while #621 was being written
/// -- the author had searched for the precedent of the FIX and not for the precedent of the
/// INSTRUMENT. One helper, every suite.
///
/// **An id is not a process.** Once a child is reaped its id may be recycled, so this answers
/// "something with this id is running", not "that child is running". A recycle reads as ALIVE, so a
/// guard built on it fails toward red rather than toward green -- which is the only reason it is
/// usable as an oracle at all. A caller that needs process IDENTITY rather than id liveness has to
/// hold something the reap cannot invalidate; that is not this function.
#[cfg(unix)]
#[must_use]
pub fn process_is_running(process_id: u32) -> bool {
    // An id that does not fit a `pid_t` is not a query that failed, it is a value no Unix process
    // can have -- so it reads as absent, and the invariant above is about queries rather than about
    // malformed input. Said here rather than left as an unstated exception.
    let Ok(process_id) = i32::try_from(process_id) else {
        return false;
    };
    let sent = unsafe { libc::kill(process_id, 0) } == 0;
    let permission_denied =
        !sent && std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
    if !decide_liveness_from_signal(sent, permission_denied) {
        return false;
    }
    // A PID TABLE ENTRY IS NOT A RUNNING PROCESS (#715). `kill(pid, 0)` succeeds against a
    // ZOMBIE -- exited, and not yet reaped by its parent -- so a correctly killed descendant
    // reads as ALIVE until its adopter gets round to it. Measured on Linux 6.6: a child that
    // exited unreaped has state `Z` and `kill(pid, 0)` returns 0; after `waitpid` the same call
    // fails with ESRCH.
    //
    // THE DIRECTION IS WHY THIS IS WORTH FIXING AND WHY IT WAS NOT A BLOCKER. The Windows twin
    // (#680) read a dead process as alive and a CALLER PASSED -- a false green. This reads a
    // dead process as alive and a cell asserting the tree is gone FAILS -- a false red, on a
    // host whose pid 1 reaps slowly, containers especially. Noisy fails safe; silent does not,
    // and nobody should later "fix" this by loosening the check.
    //
    // ONLY the zombie state is subtracted. `T` (stopped) and `D` (uninterruptible sleep) are
    // processes that exist and will run again; calling them dead would invent the false green
    // this exists to avoid.
    #[cfg(target_os = "linux")]
    {
        let Ok(pid) = u32::try_from(process_id) else {
            return true;
        };
        if read_stat(pid).is_some_and(|stat| stat.state == 'Z') {
            return false;
        }
    }
    true
}

/// Whether a process id belongs to something still running.
///
/// **Public, and it was the second-most duplicated thing in this move.** It lived in `backup.rs`
/// behind `cfg(test)`, and `adapters/tool-host` grew an identical pair while #621 was being written
/// -- the author had searched for the precedent of the FIX and not for the precedent of the
/// INSTRUMENT. One helper, every suite.
///
/// **An id is not a process.** Once a child is reaped its id may be recycled, so this answers
/// "something with this id is running", not "that child is running". A recycle reads as ALIVE, so a
/// guard built on it fails toward red rather than toward green -- which is the only reason it is
/// usable as an oracle at all. A caller that needs process IDENTITY rather than id liveness has to
/// hold something the reap cannot invalidate; that is not this function.
#[cfg(windows)]
#[must_use]
pub fn process_is_running(process_id: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, WaitForSingleObject},
    };
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, process_id) };
    if handle.is_null() {
        // No handle is the ONE undecidable case that reads as absent, and it earns that: the id is
        // gone, or it belongs to something this process may not even ask about -- and on Windows a
        // reaped id stops being openable. Distinguishing further would need a privilege this helper
        // must not require.
        return false;
    }
    let waited = unsafe { WaitForSingleObject(handle, 0) };
    unsafe { CloseHandle(handle) };
    // A wait that could not decide reads as RUNNING -- the direction rule, unchanged from the
    // exit-code form it replaces. Wrong toward "still there" costs a red; wrong toward "gone" would
    // let a caller pass on a failure to observe.
    liveness_from_wait(waited).unwrap_or(true)
}

/// A handle on a process that the reap cannot invalidate.
///
/// **Why an id is not enough**, and why the answer is not "the window is small": once a child is
/// reaped its id may be reassigned, so a check against a bare id can observe an unrelated process
/// and read it as the child still running. That is a FALSE RED, and a false red on an authoritative
/// gate is still a nondeterministic gate — being wrong in the comfortable direction is a mitigation,
/// not a property (Codex, on #680).
///
/// What removes the nondeterminism is holding something the OS binds to the process rather than to
/// the number:
///
/// - **Windows** — an open handle RESERVES the id: the system will not reassign it while any handle
///   to that process is held. So the id cannot come to mean something else while this value lives.
/// - **Linux** — a `pidfd` refers to the process itself. It reports the original's exit and never a
///   successor's.
///
/// # Refusal rather than fallback
///
/// Below either floor — a kernel without `pidfd_open`, a Windows process this one may not open, a
/// Unix that is not Linux — [`ProcessIdentity::capture`] REFUSES. It does not fall back to the bare
/// id, because a fallback is the `unwrap_or` that turns a known gap into silent drift: the caller
/// would go on believing it held identity while holding a number. A caller that cannot ask must be
/// told it cannot ask.
#[derive(Debug)]
pub struct ProcessIdentity {
    process_id: u32,
    #[cfg(any(windows, target_os = "linux"))]
    handle: OwnedIdentity,
}

impl ProcessIdentity {
    /// The id this identity is bound to. Useful in diagnostics; never as the thing to check.
    #[must_use]
    pub fn process_id(&self) -> u32 {
        self.process_id
    }
}

#[cfg(windows)]
#[derive(Debug)]
struct OwnedIdentity(isize);

#[cfg(windows)]
impl Drop for OwnedIdentity {
    fn drop(&mut self) {
        // The reservation ends here: after this the id may be reassigned, which is exactly why the
        // value has to outlive the question a caller asks with it.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.0 as _) };
    }
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct OwnedIdentity(i32);

#[cfg(target_os = "linux")]
impl Drop for OwnedIdentity {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

#[cfg(windows)]
impl ProcessIdentity {
    /// # Errors
    /// [`IdentityUnavailable`] when the process cannot be opened — it is already
    /// gone, or this process lacks the right to ask about it.
    pub fn capture(process_id: u32) -> Result<Self, IdentityUnavailable> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        // SYNCHRONIZE is what lets the handle be WAITED on; QUERY_LIMITED_INFORMATION stays because
        // holding it is also what reserves the id.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE,
                0,
                process_id,
            )
        };
        if handle.is_null() {
            return Err(IdentityUnavailable);
        }
        Ok(Self {
            process_id,
            handle: OwnedIdentity(handle as isize),
        })
    }

    /// Whether the process this identity names is still running.
    ///
    /// Exact, because the handle held since [`capture`](Self::capture) has kept the id from being
    /// reassigned: a `true` here is the original process and never a successor.
    ///
    /// # Errors
    /// [`IdentityUnavailable`] when the query itself fails — the caller could not
    /// ask, which is not the same as an answer and must not be recorded as one.
    pub fn is_running(&self) -> Result<bool, IdentityUnavailable> {
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let waited = unsafe { WaitForSingleObject(self.handle.0 as _, 0) };
        liveness_from_wait(waited).ok_or(IdentityUnavailable)
    }

    /// Block until the process this identity names is gone, or until `patience` elapses.
    ///
    /// **The point is that the OS reports the exit, rather than a caller sampling for it.** A poll
    /// loop turns "did it die" into an elapsed-time question with a granularity attached, and on a
    /// loaded host the sample can miss an exit that happened between two of them. Here the wait
    /// returns the instant the process goes, so the bound is only ever reached when it genuinely
    /// did not (Codex, on #703).
    ///
    /// `patience` still exists, and deliberately: an unbounded wait turns a surviving process into a
    /// suite that never returns, which is a third colour rather than a failure. Reaching the bound
    /// is evidence, not a timeout to be retried.
    ///
    /// Returns whether the process is GONE.
    ///
    /// # Errors
    /// [`IdentityUnavailable`] when the wait itself fails -- the caller could not ask, which is not
    /// an answer and must not be recorded as one.
    pub fn wait_until_gone(
        &self,
        patience: std::time::Duration,
    ) -> Result<bool, IdentityUnavailable> {
        use windows_sys::Win32::System::Threading::WaitForSingleObject;
        let waited =
            unsafe { WaitForSingleObject(self.handle.0 as _, windows_wait_milliseconds(patience)) };
        liveness_from_wait(waited)
            .map(|running| !running)
            .ok_or(IdentityUnavailable)
    }

    /// Kill exactly the process this identity names -- never a tree, and never a successor.
    ///
    /// Opening by id is safe HERE and nowhere else: `self` holds a handle, and while it lives the
    /// id cannot be reassigned, so the id this reopens is the same process it was captured from.
    /// The capture rights stay `QUERY_LIMITED_INFORMATION | SYNCHRONIZE` -- widening them for every
    /// caller in order to serve this one would hand out a termination right nobody asked for.
    ///
    /// # Errors
    /// [`IdentityUnavailable`] when the process cannot be opened for termination or the kill is
    /// refused. A process that has already exited reports as unavailable, which a caller that
    /// checked [`is_running`](Self::is_running) first will not see.
    pub fn terminate(&self) -> Result<(), IdentityUnavailable> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_TERMINATE, TerminateProcess,
        };
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, self.process_id) };
        if handle.is_null() {
            return Err(IdentityUnavailable);
        }
        let killed = unsafe { TerminateProcess(handle, 1) };
        unsafe { CloseHandle(handle) };
        if killed == 0 {
            return Err(IdentityUnavailable);
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl ProcessIdentity {
    /// # Errors
    /// [`IdentityUnavailable`] when `pidfd_open` is unavailable (kernels before
    /// 5.3 answer `ENOSYS`) or refuses.
    pub fn capture(process_id: u32) -> Result<Self, IdentityUnavailable> {
        let descriptor = unsafe {
            libc::syscall(
                libc::SYS_pidfd_open,
                libc::pid_t::try_from(process_id).map_err(|_| IdentityUnavailable)?,
                0,
            )
        };
        let descriptor = i32::try_from(descriptor).map_err(|_| IdentityUnavailable)?;
        if descriptor < 0 {
            return Err(IdentityUnavailable);
        }
        Ok(Self {
            process_id,
            handle: OwnedIdentity(descriptor),
        })
    }

    /// # Errors
    /// [`IdentityUnavailable`] when the poll itself fails.
    pub fn is_running(&self) -> Result<bool, IdentityUnavailable> {
        // A pidfd becomes READABLE when its process exits, and it refers to the process rather than
        // to the number -- so this can never answer about a successor.
        let mut watched = libc::pollfd {
            fd: self.handle.0,
            events: libc::POLLIN,
            revents: 0,
        };
        let polled = unsafe { libc::poll(&raw mut watched, 1, 0) };
        if polled < 0 {
            return Err(IdentityUnavailable);
        }
        Ok(watched.revents & libc::POLLIN == 0)
    }

    /// Kill exactly the process this identity names -- never a tree, and never a successor.
    ///
    /// Through the `pidfd`, not through the number. A pidfd refers to the PROCESS, so this cannot
    /// reach a successor; sending to the bare id could, because holding a pidfd does NOT reserve
    /// the id the way a Windows handle does. That asymmetry is the whole reason this method exists
    /// rather than callers running `kill` (Codex, on #703): the Windows side was already safe and
    /// the Linux side was not, and one API hides the difference from every caller.
    ///
    /// # Errors
    /// [`IdentityUnavailable`] when `pidfd_send_signal` is unavailable (kernels before 5.1) or the
    /// signal is refused.

    /// Block until the process this identity names is gone, or until `patience` elapses.
    ///
    /// **The point is that the OS reports the exit, rather than a caller sampling for it.** A poll
    /// loop turns "did it die" into an elapsed-time question with a granularity attached, and on a
    /// loaded host the sample can miss an exit that happened between two of them. Here the wait
    /// returns the instant the process goes, so the bound is only ever reached when it genuinely
    /// did not (Codex, on #703).
    ///
    /// `patience` still exists, and deliberately: an unbounded wait turns a surviving process into a
    /// suite that never returns, which is a third colour rather than a failure. Reaching the bound
    /// is evidence, not a timeout to be retried.
    ///
    /// Returns whether the process is GONE.
    ///
    /// # Errors
    /// [`IdentityUnavailable`] when the wait itself fails -- the caller could not ask, which is not
    /// an answer and must not be recorded as one.
    pub fn wait_until_gone(
        &self,
        patience: std::time::Duration,
    ) -> Result<bool, IdentityUnavailable> {
        let mut watched = libc::pollfd {
            fd: self.handle.0,
            events: libc::POLLIN,
            revents: 0,
        };
        let milliseconds = i32::try_from(patience.as_millis()).unwrap_or(i32::MAX);
        let polled = unsafe { libc::poll(&raw mut watched, 1, milliseconds) };
        if polled < 0 {
            return Err(IdentityUnavailable);
        }
        Ok(watched.revents & libc::POLLIN != 0)
    }

    pub fn terminate(&self) -> Result<(), IdentityUnavailable> {
        let sent = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.handle.0,
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        if sent < 0 {
            return Err(IdentityUnavailable);
        }
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
impl ProcessIdentity {
    /// Always refuses on a Unix that is not Linux.
    ///
    /// `pidfd_open` is Linux's, and there is no portable equivalent — a bare id would be the
    /// fallback, and a fallback here is the drift this type exists to prevent. Declared rather than
    /// approximated: this repository targets Windows and Linux (`AGENTS.md:115`), so the platform
    /// that cannot answer is one nothing runs on.
    ///
    /// # Errors
    /// Always [`IdentityUnavailable`].
    pub fn capture(_process_id: u32) -> Result<Self, IdentityUnavailable> {
        Err(IdentityUnavailable)
    }

    /// # Errors
    /// Always [`IdentityUnavailable`]; no value of this type can be constructed.
    pub fn is_running(&self) -> Result<bool, IdentityUnavailable> {
        Err(IdentityUnavailable)
    }

    /// Always [`IdentityUnavailable`]; no value of this type can be constructed.
    ///
    /// # Errors
    /// Always.
    pub fn terminate(&self) -> Result<(), IdentityUnavailable> {
        Err(IdentityUnavailable)
    }

    /// Always [`IdentityUnavailable`]; no value of this type can be constructed.
    ///
    /// # Errors
    /// Always.
    pub fn wait_until_gone(
        &self,
        _patience: std::time::Duration,
    ) -> Result<bool, IdentityUnavailable> {
        Err(IdentityUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        IdentityUnavailable, ProcessIdentity, TerminationOutcome, WAIT_SIGNALED,
        WAIT_STILL_RUNNING, decide_liveness_from_signal, group_signal_target, liveness_from_wait,
        windows_wait_milliseconds,
    };

    /// #815: the pid a `kill(-pgid)` can take, or the outcome that says nothing was sent.
    ///
    /// THIS CELL RUNS ON EVERY HOST, WHICH IS THE POINT. Both producers of the outcome this splits
    /// are `#[cfg(unix)]`, so the issue expected a Unix-gated cell or an argument at the
    /// construction site -- and a cell that cannot run where the suite runs is a cell nobody sees
    /// go red. Pulling the conversion out of the `let ... else` leaves a pure function with no
    /// platform in it, and the decision becomes a value instead of a control-flow edge.
    ///
    /// The failing input is not reachable in production: `i32::try_from(u32)` fails only above
    /// 2_147_483_647 and Linux caps `pid_max` at 4_194_304. That is exactly why this needs a cell
    /// rather than a live reproduction -- an unreachable path is one nothing else will ever redden.
    #[test]
    fn a_pid_that_cannot_be_signalled_reports_that_nothing_was_attempted() {
        assert_eq!(
            group_signal_target(u32::MAX),
            Err(TerminationOutcome::NotAttempted),
            "a pid too large to signal reported an outcome other than NotAttempted: every path \
             that returns before `kill` must say the tree is untouched, and SweepUnavailable \
             promises the opposite -- that the group signal was sent"
        );
        assert_ne!(
            group_signal_target(u32::MAX),
            Err(TerminationOutcome::SweepUnavailable),
            "the unsignallable pid still reports SweepUnavailable, whose own documentation says \
             the group signal WAS sent -- which is the overload #815 exists to remove"
        );
        // The controls: an ordinary pid converts, and the boundary converts on the legal side.
        assert_eq!(
            group_signal_target(4_194_304),
            Ok(4_194_304),
            "CONTROL: a pid inside Linux's pid_max did not convert, so the assertions above would \
             pass for a function that refuses everything"
        );
        assert_eq!(
            group_signal_target(2_147_483_647),
            Ok(2_147_483_647),
            "CONTROL: the largest pid i32 can hold did not convert, so the refusal above is about \
             the conversion boundary and not about large numbers in general"
        );
    }

    /// The direction is the property, so every outcome is pinned rather than the happy one.
    #[test]
    fn a_wait_that_could_not_decide_reads_as_running() {
        // The defect L found in the exit-code form: it answered "gone", a false DEAD -- a caller
        // asserting "the child is gone" would have passed on a failure to observe. `None` is what
        // the free function turns into "running" and what the identity turns into an error; neither
        // may turn it into "gone".
        assert_eq!(
            liveness_from_wait(0xFFFF_FFFF),
            None,
            "WAIT_FAILED must not decide"
        );
        assert_eq!(
            liveness_from_wait(1),
            None,
            "an unknown wait code must not decide"
        );
    }

    #[test]
    fn a_wait_that_decided_is_believed_in_both_directions() {
        assert_eq!(
            liveness_from_wait(WAIT_SIGNALED),
            Some(false),
            "a signaled process handle means the process exited"
        );
        assert_eq!(
            liveness_from_wait(WAIT_STILL_RUNNING),
            Some(true),
            "a wait that timed out means it is still running"
        );
    }

    /// The ambiguity this replaced, pinned as a property rather than left in prose.
    ///
    /// `GetExitCodeProcess` answered 259 for a process that had NOT exited and for one that exited
    /// WITH 259, so those two states were indistinguishable and the fixture guard covered only
    /// `fake_tool`. A signaled handle has no such overlap.
    #[test]
    fn no_wait_code_means_both_running_and_exited() {
        assert_ne!(
            liveness_from_wait(WAIT_STILL_RUNNING),
            liveness_from_wait(WAIT_SIGNALED),
            "the two states must be distinguishable, which is the whole reason for the change"
        );
        assert_ne!(
            WAIT_SIGNALED, WAIT_STILL_RUNNING,
            "and distinguishable at the source, not only after interpretation"
        );
    }

    #[test]
    fn a_refused_signal_means_the_process_exists() {
        // EPERM is refusal, not absence: only a running process can refuse. The same false-dead
        // direction as the Windows case, on the platform the report did not name.
        assert!(
            decide_liveness_from_signal(false, true),
            "a permission-denied signal must read as running"
        );
        assert!(
            decide_liveness_from_signal(true, false),
            "delivered is alive"
        );
        assert!(
            !decide_liveness_from_signal(false, false),
            "undelivered and not denied is gone"
        );
    }

    /// Requirement (1): below the floor the instrument REFUSES; it never answers with the id.
    ///
    /// Process id 0 cannot be opened on Windows and cannot be signalled meaningfully on Unix, so
    /// `capture` has nothing to bind to. The property under test is the SHAPE of that outcome: an
    /// `Err` the caller must handle, rather than a `bool` that quietly means "I used the number
    /// instead". A fallback here is the `unwrap_or` that turns a known gap into silent drift.
    #[test]
    fn an_unbindable_process_refuses_instead_of_falling_back_to_the_id() {
        let refused = ProcessIdentity::capture(0);
        // `Err(IdentityUnavailable)` is a unit-struct PATTERN, and it only is one because the type
        // is imported above. Without that import the same line binds a fresh variable and matches
        // ANY error -- a pattern that always passes, wearing the shape of one that discriminates.
        // Clippy is what caught it here; the import is load-bearing rather than tidy.
        assert_eq!(
            refused.err(),
            Some(IdentityUnavailable),
            "capture must refuse rather than hand back an id-shaped answer"
        );
    }

    /// Requirement (2), the half this platform can prove: the identity SURVIVES the reap.
    ///
    /// A child is spawned, bound, killed and reaped. The bare id is then free to mean something
    /// else -- that is the false-red path -- but the held identity still answers, and answers GONE.
    /// A bare-id check has no way to promise the same after the reap.
    ///
    /// **What this cell proves and what it cites.** It proves the binding outlives the process and
    /// keeps reporting the original. That the OS will not REASSIGN the id while a handle is held is
    /// a documented Windows guarantee (the reservation is why the handle is held at all) and is not
    /// proven here: arming it would need to exhaust the id space under churn, which is a stress test
    /// rather than a cell. On Linux the same property comes from the `pidfd` referring to the
    /// process rather than the number, and it is CI's Linux leg that exercises it -- the half not
    /// run here is the half that carries the weight there.
    #[test]
    fn a_bound_identity_still_answers_after_the_process_is_reaped() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .arg("--this-argument-makes-the-test-binary-exit-immediately")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("a child to bind to");
        let identity = ProcessIdentity::capture(child.id()).expect("the live child binds");
        assert_eq!(identity.process_id(), child.id());

        let _ = child.kill();
        let _ = child.wait();

        match identity.is_running() {
            Ok(alive) => assert!(
                !alive,
                "the reaped child must read as gone through its own identity"
            ),
            Err(error) => panic!("the identity stopped answering after the reap: {error}"),
        }
    }

    /// The identity can also ACT, not only observe -- and it acts on the process it names.
    ///
    /// **Why the crate owns this instead of a caller running `kill`.** A caller that holds an
    /// identity and then shells out to `kill <pid>` is safe on Windows, where the held handle
    /// reserves the id, and unsafe on Linux, where a pidfd does not: between the liveness answer
    /// and the signal the number can be reassigned, and the signal lands on a stranger. One method
    /// hides that asymmetry from every caller rather than asking each to remember it (Codex, on
    /// #703, after the tree-kill cell did exactly the unsafe thing).
    ///
    /// The cell proves the ACT reaches its subject. It does not prove the negative -- that no
    /// unrelated process is ever signalled -- which would need id exhaustion under churn and is a
    /// stress test rather than a cell. What carries that half is the mechanism: on Linux the signal
    /// goes through the pidfd, which cannot name a successor at all.
    /// A child that outlives the cell unless something kills it. Its LIFETIME is what bounds a
    /// failing run, since no cell here decides by clock: a kill that does nothing is reported when
    /// the sleeper ends.
    fn sleeper() -> std::process::Child {
        if cfg!(windows) {
            let mut command = std::process::Command::new("ping");
            command.args(["-n", "30", "127.0.0.1"]);
            command
        } else {
            let mut command = std::process::Command::new("sleep");
            command.arg("30");
            command
        }
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("a long-lived child to bind to")
    }

    /// `INFINITE` is not a long wait, it is no wait bound at all -- so the conversion must never
    /// produce it, however absurd the patience.
    #[test]
    fn an_oversized_patience_never_becomes_an_unbounded_wait() {
        for patience in [
            std::time::Duration::MAX,
            std::time::Duration::from_millis(u64::from(u32::MAX)),
            std::time::Duration::from_millis(u64::from(u32::MAX) + 1),
        ] {
            let milliseconds = windows_wait_milliseconds(patience);
            assert_ne!(
                milliseconds,
                u32::MAX,
                "a patience of {patience:?} became INFINITE, which is a hang and not a bound"
            );
        }
    }

    /// The control: ordinary patiences pass through EXACTLY, so the cap above is a cap and not a
    /// clamp that quietly rewrites every caller's bound.
    #[test]
    fn an_ordinary_patience_is_passed_through_unchanged() {
        assert_eq!(
            windows_wait_milliseconds(std::time::Duration::from_secs(30)),
            30_000
        );
        assert_eq!(windows_wait_milliseconds(std::time::Duration::ZERO), 0);
    }

    /// The wait returns as soon as the process goes, and it is the OS that says so.
    ///
    /// Paired with the control below, because "gone" from a call that ALWAYS says gone would be
    /// worth nothing. Together they say the wait discriminates: a killed process reports gone well
    /// inside a generous bound, and a living one reports still-here at a short one.
    #[test]
    fn the_wait_returns_when_the_process_goes_and_not_before() {
        let mut sleeper = sleeper();
        let identity = ProcessIdentity::capture(sleeper.id()).expect("the live child binds");

        // THE CONTROL, first and on the same subject: a process that is running is not reported
        // gone. Short on purpose -- this bound is expected to be reached.
        let still_here = !identity
            .wait_until_gone(std::time::Duration::from_millis(200))
            .expect("the wait answers");

        // And the control needs its own control, because the subject is MORTAL. `sleeper()` ends on
        // its own eventually, so a host that descheduled this thread long enough would have the
        // wait correctly report an exit and this cell blame `wait_until_gone` for it (Codex, on
        // #703). Asking the child whether it is still there removes the time assumption entirely:
        // if it ended by itself, the ARRANGEMENT failed and there is nothing here to conclude.
        if let Some(status) = sleeper.try_wait().expect("the sleeper can be asked") {
            panic!(
                "HARNESS-BROKE: the sleeper ended on its own ({status}) before the control ran, so \
                 nothing here measures whether a running process is reported as gone"
            );
        }

        assert!(still_here, "a running sleeper was reported as gone");

        identity
            .terminate()
            .expect("the identity kills its own process");

        // Generous on purpose: this bound must NOT be reached, so its size costs nothing on the
        // path that matters and only bounds a genuine failure.
        let gone = identity
            .wait_until_gone(std::time::Duration::from_secs(30))
            .expect("the wait answers after the kill");
        let status = sleeper.wait().expect("the sleeper is reaped");

        assert!(gone, "the wait did not report a killed process as gone");
        assert!(
            !status.success(),
            "the sleeper exited successfully, so nothing killed it and this cell measured nothing"
        );
    }

    #[test]
    fn an_identity_terminates_the_process_it_names() {
        let mut sleeper = sleeper();

        let identity = ProcessIdentity::capture(sleeper.id()).expect("the live child binds");
        assert!(
            identity.is_running().expect("the identity answers"),
            "ARRANGEMENT: the sleeper must be running, or nothing below measures a kill"
        );

        identity
            .terminate()
            .expect("the identity kills its own process");

        // NO CLOCK. `TerminateProcess` and `SIGKILL` are both asynchronous, so a polling window is
        // an elapsed-time assertion about an event the OS reports exactly (Codex, on #703): on a
        // loaded runner a fixed window can expire while the kill was working perfectly, and the
        // gate then fails for the scheduler's reasons. `wait` returns when the process is gone.
        let status = sleeper.wait().expect("the sleeper is reaped");

        // But waiting ALONE would be vacuous, and that is the trap inside the obvious fix. A sleeper
        // nothing killed exits when its own sleep ends; `wait` returns just the same, and the
        // identity then reports it gone -- a cell that passes whether or not `terminate` did
        // anything. The exit STATUS separates the two: a killed process is never a success, and the
        // sleeper's own completion is always exit 0.
        assert!(
            !status.success(),
            "the sleeper exited successfully, which is what it does when nothing kills it -- \
             `terminate` did not reach it"
        );

        assert!(
            !identity
                .is_running()
                .expect("the identity keeps answering after the kill"),
            "the identity still reports the sleeper as running after it was reaped"
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod zombie_liveness {
    /// #715: a PID TABLE ENTRY IS NOT A RUNNING PROCESS.
    ///
    /// `kill(pid, 0)` succeeds against a zombie -- a process that exited and whose parent has not
    /// reaped it -- so a correctly killed descendant used to read as ALIVE until its adopter got
    /// round to it. On a host whose pid 1 reaps slowly, containers especially, a cell asserting a
    /// tree is gone would fail while the kill was perfectly correct.
    ///
    /// The zombie here is DELIBERATE and is made the only way it can be: spawn a child that exits
    /// at once and never `wait` on it. Rust does not reap on drop, so the entry stays.
    ///
    /// **The CONTROL is a live child**, and it is what makes the assertion mean something. Without
    /// it, a `process_is_running` that had been broken to return `false` for everything would
    /// satisfy the zombie assertion and look like the fix.
    #[test]
    fn a_zombie_is_not_running_and_a_live_child_still_is() {
        let mut alive = std::process::Command::new("/bin/sleep")
            .arg("30")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the live child spawns");

        // CONTROL FIRST, so a broken probe is caught before the finding is asserted.
        assert!(
            super::process_is_running(alive.id()),
            "CONTROL: a live child read as NOT running, so the assertion below would pass for the \
             wrong reason"
        );

        #[allow(clippy::zombie_processes)]
        let departed = std::process::Command::new("/bin/true")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the short-lived child spawns");
        let zombie = departed.id();

        // It has to actually have exited, or this measures a live process. Bounded, and an
        // exhausted bound says so rather than asserting on a state it never reached.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut reached_zombie = false;
        while std::time::Instant::now() < deadline {
            if super::read_stat(zombie).is_some_and(|stat| stat.state == 'Z') {
                reached_zombie = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            reached_zombie,
            "ARRANGEMENT: the child never became a zombie within 5s, so this run measures nothing"
        );

        assert!(
            !super::process_is_running(zombie),
            "a zombie read as RUNNING: a correctly killed descendant looks alive until its adopter \
             reaps it, and a gate asserting the tree is gone fails on the OS's schedule"
        );

        let _ = alive.kill();
        let _ = alive.wait();
        let mut departed = departed;
        let _ = departed.wait();
    }
}
