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

#[cfg(windows)]
pub fn configure(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Threading::CREATE_SUSPENDED;
    command.creation_flags(CREATE_SUSPENDED);
}

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

/// Whether a process id belongs to something still running.
///
/// **Public, and it was the second-most duplicated thing in this move.** It lived in `backup.rs`
/// behind `cfg(test)`, and `adapters/tool-host` grew an identical pair while #621 was being written
/// — the author had searched for the precedent of the FIX and not for the precedent of the
/// INSTRUMENT. One helper, every suite.
///
/// **An id is not a process.** Once a child is reaped its id may be recycled, so this answers
/// "something with this id is running", not "that child is running". A recycle reads as ALIVE, so a
/// guard built on it fails toward red rather than toward green — which is the only reason it is
/// usable as an oracle at all.
/// What a Windows process that has not exited reports as its exit code (`STATUS_PENDING`).
const STILL_ACTIVE: u32 = 259;

/// The decision the Windows query feeds, separated so BOTH directions are testable without an
/// operating system that fails on demand.
///
/// **A query that could not decide reads as RUNNING** (L, on #680). The previous form was
/// `queried && code == STILL_ACTIVE`, so an `OpenProcess` that succeeded followed by a
/// `GetExitCodeProcess` that failed answered NOT-RUNNING — the one direction this helper's whole
/// justification says it never takes. A caller asserting "the child is gone" would have passed on a
/// failure to observe. Reading an undecidable query as running costs nothing and keeps the property.
/// Compiled on every platform, and unused on the ones whose query it does not describe — declared
/// here rather than silenced, because the point of hoisting the decision out of the `unsafe` block
/// is that BOTH platforms' test suites exercise both directions.
#[cfg_attr(not(windows), allow(dead_code))]
#[must_use]
fn decide_liveness(query_succeeded: bool, exit_code: u32) -> bool {
    !query_succeeded || exit_code == STILL_ACTIVE
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

#[cfg(windows)]
#[must_use]
pub fn process_is_running(process_id: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        // No handle is the ONE undecidable case that reads as absent, and it earns that: the id is
        // gone, or it belongs to something this process may not even ask about -- and on Windows a
        // reaped id stops being openable. Distinguishing further would need a privilege this helper
        // must not require.
        return false;
    }
    let mut exit_code = 0_u32;
    let queried = unsafe { GetExitCodeProcess(handle, &mut exit_code) } != 0;
    unsafe { CloseHandle(handle) };
    decide_liveness(queried, exit_code)
}

#[cfg(test)]
mod tests {
    use super::{STILL_ACTIVE, decide_liveness, decide_liveness_from_signal};

    /// The direction is the property, so both directions are pinned rather than the happy one.
    #[test]
    fn a_query_that_could_not_decide_reads_as_running() {
        // The defect L found: this returned false, which is a false DEAD -- a caller asserting
        // "the child is gone" would have passed on a failure to observe.
        assert!(
            decide_liveness(false, 0),
            "a failed exit-code query must read as running, never as gone"
        );
        assert!(
            decide_liveness(false, STILL_ACTIVE),
            "a failed query reads as running whatever the untouched buffer happens to hold"
        );
    }

    #[test]
    fn a_query_that_decided_is_believed_in_both_directions() {
        assert!(decide_liveness(true, STILL_ACTIVE), "still active is alive");
        assert!(!decide_liveness(true, 0), "a real exit code is gone");
        assert!(
            !decide_liveness(true, 1),
            "a nonzero exit code is still an exit"
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
}
