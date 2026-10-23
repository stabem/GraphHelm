//! #1962: `serve --supervise` keeps the Runtime up after an unexpected exit.
//!
//! The supervisor is a small parent that never opens the keyring, the journal or a port. It runs
//! the Runtime as a hidden child with its own argv (minus `--supervise`) and its own environment,
//! so the events key and the gateway key reach every restart the way they reached the first
//! start: inherited in memory, never written or put on a command line. When the child exits it
//! reads the lifecycle record (#41) under the lifetime lock:
//!
//! - `clean`: an owner stop or the test seam; the supervisor stops too and nothing restarts.
//! - `clean` + handed over (#1523): the successor now owns the lock; the supervisor waits for
//!   that lock and then judges the successor's own ending the same way.
//! - anything else (vanished, out of memory, panicked, serve error): start again after a
//!   backoff that doubles from 1 s to 60 s and resets once a run has lasted a minute.
//!
//! A Runtime that cannot even start (it dies within `FAST_EXIT` five times in a row) has a setup
//! error a restart cannot fix; the supervisor then stops and says so. On Windows the children
//! live in a kill-on-close job, so stopping the supervisor stops its Runtime and any successor.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::lifecycle::{Ending, ending};
use crate::args::ServeArgs;
use crate::output::Outcome;

/// Set on every child: the supervisor's pid. Not a secret.
const SUPERVISED_ENV: &str = "GRAPHHELM_SUPERVISED";
/// Set on a restarted child only: how many restarts this supervisor has made so far.
const RESTART_ENV: &str = "GRAPHHELM_SUPERVISOR_RESTART";
const FIRST_DELAY: Duration = Duration::from_secs(1);
const MAX_DELAY: Duration = Duration::from_secs(60);
/// A run at least this long resets the backoff.
const STEADY: Duration = Duration::from_secs(60);
/// A run shorter than this counts toward giving up.
const FAST_EXIT: Duration = Duration::from_secs(30);
const FAST_EXIT_LIMIT: u32 = 5;
const POLL: Duration = Duration::from_millis(500);
/// How long a handed-over run's successor may take to own the lock before it counts as dead.
const TAKEOVER: Duration = Duration::from_secs(60);

/// True in a child this supervisor started (and in that child's handover successor).
pub(super) fn is_supervised() -> bool {
    std::env::var_os(SUPERVISED_ENV).is_some()
}

/// The restart count a supervisor handed this run; never on a handover successor, which was
/// started by a live Runtime rather than after a death.
pub(super) fn restart_count() -> Option<u64> {
    if super::handover::is_successor() {
        return None;
    }
    std::env::var(RESTART_ENV).ok()?.parse().ok()
}

fn child_args() -> Vec<OsString> {
    std::env::args_os()
        .skip(1)
        .filter(|arg| arg != "--supervise")
        .collect()
}

/// The binary each start runs: the installed one under `--handover-install` when it exists, so a
/// restart after an update runs the update; otherwise the binary this supervisor was started from.
fn binary(args: &ServeArgs, own: &Path) -> PathBuf {
    args.handover_install
        .as_ref()
        .map(|root| {
            root.join("bin").join(if cfg!(windows) {
                "graphhelm.exe"
            } else {
                "graphhelm"
            })
        })
        .filter(|path| path.is_file())
        .unwrap_or_else(|| own.to_path_buf())
}

fn note(message: &str) {
    eprintln!("runtime.supervise {message}");
}

pub(super) fn run(args: &ServeArgs) -> Outcome {
    const COMMAND: &str = "serve";
    let Ok(own) = std::env::current_exe() else {
        return Outcome::internal(COMMAND, "the supervisor could not name its own binary");
    };
    let job = job::Job::new();
    let mut restarts: u64 = 0;
    let mut delay = FIRST_DELAY;
    let mut fast_exits = 0;
    loop {
        let mut command = graphhelm_process::command(binary(args, &own));
        command
            .args(child_args())
            .env(SUPERVISED_ENV, std::process::id().to_string())
            .stdin(std::process::Stdio::null());
        if restarts > 0 {
            command.env(RESTART_ENV, restarts.to_string());
        } else {
            command.env_remove(RESTART_ENV);
        }
        let started = Instant::now();
        let Ok(mut child) = command.spawn() else {
            return Outcome::internal(COMMAND, "the supervisor could not start the Runtime");
        };
        if let Some(job) = &job {
            job.adopt(&child);
        }
        let status = child.wait();
        let lasted = started.elapsed();
        let mut ending = settle(&args.events);
        // After a handover, follow the successor until it, too, has ended.
        let mut handed_from = None;
        while let Ending::HandedOver { pid } = ending {
            if handed_from != Some(pid) {
                note(&format!("handed over by pid {pid}; watching the successor"));
                handed_from = Some(pid);
            }
            ending = watch_successor(&args.events, pid);
        }
        if matches!(ending, Ending::Clean) {
            note("the Runtime stopped cleanly; the supervisor stops");
            return Outcome::success(COMMAND, serde_json::json!({ "supervised": true }));
        }
        if lasted >= STEADY || handed_from.is_some() {
            delay = FIRST_DELAY;
            fast_exits = 0;
        } else if lasted < FAST_EXIT {
            fast_exits += 1;
            if fast_exits >= FAST_EXIT_LIMIT {
                note("the Runtime exited at start five times in a row; the supervisor stops");
                return Outcome::internal(
                    COMMAND,
                    "the supervised Runtime exits at start; read its stderr",
                );
            }
        }
        let code = status
            .ok()
            .and_then(|status| status.code())
            .map_or_else(|| "none".to_owned(), |code| code.to_string());
        note(&format!(
            "the Runtime exited unexpectedly (code {code}); restarting in {}s",
            delay.as_secs()
        ));
        std::thread::sleep(delay);
        delay = (delay * 2).min(MAX_DELAY);
        restarts += 1;
    }
}

/// The ending of the child that just exited. Its lock is normally free already; a lock still
/// held is a successor taking over, or another process that is not ours to restart over.
fn settle(events: &Path) -> Ending {
    let deadline = Instant::now() + TAKEOVER;
    loop {
        if let Some(ending) = ending(events) {
            return ending;
        }
        if Instant::now() > deadline {
            return Ending::Clean;
        }
        std::thread::sleep(POLL);
    }
}

/// Waits while the successor of `from` owns the lock; returns that successor's ending, or
/// `Unexpected` when no successor ever took the lock.
fn watch_successor(events: &Path, from: u32) -> Ending {
    let waiting = Instant::now();
    loop {
        std::thread::sleep(POLL);
        match ending(events) {
            None => {}
            Some(Ending::HandedOver { pid }) if pid == from => {
                if waiting.elapsed() > TAKEOVER {
                    return Ending::Unexpected;
                }
            }
            Some(other) => return other,
        }
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// A job whose processes die when the supervisor's handle closes, however it exits.
    pub(super) struct Job(HANDLE);

    impl Job {
        pub(super) fn new() -> Option<Self> {
            // SAFETY: null attributes and a null name create an unnamed job with default security.
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return None;
            }
            let job = Self(handle);
            // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is a valid value.
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            // SAFETY: the pointer and size describe `info`, which outlives the call.
            let set = unsafe {
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    std::ptr::from_ref(&info).cast(),
                    u32::try_from(std::mem::size_of_val(&info)).unwrap_or(u32::MAX),
                )
            };
            (set != 0).then_some(job)
        }

        pub(super) fn adopt(&self, child: &std::process::Child) {
            // SAFETY: both handles are open for the duration of the call.
            let _ = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle came from CreateJobObjectW and is closed exactly once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(not(windows))]
mod job {
    /// Outside Windows a stopped supervisor leaves its Runtime running; stop both.
    pub(super) struct Job;

    impl Job {
        pub(super) fn new() -> Option<Self> {
            None
        }

        pub(super) fn adopt(&self, _child: &std::process::Child) {}
    }
}
