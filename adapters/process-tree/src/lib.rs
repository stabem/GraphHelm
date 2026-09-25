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
    let previous_suspend_count = match resume_suspended_process(child.id()) {
        Ok(count) => count,
        Err(error) => {
            unsafe { CloseHandle(job) };
            return Err(error);
        }
    };
    // #878, DETECTED RATHER THAN DOCUMENTED. Zero means nothing had suspended this thread, so
    // [`configure`] was never called and the child has been running since the spawn -- free, for
    // that whole interval, to start descendants that this job will never contain. Returning a
    // group here would promise a containment it cannot deliver, and a caller that believes it
    // can kill the tree stops looking.
    //
    // THIS REFUSAL KILLS THE CHILD, and an earlier version of this comment claimed the opposite.
    // By the time the suspend count is known the child is ALREADY a member of a job carrying
    // `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and a process cannot leave a job -- Windows offers no
    // removal, which is the same property that makes the container worth having. So closing the
    // handle on this path terminates it. Leaving the handle open to spare the child would leak
    // it and keep the kill pending on a value nobody holds, which is worse (Codex P2 on #1027).
    //
    // Refusing destructively is also the right direction on its merits: this child has been
    // running unconfined since the spawn and may already have started descendants outside the
    // job. Killing it does not reach those -- nothing here can -- but leaving it running would
    // add a process nobody is tracking to a hole nobody knew about. The contract is stated on
    // the error and asserted by a cell, so a caller reads it rather than discovers it.
    if previous_suspend_count == 0 {
        unsafe { CloseHandle(job) };
        return Err(ProcessTreeError::ChildNotSuspended);
    }
    Ok(ProcessGroup(job as usize))
}

#[cfg(windows)]
/// Resume the child and report its thread's PREVIOUS suspend count.
///
/// The count is the whole of #878's answer and it was being discarded. `ResumeThread` returns
/// how many times the thread had been suspended BEFORE this call: one for a child spawned with
/// `CREATE_SUSPENDED`, and ZERO for a child that has been running since the spawn -- which is
/// exactly the condition under which descendants can be started outside the job.
fn resume_suspended_process(process_id: u32) -> Result<u32, ProcessTreeError> {
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
    // EVERY THREAD, and the MINIMUM of their counts. The first version resumed the first thread
    // it could open and stopped: for a `CREATE_SUSPENDED` child that is the only thread and the
    // answer is exact, but for a child that was never configured the snapshot can hand back a
    // suspended WORKER while the primary runs, and a non-zero count from it would report a
    // containment that never held (Codex P2 on #1027). A process is suspended only if all of its
    // threads are, so the minimum is the honest reading. Resuming a thread that was not
    // suspended is a no-op that returns 0, which is exactly the value that must refuse.
    //
    // NOT COVERED BY A CELL, and said so rather than covered by one that cannot discriminate.
    // Telling MINIMUM from FIRST needs a child with a suspended non-primary thread, and nothing
    // this suite can spawn arranges that: a cooperative process has no suspended threads, so a
    // cell built from one passes under both readings and would be evidence of nothing. The
    // existing pair still covers the property that matters -- configured is accepted,
    // unconfigured is refused -- and this line is the narrower claim it does not reach.
    let mut found = unsafe { Thread32First(snapshot, std::ptr::addr_of_mut!(entry)) } != 0;
    let mut previous_suspend_count: Option<u32> = None;
    while found {
        if entry.th32OwnerProcessID == process_id {
            let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            if !thread.is_null() {
                let count = unsafe { ResumeThread(thread) };
                unsafe { CloseHandle(thread) };
                if count != u32::MAX {
                    previous_suspend_count = Some(match previous_suspend_count {
                        Some(lowest) => lowest.min(count),
                        None => count,
                    });
                }
            }
        }
        found = unsafe { Thread32Next(snapshot, std::ptr::addr_of_mut!(entry)) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    previous_suspend_count.ok_or(ProcessTreeError::ProcessResume)
}

/// Release the job handle — **which KILLS the job's remaining members**.
///
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the handle's lifetime the kill policy, so this is not
/// merely cleanup: it is a second kill, and callers order it after their readers for that reason.
///
/// **The Unix counterpart now does the same thing by a different mechanism** (#714). There a
/// process group has no handle, so the release is an explicit `SIGKILL` to the group -- and
/// because a pgid is only its leader's pid, that platform also asks its caller to keep the leader
/// unreaped until the release has run. Written on BOTH bodies deliberately: each is invisible in
/// the other's rendered documentation, and the reader who most needs to know how the two differ is
/// the one reading only the platform they are on. What is still NOT symmetric is escape (#717): a
/// descendant that called `setsid` has left the Unix group and survives this call; nothing leaves
/// a job object.
///
/// The measured consequence is #726: when a capture abandons its readers, `close` still runs. Here
/// the job's remaining members die, their inherited pipe handles close, and the blocked reader thread
/// returns within microseconds. On Unix that now happens too, for members that did not leave.
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
/// **`TerminateJobObject` REQUESTS termination and returns without waiting for it**, so this used to
/// answer [`TerminationOutcome::Complete`] unconditionally -- a value documented as *"signalled
/// everything reachable, and a final pass found nothing new"* on a path where no pass was ever made.
/// The kernel guarantees CONTAINMENT, not that containment has finished by the time the call
/// returns. Measured on main before this changed (#824): the parent, which `Drop` explicitly waits
/// for, was gone in 20 of 20 runs; the descendant, which nothing waited for, was still alive in 6 of
/// 20 and died 1.03-1.30 ms later -- never escaping, merely not yet drained.
///
/// So this OBSERVES the drain before saying `Complete`, and answers
/// [`TerminationOutcome::BoundReached`] when the ceiling arrives first. That variant's existing
/// meaning -- still appearing, there may be more -- is right here, and reusing it is deliberate so
/// that [`TerminationOutcome::SweepUnavailable`], which already carries two meanings (#815), does not
/// acquire a third.
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
        // THE IDS ARE READ BEFORE THE KILL, and that order is the whole of it. Measured (#824):
        // `JobObjectBasicAccountingInformation.ActiveProcesses` reads ZERO on the very first query
        // after `TerminateJobObject` -- pass 1, every run -- while the descendant is still answering
        // "running" to the caller. Job accounting stops counting a process before its handle
        // signals, so it cannot be the completion predicate. Enumerate first, then wait on what was
        // enumerated.
        //
        // AND THE LIST IS OLDER THAN THE KILL. A process a listed member spawns between the
        // enumeration and `TerminateJobObject` is in the job -- the kernel kills it -- but is not in
        // `members`, and THIS is not closed (#846, found in review of #826 by the GraphHelm ISSUES
        // lane). A re-read after the kill was tried and removed: measured 2026-09-08, a
        // `job_member_ids` query taken immediately, at 5ms and at 55ms after `TerminateJobObject`
        // against a real job holding two real members all answered an empty list -- the job's own
        // accounting empties before or with the kill, the same way `ActiveProcesses` does (#824), so a
        // post-kill re-read cannot observe the thing it would exist to catch. `Complete` on this arm
        // means "every member THIS enumeration named has been observed gone", not "the whole job is
        // empty" -- see `drain_terminated_job`'s own doc for the open window this leaves and issue
        // #846 for the measurement.
        let members = job_member_ids(group);
        unsafe { TerminateJobObject(group.0 as _, 1) };
        // `passes: 0` IS THE SIGNATURE of "membership could not be read", and it is distinguishable
        // from a real ceiling, which always reports at least one pass. Both are `BoundReached`
        // because both mean the same thing to a caller: this did not observe the tree go.
        let Some(members) = members else {
            return TerminationOutcome::BoundReached {
                passes: 0,
                remaining: 0,
            };
        };
        return drain_terminated_job(&members);
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

/// How long [`terminate`] waits for a terminated job to empty before answering
/// [`TerminationOutcome::BoundReached`].
///
/// **A CEILING, NOT AN EXPECTED COST.** Measured drain on an idle host is 1.03-1.30 ms (#824), so the
/// loop below normally exits on its second pass and this is some four thousand times the observed
/// figure. It is set that far above because the gate runs the workspace in parallel and the same
/// ficha measured a cell taking 20.60 s under gate load against 0.43 s isolated -- a 48x stretch. A
/// bound chosen from idle numbers would turn a slow machine into a `BoundReached`, which reads as
/// "descendants are still appearing": a false alarm about the product caused by the host.
///
/// The loop needs a ceiling more than a clever condition. Without one, a job that never empties makes
/// `terminate` HANG, and a hang has no colour -- the gate has no per-test timeout, so it would stop
/// rather than redden.
#[cfg(windows)]
const JOB_DRAIN_CEILING: std::time::Duration = std::time::Duration::from_secs(5);

/// The most job members this will enumerate before it stops claiming to have seen them all.
///
/// A job holding more than this is not a case this crate can answer `Complete` for honestly, so the
/// overflow is reported as `BoundReached` with the unlisted count in `remaining` rather than silently
/// waiting on a prefix and calling it the whole.
#[cfg(windows)]
const JOB_MEMBER_LIST_CAP: usize = 1024;

/// The process ids currently assigned to the job.
///
/// **Must be called BEFORE `TerminateJobObject`.** Afterwards the list empties as fast as the
/// accounting does, and an empty list would read as "nothing to wait for" -- the exact false green
/// this whole path exists to remove.
///
/// Returns the ids seen and how many were assigned but did not fit, or `None` when the membership
/// could not be READ at all.
///
/// **`None` is not an empty job**, and collapsing the two is the failure this signature exists to
/// prevent: an unreadable job would yield no ids, the wait would have nothing to wait for, and the
/// answer would be `Complete` -- a confident "the tree is gone" derived from having failed to look.
/// The caller turns `None` into `BoundReached` instead, so the unobservable case fails toward the
/// same colour as the observed-incomplete one.
#[cfg(windows)]
fn job_member_ids(group: ProcessGroup) -> Option<(Vec<u32>, usize)> {
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
    };

    let header = std::mem::size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
    let slot = std::mem::size_of::<usize>();
    let bytes = header + slot * JOB_MEMBER_LIST_CAP;
    let mut buffer = vec![0u8; bytes];
    // SAFETY: `group.0` is a live job handle this process created and owns; the buffer is at least
    // the size of the structure and is written only up to the length passed. The return value is
    // checked before anything is read out of it.
    let queried = unsafe {
        QueryInformationJobObject(
            group.0 as _,
            JobObjectBasicProcessIdList,
            buffer.as_mut_ptr().cast(),
            u32::try_from(bytes).unwrap_or(u32::MAX),
            std::ptr::null_mut(),
        )
    };
    if queried == 0 {
        return None;
    }
    // SAFETY: a successful query wrote a valid header followed by `NumberOfProcessIdsInList` slots,
    // and the buffer was allocated with room for the header plus `JOB_MEMBER_LIST_CAP` of them.
    let list = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
    let listed = (list.NumberOfProcessIdsInList as usize).min(JOB_MEMBER_LIST_CAP);
    let assigned = list.NumberOfAssignedProcesses as usize;
    let mut ids = Vec::with_capacity(listed);
    for index in 0..listed {
        // SAFETY: `ProcessIdList` is a flexible array of `listed` valid entries inside the buffer.
        let id = unsafe { *list.ProcessIdList.as_ptr().add(index) };
        if let Ok(id) = u32::try_from(id) {
            ids.push(id);
        }
    }
    Some((ids, assigned.saturating_sub(listed)))
}

/// Wait for the processes that were in a job when it was terminated to actually be gone.
///
/// Waits only on the membership `terminate` enumerated BEFORE calling `TerminateJobObject` --
/// nothing here re-reads the job afterward. A member assigned to the job between that enumeration
/// and the kill is terminated by the job -- containment is real -- but is not named by `members`,
/// and this function has no way to learn about it: `job_member_ids`'s own doc already says the list
/// empties as fast as the accounting does, and measuring it directly (2026-09-08, #846) confirmed
/// there is no post-kill instant where a re-read still names a genuinely-late member -- immediately,
/// at 5ms and at 55ms after `TerminateJobObject`, against a real job holding two real assigned
/// members, the answer was an empty list every time. A re-read branch lived here briefly to try to
/// close that window and was removed for this reason: code that cannot observe what it exists to
/// catch is not a fix, it is a claim with no consumer. The window stays open and is named here
/// instead of hidden behind a branch that never fires. See issue #846 for the measurement and a
/// candidate mechanism (pre-kill fixpoint enumeration) that would narrow it, left for a later PR.
///
/// **The predicate is the one the CALLER uses**, and that is the correction this function exists to
/// carry (#824): `Complete` means every member THIS enumeration named was observed gone, checked by
/// `process_is_running` rather than by polling job accounting, which stops counting a member before
/// its handle signals. The first version polled the job's `ActiveProcesses` and returned `Complete`
/// on pass 1, every run, while `process_is_running` still answered true for the descendant -- so the
/// flake was unchanged and the fix looked applied. Two instruments disagreeing about the same
/// instant; the one that decides is the one a caller can observe.
#[cfg(windows)]
fn drain_terminated_job(members: &(Vec<u32>, usize)) -> TerminationOutcome {
    let mut ids = members.0.clone();
    let unlisted = members.1;
    let started = std::time::Instant::now();
    let mut passes: u32 = 0;
    loop {
        passes = passes.saturating_add(1);
        ids.retain(|id| process_is_running(*id));
        if ids.is_empty() {
            if unlisted == 0 {
                return TerminationOutcome::Complete;
            }
            return TerminationOutcome::BoundReached {
                passes,
                remaining: unlisted,
            };
        }
        if started.elapsed() >= JOB_DRAIN_CEILING {
            return TerminationOutcome::BoundReached {
                passes,
                remaining: ids.len() + unlisted,
            };
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[cfg(all(test, windows))]
mod post_enumeration_join_window {
    use super::{
        ProcessGroup, ProcessIdentity, TerminationOutcome, drain_terminated_job, job_member_ids,
        process_is_running,
    };
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        CREATE_SUSPENDED, OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE, TerminateProcess,
    };

    /// A process that never runs is still a process: `CREATE_SUSPENDED` leaves it alive
    /// (`process_is_running` checks the process handle's signal state, not its threads), so this is a
    /// real, assignable job member without needing it to execute any code. The arguments name a test
    /// that does not exist; they are never read, because this is never resumed.
    fn spawn_suspended_member() -> std::process::Child {
        std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "process_tree::post_enumeration_join_window::never_run",
                "--nocapture",
            ])
            .creation_flags(CREATE_SUSPENDED)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("ARRANGEMENT: suspended fixture process spawns")
    }

    // `Child::wait()` has no independent timeout. A successful termination request makes the
    // process handle expected to signal, but a request that failed is not a reason to block here:
    // the test has no per-test timeout that could turn such a wait into a useful red. Cleanup below
    // therefore waits only after a confirmed request and otherwise makes one non-blocking `try_wait`
    // observation.

    /// Owns the raw test job and gives it the same kill-on-close policy as production `create`.
    ///
    /// The fixture deliberately creates this job by hand so it can arrange a member that joins
    /// after the stale membership read. Keeping the handle in an owner makes the panic path safe:
    /// dropping it kills every fixture process that was successfully assigned, including one left
    /// behind by a failed assertion before the normal cleanup reaches it.
    struct OwnedKillOnCloseJob(ProcessGroup);

    impl OwnedKillOnCloseJob {
        fn new() -> Self {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            assert!(!handle.is_null(), "ARRANGEMENT: CreateJobObjectW failed");
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let configured = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&limits).cast(),
                    u32::try_from(std::mem::size_of_val(&limits)).unwrap(),
                )
            } != 0;
            if !configured {
                unsafe { CloseHandle(handle) };
                panic!("ARRANGEMENT: could not configure kill-on-close for the fixture job");
            }
            Self(ProcessGroup(handle as usize))
        }

        fn group(&self) -> ProcessGroup {
            self.0
        }
    }

    impl Drop for OwnedKillOnCloseJob {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0.0 as _) };
        }
    }

    fn terminate_directly(process_id: u32) {
        let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, process_id) };
        assert!(
            !handle.is_null(),
            "ARRANGEMENT: could not open the fixture process to end it"
        );
        let requested = unsafe { TerminateProcess(handle, 1) };
        unsafe { CloseHandle(handle) };
        assert_ne!(
            requested, 0,
            "ARRANGEMENT: could not terminate the fixture process"
        );
    }

    /// End a retained fixture child using its owned handle, then reap only after the request was
    /// accepted. A child that already exited needs only the non-blocking observation; a failed kill
    /// must never turn cleanup into an unbounded wait. The job owner is the second line of defense
    /// for children that were assigned before an assertion failed.
    fn cleanup_child(child: &mut Option<std::process::Child>) {
        let Some(mut child) = child.take() else {
            return;
        };
        if child.kill().is_ok() {
            let _ = child.wait();
        } else {
            let _ = child.try_wait();
        }
    }

    fn assign_to_job(job: super::ProcessGroup, process_id: u32) {
        let process = unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, process_id) };
        assert!(
            !process.is_null(),
            "ARRANGEMENT: could not open the fixture process to assign it"
        );
        let assigned = unsafe { AssignProcessToJobObject(job.0 as _, process) };
        unsafe { CloseHandle(process) };
        assert_ne!(
            assigned, 0,
            "ARRANGEMENT: could not assign the fixture process to the job"
        );
    }

    /// #846 (redesigned 2026-09-08): a member the FIRST enumeration missed -- because it joined the
    /// job after that read, in the window `terminate` does not control -- is genuinely killed by the
    /// job, but the drain has no way to learn about it and does not claim to have waited for it.
    /// `drain_terminated_job`'s own doc names why: a post-kill re-read was measured against a real
    /// terminated job and always came back empty, so there is nothing to re-read.
    ///
    /// **Deterministic by construction, not by racing a live spawn against a live kill.** `late` joins
    /// the SAME real job `stale` was read from, so the job genuinely holds a member the snapshot does
    /// not name -- exactly #846's shape -- but `late`'s liveness for the rest of this test is left
    /// alone, not raced against anything. Nothing here depends on how fast Windows happens to tear a
    /// process down.
    ///
    /// **Cleanup runs even when an assertion panics.** A first draft of this cell left a suspended
    /// fixture process running on a panic path -- it holds no lock on anything of ITS OWN, but it is a
    /// live copy of THIS CRATE'S OWN TEST BINARY, and that file being open is exactly what made the
    /// next `cargo test` fail to link (measured while writing this test: `LNK1104`, the previous
    /// process still holding `graphhelm_process_tree-*.exe`). So the arrangement below happens before
    /// any assertion that can fail, and everything that can panic is wrapped so the two fixture
    /// processes and the job handle are always ended, panic or not.
    #[test]
    fn a_member_assigned_after_enumeration_is_not_waited_for() {
        // EVERY FIXTURE LIVES BEHIND THIS GUARD FROM THE MOMENT IT EXISTS (Codex, second pass): the
        // first draft created `enumerated` (and assigned it to the job) and `late` BEFORE
        // `catch_unwind` began, so a panic in that first `assign_to_job`'s own assertion, or in the
        // second `spawn_suspended_member()`, skipped the unconditional cleanup below entirely --
        // `enumerated` was created suspended and `Child` does not terminate on drop, so a panic there
        // leaked a live copy of this crate's own test binary, exactly the `LNK1104` failure the
        // comment on this test already names. Nothing is created outside the closure now; each
        // `Option` starts empty and the unconditional cleanup after `catch_unwind` acts only on
        // whatever actually got made before the panic (if any).
        let mut job = None;
        let mut enumerated: Option<std::process::Child> = None;
        let mut late: Option<std::process::Child> = None;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let owned_job = OwnedKillOnCloseJob::new();
            let group = owned_job.group();
            job = Some(owned_job);

            let enumerated_child = spawn_suspended_member();
            let enumerated_id = enumerated_child.id();
            enumerated = Some(enumerated_child);
            assign_to_job(group, enumerated_id);

            let late_child = spawn_suspended_member();
            let late_id = late_child.id();
            late = Some(late_child);

            let stale =
                job_member_ids(group).expect("ARRANGEMENT: the job's membership could be read");
            assert_eq!(
                stale.0,
                vec![enumerated_id],
                "ARRANGEMENT: the stale snapshot names only the first member, or this cell tests \
                 nothing -- `late` must not be assigned to the job before this read"
            );

            // JOINS AFTER THE SNAPSHOT, exactly like a member's grandchild would (#846). Real job
            // membership, real liveness -- `stale` simply never heard about it, and nothing from here
            // on re-reads the job to find out.
            assign_to_job(group, late_id);

            // Only the TRACKED member dies. `late` is left alive and untouched -- the drain must not
            // wait for it.
            terminate_directly(enumerated_id);

            let drain = std::thread::spawn(move || drain_terminated_job(&stale));

            // EXPLICIT SYNCHRONIZATION, NOT A WALL-CLOCK RACE (Codex, second pass): a bounded poll
            // here would compare THIS thread's elapsed time against a margin over
            // `JOB_DRAIN_CEILING`, but that margin describes this thread's own scheduling, not the
            // drain thread's -- under the documented 48x gate-load stretch (#750) the two can drift
            // far enough apart for the poll to expire before the drain thread was ever scheduled,
            // reddening a correct implementation. `drain_terminated_job` already carries its own
            // bound, measured from INSIDE that thread with a roughly 4000x margin over the idle case
            // (#824) -- so this joins and trusts it, rather than laying a second, weaker bound on top
            // from a different thread's clock.
            let outcome = drain.join().expect("the drain thread did not panic");
            assert_eq!(
                outcome,
                TerminationOutcome::Complete,
                "the drain only waits on the pre-kill enumeration, so it must report Complete once \
                 `enumerated` is gone -- regardless of `late`"
            );

            // THE HONEST CLAIM, MADE OBSERVABLE: `late` is still alive. `Complete` above did not mean
            // "the whole job emptied" -- it means "everything this drain was told to wait for is
            // gone" -- and this is what tells the two apart. If this ever fails, the window this test
            // exists to document has closed and both this cell and the doc above need revisiting.
            assert!(
                process_is_running(late_id),
                "HARNESS-BROKE (or the documented gap closed): `late` should still be alive here, or \
                 the Complete assertion above proves nothing about the window"
            );

            terminate_directly(late_id);

            // `.wait()`, NOT A FIXED-DEADLINE POLL (Codex, third pass): `Child::wait` has no timeout
            // of its own. The direct termination calls above assert that the OS accepted both
            // requests, so these waits reap the two known-terminated children; cleanup below never
            // waits after an unconfirmed request.
            if let Some(child) = enumerated.as_mut() {
                child
                    .wait()
                    .expect("the terminated `enumerated` fixture process could be waited on");
            }
            if let Some(child) = late.as_mut() {
                child
                    .wait()
                    .expect("the terminated `late` fixture process could be waited on");
            }
        }));

        // ALWAYS: drop the owned job first so its kill-on-close policy reaches every assigned
        // member, even when setup panicked before the normal termination calls. Then use each
        // retained `Child` handle for an unassigned fixture; opening a fresh handle by PID was the
        // bug because a failed open skipped both kill and wait while `Child` itself does not kill
        // on drop. `cleanup_child` never performs an unbounded wait after a failed request.
        drop(job);
        cleanup_child(&mut enumerated);
        cleanup_child(&mut late);

        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }

    /// The cleanup boundary itself: a child created before an assignment failure is not in the
    /// job, so kill-on-close cannot reach it. The retained `Child` handle must still end and reap
    /// that child after the assignment assertion panics.
    #[test]
    fn failed_assignment_cleanup_reaps_the_retained_child() {
        let mut job = None;
        let mut child: Option<std::process::Child> = None;
        let mut child_identity = None;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let owned_job = OwnedKillOnCloseJob::new();
            job = Some(owned_job);
            let child_process = spawn_suspended_member();
            let id = child_process.id();
            child = Some(child_process);
            // Bind the identity before cleanup consumes and reaps the Child. The bare PID can be
            // reassigned after that reap, so it is not an adequate observation target.
            child_identity =
                Some(ProcessIdentity::capture(id).expect(
                    "ARRANGEMENT: the live failed-assignment child must bind to an identity",
                ));

            // An impossible process id makes the assignment fail before the child joins the job.
            // This is the path the job guard cannot cover, and the retained handle must cover.
            assign_to_job(
                job.as_ref().expect("the fixture job exists").group(),
                u32::MAX,
            );
        }));

        drop(job);
        cleanup_child(&mut child);
        assert!(
            result.is_err(),
            "ARRANGEMENT: the forced assignment must panic"
        );
        assert!(
            !child_identity
                .as_ref()
                .expect("the child identity must survive cleanup")
                .is_running()
                .expect("the retained identity must remain observable after cleanup"),
            "a child retained across failed assignment cleanup must not survive the test"
        );
    }
}

/// `SYNCHRONIZE` (`0x0010_0000`): the access right that permits WAITING on a handle.
///
/// Declared here rather than imported: `windows-sys` exposes it only under
/// `Win32_Storage_FileSystem`, as a FILE access right, and pulling that feature in for one constant
/// would widen this crate's surface for no other reason. The value is the documented Win32 one.
#[cfg(windows)]
const SYNCHRONIZE: u32 = 0x0010_0000;

/// `WAIT_OBJECT_0`: the handle is SIGNALED, which for a process handle means it has exited.
#[cfg_attr(not(windows), allow(dead_code))]
const WAIT_SIGNALED: u32 = 0;
/// `WAIT_TIMEOUT`: nothing happened in the interval, so the process is still running.
#[cfg_attr(not(windows), allow(dead_code))]
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
///
/// **A pure match over `u32`, nothing Windows in the body** (C's review of #877): unlike
/// `SYNCHRONIZE`, its only production caller is `#[cfg(windows)]`, but the function itself is not,
/// which is exactly what makes it testable off the platform. `#[cfg(windows)]` here would have
/// removed that -- the same `#[cfg_attr(not(windows), allow(dead_code))]` shape as
/// `windows_wait_milliseconds` below silences the dead-code lint without removing the code (or its
/// cells) from Linux.
#[cfg_attr(not(windows), allow(dead_code))]
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
