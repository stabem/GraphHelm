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
/// On Windows the child starts SUSPENDED, because a job object can only be assigned after the
/// process exists and a child that ran first could spawn descendants outside the job. [`create`]
/// resumes it once the assignment holds.
#[cfg(unix)]
pub fn configure(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
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
#[cfg(unix)]
pub fn close(_group: &mut ProcessGroup) {}

/// Kill the process and everything it spawned.
#[cfg(unix)]
pub fn terminate(process_id: u32, _group: ProcessGroup) {
    if let Ok(process_id) = i32::try_from(process_id) {
        // NEGATIVE pid: the signal goes to the whole process group, which is the difference
        // between this and `Child::kill`.
        unsafe {
            libc::kill(-process_id, libc::SIGKILL);
        }
    }
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

#[cfg(windows)]
pub fn close(group: &mut ProcessGroup) {
    if group.0 != 0 {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(group.0 as _) };
        *group = ProcessGroup::EMPTY;
    }
}

#[cfg(windows)]
pub fn terminate(process_id: u32, group: ProcessGroup) {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::{
            JobObjects::TerminateJobObject,
            Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess},
        },
    };
    if group.0 != 0 {
        unsafe { TerminateJobObject(group.0 as _, 1) };
        return;
    }
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, process_id) };
    if !handle.is_null() {
        unsafe {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
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
    decide_liveness_from_signal(sent, permission_denied)
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
}

#[cfg(test)]
mod tests {
    use super::{
        IdentityUnavailable, ProcessIdentity, WAIT_SIGNALED, WAIT_STILL_RUNNING,
        decide_liveness_from_signal, liveness_from_wait,
    };

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
}
