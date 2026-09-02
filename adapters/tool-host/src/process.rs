//! The scrubbed process primitive: argv-only spawning inside a workspace, with an allowlist
//! environment, deadline kill, and output caps. Every Tier 1 execution in this crate funnels
//! through [`run_in_workspace`]; there is no second spawn path to keep honest.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
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
/// **Descendants.** [`Child::kill`] ends the direct child; a grandchild it spawned survives and is
/// reparented (Codex, #609). Worse than a leak: a grandchild that inherited stdout or stderr keeps
/// those pipes open, so the reader joins below block after the direct child is already reaped — the
/// failure shows up as a wedged caller rather than as a cancellation that did not take. Closing it
/// means a Windows job object or a Unix process group, which is platform work this change does not
/// carry. Tracked as #618.
///
/// This comment used to add that nothing in this repository reaches the gap today, on the grounds
/// that `fake_tool` spawns nothing and the builtin tools run `git` directly. That was a measurement
/// of the FIXTURES wearing the clothes of a claim about the FUNNEL. `validate_program_name` checks
/// the shape of a name only — it is not an allowlist — and `ToolCall::Tests` runs
/// `config.tests_runner`, an operator-supplied program whose whole job is to spawn other processes.
/// `TestsTool` in production is the shape that reaches this, and it reaches it every time; the
/// repository's own tests do not only because they set `tests_runner` to `fake_tool`.
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
    /// The host's cancellation signal was already raised when this call asked to spawn (#609).
    /// Refused rather than spawned-then-killed: `cancel` waits only for the children it knows
    /// about, so one created after it returned would outlive the guarantee it had just given.
    #[error("the host was cancelled before this call could spawn")]
    Cancelled,
    /// The index-snapshot copy no longer hashes to its recorded generation (#539). Carries both
    /// values — re-pin or investigate, never read anyway.
    #[error("the index snapshot does not match its pin")]
    SnapshotMismatch { expected: String, actual: String },
}

/// The fixed inheritance allowlist. Everything else the parent holds — passphrases, tokens,
/// profile paths — is structurally absent from the child. `HOME`/`USERPROFILE`/`TEMP`/`TMP`
/// are not inherited but REDIRECTED into the workspace so git and tools that insist on a home
/// write inside the sandbox and read no host config (`GIT_CONFIG_NOSYSTEM` closes the /etc
/// side). Note the contrast with the 05b gateway's allowlist, which deliberately keeps the
/// host `HOME`/`APPDATA` because official CLIs own their own auth: a tool workspace has no
/// auth of its own to keep, so Tier 1 is stricter by design.
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
const REDIRECTED: &[&str] = &["HOME", "USERPROFILE", "TEMP", "TMP", "CBM_CACHE_DIR"];

/// The one address a broker-run index provider reads its store from: the funnel sets
/// `CBM_CACHE_DIR` here (#538), so anything that must be VISIBLE to the provider — the pinned
/// snapshot's serving copy — must be placed here, by the session, before the spawn.
pub(crate) fn cbm_cache_dir(root: &Path) -> PathBuf {
    root.join(".cbm-cache")
}

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

    let home = root.join(".home");
    let tmp = root.join(".tmp");
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

    let mut child = command
        .spawn()
        .map_err(|source| HostError::Spawn { source })?;

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
    let reader = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
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
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
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
            (kept, truncated)
        })
    };
    let stdout_reader = reader(Box::new(stdout_pipe));
    let stderr_reader = reader(Box::new(stderr_pipe));

    // The 05b runtime-adapter pattern: poll every 50 ms against the deadline; on expiry kill
    // and reap, never leaving a zombie.
    let deadline = Instant::now() + limits.timeout;
    let mut timed_out = false;
    let mut was_cancelled = false;
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                let expired = Instant::now() >= deadline;
                let cancelled = cancel.is_some_and(CancelSignal::is_cancelled);
                // #180: cancellation kills through the SAME reaping path as the deadline. The
                // two differ only in `timed_out`, because a cancelled call did not run out of
                // time -- recording it as a timeout would put a false cause in the record.
                if expired || cancelled {
                    let _ = child.kill();
                    let status = child.wait().ok();
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
                    break status;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };

    if let Some(writer) = stdin_writer {
        let _ = writer.join();
    }
    let (stdout, stdout_truncated) = stdout_reader.join().unwrap_or_default();
    let (stderr, stderr_truncated) = stderr_reader.join().unwrap_or_default();

    Ok(CapturedProcess {
        exit_code: exit_status.and_then(|status| status.code()),
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
        truncated: stdout_truncated || stderr_truncated,
        timed_out,
        cancelled: was_cancelled,
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
