//! Explicit host commands. No shell, inherited input, detached readers or unbounded joins.
pub mod claude;
pub mod codex;
mod packages;
pub(crate) use packages::{PackageEntry, create_package_root, prepare_entries};
pub use packages::{PinnedPackage, release_packages};
pub(crate) use packages::{
    compensate_packages, install, package_receipts, preflight_compensation, prepare_packages,
    verify_installed,
};

use graphhelm_protocols::adoption::{AdoptionError, AdoptionReason};
use std::path::PathBuf;
#[cfg(windows)]
use std::{
    io::Read,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct HostOperation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub timeout_ms: u64,
}
#[derive(Debug)]
pub struct HostReply {
    pub success: bool,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
fn invalid() -> AdoptionError {
    AdoptionError {
        reason: AdoptionReason::InvalidConfiguration,
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum HostCapability {
    Supported,
    ActionRequired,
    Unsupported,
}
pub fn installation_capability(host: &str, capabilities: &serde_json::Value) -> HostCapability {
    if capabilities["versionCompatible"] != true {
        return HostCapability::Unsupported;
    }
    match host {
        "claude" if capabilities["pluginDir"] == true => HostCapability::Supported,
        "codex" => HostCapability::ActionRequired,
        _ => HostCapability::Unsupported,
    }
}

pub(crate) fn preflight_host(plan: &serde_json::Value) -> Result<(), AdoptionError> {
    // A local settings file cannot disclose server, MDM, SDK or OS managed policy. Do not
    // partially disable a methodology before a trusted host observation supplies that boundary.
    if plan["spec"]["operations"].as_array().is_some_and(|ops| {
        ops.iter().any(|o| {
            o["disablePlugins"]
                .as_array()
                .is_some_and(|ids| !ids.is_empty())
        })
    }) {
        return Err(AdoptionError {
            reason: AdoptionReason::HostPolicyRequired,
        });
    }
    let has_packages = plan["spec"]["packages"]
        .as_array()
        .is_some_and(|p| !p.is_empty());
    let config = plan["spec"]["operations"]
        .as_array()
        .is_some_and(|ops| ops.iter().any(|o| o["path"] == ".codex/config.toml"));
    if !has_packages && !config {
        return Ok(());
    }
    let host = &plan["spec"]["host"];
    let name = host["name"].as_str().ok_or_else(invalid)?;
    if config && name != "codex" {
        return Err(AdoptionError {
            reason: AdoptionReason::HostUnsupported,
        });
    }
    let program = PathBuf::from(host["program"].as_str().ok_or_else(invalid)?);
    if !program.is_absolute() || (has_packages && host["mode"] != "local_plugin_dir") {
        return Err(invalid());
    }
    let version = run_host(&HostOperation {
        program: program.clone(),
        args: vec!["--version".into()],
        timeout_ms: 3000,
    })?;
    let expected = host["version"].as_str().ok_or_else(invalid)?;
    let observed = std::str::from_utf8(&version.stdout).map_err(|_| invalid())?;
    let observed_version = if name == "codex" {
        observed
            .strip_prefix("codex-cli ")
            .unwrap_or(observed)
            .split_whitespace()
            .next()
    } else {
        observed.split_whitespace().next()
    };
    let compatible = version.success
        && observed_version == Some(expected)
        && match name {
            "claude" => expected.split('.').next() == Some("2"),
            "codex" => true,
            _ => false,
        };
    // Exact version with a checked upstream config schema. Unknown versions cannot inherit
    // support from a user-supplied boolean. https://github.com/openai/codex/tree/rust-v0.114.0
    if config && !has_packages {
        return if name == "codex"
            && expected == "0.114.0"
            && compatible
            && host["mode"] == "configuration"
        {
            Ok(())
        } else {
            Err(AdoptionError {
                reason: AdoptionReason::HostUnsupported,
            })
        };
    }
    let help = run_host(&HostOperation {
        program,
        args: vec!["--help".into()],
        timeout_ms: 3000,
    })?;
    let capabilities = serde_json::json!({"versionCompatible":compatible, "pluginDir":help.success && String::from_utf8_lossy(&help.stdout).split_whitespace().any(|word| word == "--plugin-dir")});
    match installation_capability(name, &capabilities) {
        HostCapability::Supported => Ok(()),
        HostCapability::ActionRequired => Err(AdoptionError {
            reason: AdoptionReason::HostActionRequired,
        }),
        HostCapability::Unsupported => Err(AdoptionError {
            reason: AdoptionReason::HostUnsupported,
        }),
    }
}
pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.split('@').all(|part| {
            part.as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
        })
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'@'))
        && !value.contains("..")
        && value.bytes().filter(|b| *b == b'@').count() <= 1
}

/// Execution requires an enforced process containment boundary. Currently only Windows Job
/// Objects provide that backend; other platforms refuse before spawning, including host probes.
/// A process group is not containment: descendants can escape it with setsid/setpgid.
pub fn run_host(operation: &HostOperation) -> Result<HostReply, AdoptionError> {
    if operation.timeout_ms == 0
        || operation.timeout_ms > 60_000
        || operation.args.len() > 64
        || operation
            .args
            .iter()
            .any(|a| a.len() > 32768 || a.contains('\0'))
    {
        return Err(invalid());
    }
    if operation.args.first().is_some_and(|s| s == "plugin")
        && (operation.args.len() != 5
            || !matches!(operation.args[1].as_str(), "install" | "disable" | "enable")
            || !valid_id(&operation.args[2])
            || operation.args[3] != "--scope"
            || !matches!(operation.args[4].as_str(), "project" | "user" | "local"))
    {
        return Err(invalid());
    }
    // Windows Command would otherwise run .bat/.cmd through cmd.exe implicitly.
    if operation
        .program
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
    {
        return Err(invalid());
    }
    #[cfg(not(windows))]
    {
        Err(AdoptionError {
            reason: AdoptionReason::HostContainmentUnavailable,
        })
    }
    #[cfg(windows)]
    {
        run_contained(operation)
    }
}

/// The same deadline covers execution, termination and pipe collection. OS process creation is
/// synchronous, not a real-time scheduling guarantee. No wait/join occurs after the deadline.
#[cfg(windows)]
fn run_contained(operation: &HostOperation) -> Result<HostReply, AdoptionError> {
    let started = Instant::now();
    let deadline = started + Duration::from_millis(operation.timeout_ms);
    // Reserve half of the operation budget, capped at 500 ms, for termination and inherited-pipe
    // collection. The old 20%-of-budget rule capped this at 100 ms, which was shorter than a
    // loaded Windows host sometimes needed to signal and observe every job member.
    let termination_budget_ms = (operation.timeout_ms / 2).clamp(1, 500);
    let terminate_at = deadline - Duration::from_millis(termination_budget_ms);
    let mut command = Command::new(&operation.program);
    command
        .args(&operation.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, CREATE_SUSPENDED};
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().map_err(|_| invalid())?;
    let tree = match ProcessTree::attach(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.kill();
            return Err(error);
        }
    };
    let mut stdout = child.stdout.take().ok_or_else(invalid)?;
    let mut stderr = child.stderr.take().ok_or_else(invalid)?;
    let mut reply = HostReply {
        success: false,
        timed_out: false,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    let (mut out_done, mut err_done) = (false, false);
    let mut status = None;
    let mut termination_confirmed = false;
    loop {
        if Instant::now() >= terminate_at && !reply.timed_out {
            reply.timed_out = true;
            tree.terminate_until(deadline)?;
            termination_confirmed = true;
        }
        if Instant::now() >= deadline {
            break;
        }
        if !out_done {
            out_done = read_available(&mut stdout, &mut reply.stdout)?;
        }
        if !err_done {
            err_done = read_available(&mut stderr, &mut reply.stderr)?;
        }
        if status.is_none() {
            status = child.try_wait().map_err(|_| invalid())?;
        }
        if out_done && err_done && status.is_some() {
            break;
        }
        std::thread::sleep(
            Duration::from_millis(2).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
    if !termination_confirmed {
        // A normal parent exit does not prove that a descendant has exited. The job is still
        // terminated and drained before a successful reply is allowed to escape.
        tree.terminate_until(deadline)?;
    }
    if !(out_done && err_done && status.is_some()) {
        // Termination confirmation covers the process handles enumerated in the job. The pipes
        // are the remaining observable boundary for descendants that inherited them, and an EOF
        // on both pipes is required before the operation can return. A deadline reached with an
        // open pipe means cleanup is incomplete, even when TerminateJobObject succeeded.
        return Err(cleanup_unconfirmed());
    }
    reply.success = !reply.timed_out && status.is_some_and(|s| s.success());
    // Dropping the pipes cannot block on a descendant holding a write handle. The retained handles
    // for the pre-kill member snapshot were observed exited above; Drop remains the containment
    // backstop for the documented snapshot-to-kill observation window.
    drop(tree);
    Ok(reply)
}

#[cfg(windows)]
fn read_available<T: Read + std::os::windows::io::AsRawHandle>(
    pipe: &mut T,
    captured: &mut Vec<u8>,
) -> Result<bool, AdoptionError> {
    let mut available = 0;
    // SAFETY: live pipe handle and output are valid; a null buffer requests only its byte count.
    if unsafe {
        windows_sys::Win32::System::Pipes::PeekNamedPipe(
            pipe.as_raw_handle(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return if std::io::Error::last_os_error().raw_os_error() == Some(109) {
            Ok(true)
        } else {
            Err(invalid())
        };
    }
    if available == 0 {
        return Ok(false);
    }
    read_chunk(pipe, captured, (available as usize).min(8192))
}
#[cfg(windows)]
fn read_chunk<T: Read>(
    pipe: &mut T,
    captured: &mut Vec<u8>,
    count: usize,
) -> Result<bool, AdoptionError> {
    let mut buffer = [0; 8192];
    match pipe.read(&mut buffer[..count]) {
        Ok(0) => Ok(true),
        Ok(n) => {
            captured.extend_from_slice(&buffer[..n.min(65536 - captured.len())]);
            Ok(false)
        }
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(false)
        }
        Err(_) => Err(invalid()),
    }
}

#[cfg(windows)]
struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessTree {
    fn attach(child: &std::process::Child) -> Result<Self, AdoptionError> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        // SAFETY: all structures have the documented sizes and live storage. The child is suspended
        // until assigned to this kill-on-close job, closing the spawn-before-assignment escape.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(invalid());
            }
            let tree = Self(job);
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) == 0
                || AssignProcessToJobObject(job, child.as_raw_handle()) == 0
            {
                return Err(invalid());
            }
            #[link(name = "ntdll")]
            unsafe extern "system" {
                fn NtResumeProcess(process: windows_sys::Win32::Foundation::HANDLE) -> i32;
            }
            if NtResumeProcess(child.as_raw_handle()) < 0 {
                return Err(invalid());
            }
            Ok(tree)
        }
    }
    fn terminate_until(&self, deadline: Instant) -> Result<(), AdoptionError> {
        use std::os::windows::io::RawHandle;
        use windows_sys::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, GetLastError};
        use windows_sys::Win32::System::JobObjects::{
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList,
            QueryInformationJobObject, TerminateJobObject,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };

        const MEMBER_CAP: usize = 1024;
        const WAIT_OBJECT_0: u32 = 0;
        let header = std::mem::size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        let bytes = header + std::mem::size_of::<usize>() * MEMBER_CAP;
        let query_members = || -> Result<Vec<u32>, AdoptionError> {
            // The storage is usize-aligned because the job structure contains a pointer-sized
            // flexible-array member. Do not cast an unaligned Vec<u8> allocation to this type.
            let words = bytes.div_ceil(std::mem::size_of::<usize>());
            let mut storage = vec![0usize; words];
            let buffer = storage.as_mut_ptr().cast::<u8>();
            let mut returned = 0u32;
            let queried = unsafe {
                QueryInformationJobObject(
                    self.0,
                    JobObjectBasicProcessIdList,
                    buffer.cast(),
                    u32::try_from(bytes).unwrap_or(u32::MAX),
                    &mut returned,
                )
            };
            if queried == 0 {
                return Err(cleanup_unconfirmed());
            }
            let list = unsafe { &*buffer.cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let assigned = list.NumberOfAssignedProcesses as usize;
            let listed = list.NumberOfProcessIdsInList as usize;
            if assigned > MEMBER_CAP || listed > MEMBER_CAP || listed != assigned {
                return Err(cleanup_unconfirmed());
            }
            let process_ids = unsafe {
                buffer
                    .add(std::mem::offset_of!(
                        JOBOBJECT_BASIC_PROCESS_ID_LIST,
                        ProcessIdList
                    ))
                    .cast::<usize>()
            };
            let mut pids = Vec::with_capacity(listed);
            for index in 0..listed {
                let pid = unsafe { *process_ids.add(index) };
                let Ok(pid) = u32::try_from(pid) else {
                    return Err(cleanup_unconfirmed());
                };
                pids.push(pid);
            }
            Ok(pids)
        };

        // Repeat the bounded snapshot, terminate, handle-drain, and post-termination membership
        // query. The post-query catches members that joined during the first snapshot-to-kill
        // window; only an observed empty job membership permits success.
        let mut pids = query_members()?;
        loop {
            if pids.is_empty() {
                return Ok(());
            }
            let mut handles: Vec<RawHandle> = Vec::with_capacity(pids.len());
            for pid in pids {
                let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                if handle.is_null() {
                    // A process that exited between the membership snapshot and OpenProcess is
                    // already gone. Other open failures leave the member unobserved and must fail
                    // closed.
                    if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
                        continue;
                    }
                    for handle in handles {
                        unsafe { CloseHandle(handle) };
                    }
                    return Err(cleanup_unconfirmed());
                }
                handles.push(handle);
            }
            if unsafe { TerminateJobObject(self.0, 1) } == 0 {
                for handle in handles {
                    unsafe { CloseHandle(handle) };
                }
                return Err(cleanup_unconfirmed());
            }
            let mut complete = true;
            for handle in handles {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let millis = remaining.as_millis().min(u32::MAX as u128) as u32;
                let wait = unsafe { WaitForSingleObject(handle, millis) };
                unsafe { CloseHandle(handle) };
                if wait != WAIT_OBJECT_0 {
                    complete = false;
                }
            }
            if !complete {
                return Err(cleanup_unconfirmed());
            }
            pids = query_members()?;
            if !pids.is_empty() && Instant::now() >= deadline {
                return Err(cleanup_unconfirmed());
            }
        }
    }
}

#[cfg(windows)]
fn cleanup_unconfirmed() -> AdoptionError {
    AdoptionError {
        reason: AdoptionReason::HostCleanupUnconfirmed,
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
