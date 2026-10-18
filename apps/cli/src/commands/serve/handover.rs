//! #1523: a Runtime moves itself onto a newly installed binary (D-059).
//!
//! `serve --handover-install <root>` watches `<root>/.graphhelm-installed-head`, the stamp
//! `graphhelm update` writes after it swaps `<root>/bin/graphhelm`. When the stamp changes, this
//! process spawns that binary with its own argv and its own environment, so the events key and
//! the gateway key reach the successor the way they reached this process: inherited in memory,
//! never written, logged or put on a command line. The successor opens the keyring and verifies
//! the journal (its whole startup but the bind), then writes `<events>.handover-ready` with its
//! pid. Only then does this process stop serving and release the port and the lifecycle lock,
//! which the successor takes over. A successor that cannot be spawned, exits, or never reports
//! ready is killed and this process keeps serving (fail closed); the same stamp is not retried.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Set on the successor only: the pid of the Runtime that spawned it. Not a secret.
const FROM_ENV: &str = "GRAPHHELM_HANDOVER_FROM";
const STAMP: &str = ".graphhelm-installed-head";
const POLL: Duration = Duration::from_secs(2);
const READY_DEADLINE: Duration = Duration::from_secs(120);
/// How long the successor waits for the port and the lifecycle lock to be released.
pub(super) const TAKEOVER_DEADLINE: Duration = Duration::from_secs(30);
/// How long the old Runtime lets open requests finish once the successor is ready.
pub(super) const DRAIN: Duration = Duration::from_secs(5);

fn ready_path(events: &Path) -> PathBuf {
    let mut name = events.file_name().unwrap_or_default().to_os_string();
    name.push(".handover-ready");
    events.with_file_name(name)
}

/// True when this process was spawned by a Runtime handing over to it.
pub(super) fn is_successor() -> bool {
    std::env::var_os(FROM_ENV).is_some()
}

/// The successor's half: everything but the bind is up, so say so.
pub(super) fn announce_ready(events: &Path) -> std::io::Result<()> {
    crate::commands::atomic::atomic_write_with(
        &ready_path(events),
        std::process::id().to_string().as_bytes(),
        crate::commands::atomic::Mode {
            private: true,
            regular_only: true,
        },
    )
}

fn read_stamp(root: &Path) -> Option<String> {
    std::fs::read_to_string(root.join(STAMP))
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

fn binary(root: &Path) -> PathBuf {
    root.join("bin").join(if cfg!(windows) {
        "graphhelm.exe"
    } else {
        "graphhelm"
    })
}

/// Watches the stamp until a successor reports ready; returns only then. Every failed attempt is
/// reported on stderr without the environment, and the old Runtime keeps serving.
pub(super) async fn watch(root: PathBuf, events: PathBuf) {
    let mut seen = read_stamp(&root);
    loop {
        tokio::time::sleep(POLL).await;
        let current = read_stamp(&root);
        if current.is_none() || current == seen {
            continue;
        }
        seen.clone_from(&current);
        let (root, events) = (root.clone(), events.clone());
        match tokio::task::spawn_blocking(move || attempt(&root, &events)).await {
            Ok(Ok(pid)) => {
                eprintln!("runtime.handover ready: successor pid {pid}; this Runtime stops");
                return;
            }
            Ok(Err(reason)) => {
                eprintln!("runtime.handover refused: {reason}; this Runtime keeps serving");
            }
            Err(_) => eprintln!("runtime.handover refused: the attempt did not finish"),
        }
    }
}

fn attempt(root: &Path, events: &Path) -> Result<u32, String> {
    let ready = ready_path(events);
    let _ = std::fs::remove_file(&ready);
    // The successor inherits this process's environment, the keys included, and its argv,
    // which carries no secret. stdin is closed; stdout and stderr are this Runtime's own.
    let mut child = graphhelm_process::command(binary(root))
        .args(std::env::args_os().skip(1))
        .env(FROM_ENV, std::process::id().to_string())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "the installed binary could not be started".to_owned())?;
    let expected = child.id().to_string();
    let started = Instant::now();
    loop {
        if std::fs::read_to_string(&ready).is_ok_and(|text| text.trim() == expected) {
            let _ = std::fs::remove_file(&ready);
            return Ok(child.id());
        }
        if let Ok(Some(status)) = child.try_wait() {
            let _ = std::fs::remove_file(&ready);
            return Err(format!(
                "the installed binary exited before it was ready ({status})"
            ));
        }
        if started.elapsed() > READY_DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&ready);
            return Err("the installed binary did not report ready in time".to_owned());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
