Warning: truncated output (original token count: 31143)
Total output lines: 2480

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
//! | what `close` does | `SIGKILL` to the process group (#714), while the caller still holds the leader unreaped | kills the job's remaining MEMBERS (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) |
//!
//! **One caveat on the Windows column, because the table would otherwise overstate it** (Codex, on
//! #746). "Cannot leave" is about breakaway, and there is a second way to be outside a job: never
//! having joined it. Nothing in this API forces [`configure`] to run before the spawn, and a child
//! spawned without it runs immediately — so anything IT starts before [`create`] assigns the job is
//! outside the job, and neither [`terminate`] nor [`close`] reaches it. That is why the suspension in
//! [`configure`] is load-bearing rather than tidiness, and why `adapters/tool-host`'s funnel calls it.
//! The Windows advantage is real and it is conditional on that call.
//!
//! **#878 asked whether a grandchild can escape through that interval. Along the documented path
//! the interval carries no running child.** [`configure`] sets `CREATE_SUSPENDED` and
//! [`create`] resumes only after the assignment succeeds, so the child executes no instruction
//! before it is contained and has nothing to spawn a descendant with.
//!
//! **And the caller who skips [`configure`] is now REFUSED rather than documented.**
//! `ResumeThread` returns the thread's previous suspend count, which this crate was
//! discarding; zero means nothing ever suspended it, so the child has been running since the
//! spawn and the group [`create`] would return promises a containment it cannot deliver.
//! [`create`] answers [`ProcessTreeError::ChildNotSuspended`] instead. That is #878's second
//! question -- whether the ordering can be tightened rather than mitigated -- answered by
//! eliminating the window instead of shrinking it, and it costs one comparison on a value the
//! OS was already returning.
//!
//! `tests/suspended_window.rs` holds the pair: a configured child is accepted, an
//! otherwise-identical unconfigured one is refused, and a third cell pins that the refusal is
//! that specific error rather than a job that failed to build. They ask the operating system
//! rather than the clock -- an earlier version slept and looked for a marker file, and a
//! reviewer showed that two cells in separate tests meet different scheduler load, so neither
//! established the other's interval.
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
//!    and the blocked reader returns microseconds later; on Unix `close` USED TO do nothing, so the
//!    descendant lived, kept the pipe, and the thread blocked forever — two stranded threads and
//!    their buffers per invocation. #714 closed that half: the Unix release now signals the group,
//!    and the leader stays unreaped until it has, so the pgid it signals is still the group's.
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
    /// The child was NOT suspended, so [`configure`] was never called on its command and it has
    /// been running since the spawn (Windows only).
    ///
    /// This is #878's window, detected instead of documented. `ResumeThread` returns the
    /// thread's PREVIOUS suspend count, and zero means nothing ever suspended it -- so between
    /// the spawn and this assignment the child was free to start descendants, and every one of
    /// them is outside the job. The group this call would return promises containment it cannot
    /// deliver, which is worse than no group at all: a caller that believes it can kill the tree
    /// stops looking. Refusing is the only answer that does not lie.
    ///
    /// **WHAT THE COUNT CANNOT SEE, because it is a state and not a provenance.** It says every
    /// thread is suspended NOW; it does not say when or why. A hostile executable spawned
    /// without [`configure`] could start a descendant and then suspend all of its own threads,
    /// and this check would accept it. So the refusal catches the ACCIDENT -- a caller who
    /// forgot -- and not an adversary, and the crate's containment claim for an untrusted
    /// executable rests on the funnel calling [`configure`], not on this. Closing that would
    /// need the spawn and the assignment to be one operation the caller cannot separate, which
    /// is a different API and a different change (Codex P2 on #1027).
    ///
    /// **THIS ERROR IS DESTRUCTIVE: the child is DEAD when it is returned.** The suspend count
    /// is only knowable after the job has taken the process, the job carries
    /// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and a process cannot leave a job -- so releasing
    /// the handle on the refusal path terminates it. A caller must not treat this as a
    /// non-destructive rejection it can retry; the child is gone, and any descendants it
    /// started before the assignment are NOT, which is the whole point of the refusal.
    ChildNotSuspended,
}

/// Output returned by [`run_bounded`] when the contained process completed and cleanup was
/// observed.
pub type BoundedOutput = (bool, String, String);

/// Run a command with one execution deadline and bounded process-tree cleanup.
///
/// The execution deadline covers leader observation and both output streams. Cleanup has its own
/// bounded observer because terminating a Windows job is asynchronous. A cleanup bound is reported
/// as an error rather than being mistaken for a contained timeout.
pub fn run_bounded(
    mut command: std::process::Command,
    timeout: std::time::Duration,
) -> Result<Option<BoundedOutput>, String> {
    configure(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cargo did not start: {error}"))?;
    let mut group = match create(&child) {
        Ok(group) => group,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("process tree setup failed: {error}"));
        }
    };
    let readers = [
        child
            .stdout
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|pipe| Box::new(pipe) as Box<dyn std::io::Read + Send>),
    ]
    .map(|pipe| {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            let _ = sender.send(String::from_utf8_lossy(&bytes).into_owned());
        });
        receiver
    });
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match leader_exited(&mut child) {
            Ok(true) => {
                let read_to_deadline = |reader: &std::sync::mpsc::Receiver<String>| match reader
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                {
                    Ok(output) => Some(output),
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(String::new()),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                };
                let out = read_to_deadline(&readers[0]);
                let err = out.as_ref().and_then(|_| read_to_deadline(&readers[1]));
                if let (Some(out), Some(err)) = (out, err) {
                    let succeeded = cleanup_bounded(&mut child, &mut group, true)?;
                    return Ok(Some((succeeded, out, err)));
                }
                cleanup_bounded(&mut child, &mut group, true)?;
                return Ok(None);
            }
            Ok(false) if std::time::Instant::now() >= deadline => {
                cleanup_bounded(&mut child, &mut group, false)?;
                return Ok(None);
            }
            Ok(false) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(error) => {
                cleanup_bounded(&mut child, &mut group, false)?;
                return Err(format!("waiting on cargo failed: {error}"));
            }
        }
    }
}

fn cleanup_bounded(
    child: &mut std::process::Child,
    group: &mut ProcessGroup,
    leader_already_exited: bool,
) -> Result<bool, String> {
    let outcome = terminate(child.id(), *group);
    close(group);
    let succeeded = reap_bounded(child, leader_already_exited)?;
    match outcome {
        TerminationOutcome::Complete => Ok(succeeded),
        other => Err(format!("process-tree cleanup inconclusive: {other:?}")),
    }
}

fn reap_bounded(
    child: &mut std::process::Child,
    leader_already_exited: bool,
) -> Result<bool, String> {
    #[cfg(not(windows))]
    let _ = leader_already_exited;
    #[cfg(windows)]
    if leader_already_exited {
        return child
            .wait()
            .map(|status| status.success())
            .map_err(|error| format!("reaping cargo failed: {error}"));
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        if leader_exited(child).map_err(|error| format!("reaping cargo failed: {error}"))? {
            #[cfg(unix)]
            return child
                .wait()
                .map(|status| status.success())
                .map_err(|error| format!("reaping cargo failed: {error}"));
            #[cfg(windows)]
            return child
                .wait()
                .map(|status| status.success())
                .map_err(|error| format!("reaping cargo failed: {error}"));
        }
        if std::time::Instant::now() >= deadline {
            return Err("process-tree cleanup could not reap cargo within 1s".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

impl std::fmt::Display for ProcessTreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::JobSetup => formatter.write_str("the process job object could not be set up"),
            Self::ProcessResume => {
                formatter.write_str("the suspended process could not be resumed")
            }
            Self::ChildNotSuspended => formatter.write_str(
                "the child was not suspended, so it ran before the job could contain it",
            ),
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
/// Opaque, and one type name on both platforms so callers need no `cfg` of their own. On Unix it
/// carries the child's own process group id; on Windows it is a job object handle. [`close`] is a
/// destructive release on both (#714).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessGroup(GroupHandle);

/// On Unix the handle is the child's own process group id, kept only when the child is that
/// group's LEADER -- which is what [`configure`]'s `process_group(0)` makes it. `None` means the
/// crate is holding nothing and [`close`] must do nothing: either the group was never established,
/// or it has already been released.
#[cfg(unix)]
type GroupHandle = Option<u32>;

#[cfg(windows)]
type GroupHandle = usize;

#[cfg(unix)]
impl ProcessGroup {
    const EMPTY: Self = Self(None);
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
pub fn create(child: &std::process::Child) -> Result<ProcessGroup, ProcessTreeError> {
    // The group is the child's own, and this ASKS THE KERNEL rather than assuming it (#714).
    // Nothing in this API forces [`configure`] to run before the spawn -- the funnels do, but a
    // caller that skipped it has a child sitting in the PARENT'S group, and `kill(-pid)` would
    // then signal a group this crate never created, including the caller's own processes. So the
    // pgid is kept only when it equals the child's pid, which is exactly "this child leads the
    // group `configure` put it in". Anything else keeps `None`, and [`close`] does nothing on that
    // path rather than signalling a group this crate does not own.
    //
    // Read HERE rather than at close time. The caller must keep the leader unreaped until close,
    // and after the reap the pid no longer resolves.
    let pid = child.id();
    let leads_its_own_group = i32::try_from(pid).is_ok_and(|pid| {
        let pgid = unsafe { libc::getpgid(pid) };
        pgid == pid
    });
    Ok(ProcessGroup(leads_its_own_group.then_some(pid)))
}

/// The same group, usable from another thread.
#[must_use]
pub fn for_thread(group: ProcessGroup) -> ProcessGroup {
    group
}

/// Has the leader finished, asked WITHOUT reaping it?
///
/// The anchor [`close`] depends on (#714). `Child::try_wait` answers the same question and pays
/// for the answer with the pid: it reaps, the kernel is free to reissue that number, and the pgid
/// [`close`] holds stops naming the group it was read from. This asks and leaves the zombie in
/// place, so the number stays taken until the caller reaps -- which it must do, AFTER [`close`].
///
/// On Unix that is `waitid` with `WNOWAIT`, the one wait primitive that does not consume the
/// child's exit state. On Windows it is `Child::try_wait` unchanged: there the group is a job
/// HANDLE, the handle is the identity, and a reap invalidates nothing.
///
/// A caller that never reaps afterwards leaks a zombie per invocation; a caller that reaps before
/// [`close`] gets the defect this exists to prevent. Both are visible to the exit ordering and one
/// of them is asserted in `adapters/tool-host/tests/process_isolation.rs`.
///
/// # Errors
///
/// The platform wait failed. `EINTR` is retried rather than reported.
#[cfg(unix)]
pub fn leader_exited(child: &mut std::process::Child) -> std::io::Result<bool> {
    wait_leaving_the_zombie(child, libc::WNOHANG)
}

/// Block until the leader has finished, WITHOUT reaping it. See [`leader_exited`].
///
/// # Errors
///
/// The platform wait failed. `EINTR` is retried rather than reported.
#[cfg(unix)]
pub fn await_leader_exit(child: &mut std::process::Child) -> std::io::Result<()> {
    wait_leaving_the_zombie(child, 0).map(|_| ())
}

/// `waitid(P_PID, ..., WEXITED | WNOWAIT)`: reports the exit and leaves the child unreaped.
///
/// `si_signo` is the "did anything happen" flag. `waitid` returns 0 under `WNOHANG` whether or not
/// the child changed state, and POSIX makes the zeroed `siginfo_t` the way to tell the two apart --
/// so the struct is zeroed BEFORE the call and read after. A `si_pid()` accessor would say the same
/// thing on Linux and is not portable across the Unixes this crate compiles for.
#[cfg(unix)]
fn wait_leaving_the_zombie(
    child: &std::process::Child,
    extra_options: libc::c_int,
) -> std::io::Result<bool> {
    let pid = libc::id_t::from(child.id());
    loop {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let answered = unsafe {
            libc::waitid(
                libc::P_PID,
                pid,
                std::ptr::addr_of_mut!(info),
                libc::WEXITED | libc::WNOWAIT | extra_options,
            )
        };
        if answered == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        return Ok(info.si_signo != 0);
    }
}

/// Has the leader finished? On Windows this is `Child::try_wait`, and the reap costs nothing.
///
/// See the Unix body for why the two platforms need the same call under one name: there the group
/// is a pgid, which is only its leader's pid, so the reap has to wait for [`close`]. Here the group
/// is a job handle, which no reap can invalidate.
///
/// # Errors
///
/// `Child::try_wait` failed.
#[cfg(windows)]
pub fn leader_exited(child: &mut std::process::Child) -> std::io::Result<bool> {
    child.try_wait().map(|status| status.is_some())
}

/// Block until the leader has finished. See [`leader_exited`].
///
/// # Errors
///
/// `Child::wait` failed.
#[cfg(windows)]
pub fn await_leader_exit(child: &mut std::process::Child) -> std::io::Result<()> {
    child.wait().map(|_| ())
}

/// Release the group: `SIGKILL` to the whole process group, once.
///
/// **This used to be a no-op, and the no-op was the defect** (#714). The Windows job carries
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, so closing it KILLS whatever is still inside; here there
/// was nothing to close and nothing died, so a caller that treated `close` as cleanup got cleanup
/// on one platform and a comment on the other. The measured consequence was #726 and then #714:
/// when a capture releases its readers, `close` runs -- on Windows the escaped descendant dies,
/// its inherited pipe handle closes and the blocked reader returns; on Unix it lived, kept the
/// pipe, and the capture was discarded with the bytes still inside the reader.
///
/// So the two platforms now mean the same thing by "release", and the Windows semantics are the
/// reference: **this call is DESTRUCTIVE and it always was on one platform**. A caller that wants
/// the group to keep running must not call it.
///
/// It reaches only what is still IN the group, which is the same qualifier [`terminate`] carries
/// on this platform (#717): a descendant that called `setsid` has left, and only the subtree sweep
/// in [`terminate`] can find it. Taking the pgid makes a second call a no-op, so a caller that
/// closes twice does not signal twice.
///
/// # The pgid is only as stable as its leader's pid, and that is a CALLER obligation
///
/// A pgid is not an identity. It is the pid of the process that led the group, and a pid belongs
/// to whoever the kernel last handed it to. Reap the leader and the number is free: the kernel may
/// give it to any other job of the same user, and this `SIGKILL` then lands on a group this crate
/// never created. That is not a five-second race on the cancel path -- it is every path, including
/// the ordinary EOF one, and it is the reason the first version of this function was blocked
/// (Codex, on PR #1170).
///
/// **So the anchor is the leader itself, held UNREAPED until this call has run.** A zombie still
/// occupies its pid, and an occupied pid cannot be reissued, so the number below keeps naming the
/// group [`create`] read it from. [`leader_exited`] and [`await_leader_exit`] exist so a caller can
/// learn that the child is finished without giving that anchor up; the reap goes AFTER the release.
/// A fresh `getpgid` here would not do instead -- it answers about the instant it runs and the
/// signal is sent after it, which is the same TOCTOU one line narrower.
///
/// The obligation is the caller's because only the caller owns the [`std::process::Child`], and it
/// is stated here because a caller who reaps first gets a function that still looks correct.
#[cfg(unix)]
pub fn close(group: &mut ProcessGroup) {
    let Some(pgid) = group.0 else {
        return;
    };
    // Emptied BEFORE the signal and by the same statement shape the Windows arm uses, so a second
    // release is a no-op on either platform rather than a second `SIGKILL` at a pgid the kernel
    // may have reissued.
    *group = ProcessGroup::EMPTY;
    let Ok(pgid) = i32::try_from(pgid) else {
        return;
    };
    // Negative pid: the GROUP, not the leader. The leader is normally a zombie by now -- held as
    // the anchor above -- and signalling it alone would leave exactly the descendant that is
    // holding the pipe.
    unsafe { libc::kill(-pgid, libc::SIGKILL) };
}

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
    ///
    /// **`passes` is PER-PLATFORM and not comparable across them.** On Unix a pass is a full subtree
    /// walk and the bound is a count (`MAX_PASSES`); on Windows a pass is one liveness poll of the
    /// enumerated members with a 1 ms sleep, and the bound is time (`JOB_DRAIN_CEILING`), so the number
    /// can reach the thousands. Compare it to its own platform's bound, never to the other's.
    ///
    /// **`passes: 0` means NO PASS WAS MADE**, not a bound hit: on Windows the job's membership could
    /// not be read, so nothing was enumerated and nothing was waited on, and `remaining` there is
    /// UNKNOWN rather than zero -- the `0` it carries is the absence of a reading, not a count of
    /// survivors. A real ceiling always reports at least one pass. Said on the variant because a
    /// consumer reads the enum, not the producer that documents the signature. (Raised in review of
    /// #826 by two lanes.)
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
/// `#[cfg(uni…15143 tokens truncated…ntity {
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
    ///
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

/// The Windows drain's REFUSAL path, which the flake cell cannot reach.
///
/// `cancelled_watchdog_kills_the_owned_process_tree` over in `graphhelm-postgres-event-store` is the
/// guard for the ordinary path: it failed 8 times in 20 on main and passes 40 of 40 with the wait in
/// place. What it never exercises is a job whose membership cannot be READ, because a real job
/// always can be -- and that is exactly the branch where a wrong default would be invisible, since
/// answering `Complete` there looks identical to answering it correctly.
#[cfg(all(test, windows))]
mod job_drain_refusal {
    use super::{ProcessGroup, TerminationOutcome, terminate};

    #[test]
    fn a_job_whose_membership_cannot_be_read_is_not_reported_as_drained() {
        // A non-zero handle that is not a job. It takes the job branch -- the arm under test -- and
        // every call made there fails, which is the state a real failure would produce. Zero would
        // take the OTHER branch and measure nothing about this one.
        let not_a_job = ProcessGroup(usize::MAX);

        let outcome = terminate(std::process::id(), not_a_job);

        // ASSERTED AS THE WHOLE VALUE, not `matches!(.., BoundReached { .. })`: `passes: 0` is the
        // documented signature of "membership could not be read", and a coarser assertion would
        // accept a real ceiling being hit -- which would mean this ran the 5-second wait against the
        // CURRENT PROCESS and still called it a bound, a very different bug reading as this pass.
        assert_eq!(
            outcome,
            TerminationOutcome::BoundReached {
                passes: 0,
                remaining: 0
            },
            "an unreadable job must fail toward BoundReached, never toward Complete"
        );

        // And the control that makes the assertion above mean what it says: this process is STILL
        // RUNNING. Without it, `BoundReached` would be consistent with `terminate` having killed the
        // test runner's own tree, and the cell would pass by not existing any more.
        assert!(
            super::process_is_running(std::process::id()),
            "HARNESS-BROKE: terminate() acted on this process; the outcome above is not about a refusal"
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
