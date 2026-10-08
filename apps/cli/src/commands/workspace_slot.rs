//! The shared build slot (#360 phase 2): one cargo build at a time per workspace root, served in
//! arrival order, every build sharing one warm target directory.
//!
//! Each waiter creates a ticket file under `<root>/.graphhelm-workspaces/slot/` and holds an
//! exclusive OS lock on it for as long as it lives. A ticket whose lock can be taken belongs to a
//! process that has gone (the OS releases a dead process's locks), so it is removed; no process id
//! is read and no process table is asked, which is what lost a live waiter's place in the
//! script this replaces. The oldest live ticket takes `slot.lock`, also held by an OS lock, runs the
//! command with `CARGO_TARGET_DIR=<root>/target-shared` and `CARGO_BUILD_JOBS`, and releases both.
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use graphhelm_protocols::Diagnostic;
use serde_json::json;

use crate::output::Outcome;

const COMMAND: &str = "workspace.slot";
/// How long a waiter may queue before it gives up (the script's bound).
const MAX_WAIT: Duration = Duration::from_secs(120 * 60);
const POLL: Duration = Duration::from_millis(500);

fn refuse(message: &str, path: &str) -> Outcome {
    Outcome::application(
        COMMAND,
        Diagnostic::error(
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            message,
            path,
            "graphhelm",
        ),
    )
}

/// The shared target directory every slotted build of `root` uses.
pub(crate) fn shared_target(root: &Path) -> PathBuf {
    root.join("target-shared")
}

/// Live tickets in arrival order; a ticket whose lock can be taken is a dead waiter's and is
/// removed. Our own ticket is never probed (we hold its lock).
fn live_tickets(dir: &Path, mine: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut tickets: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ticket"))
        .collect();
    tickets.sort();
    let mut live = Vec::new();
    for ticket in tickets {
        if ticket == mine {
            live.push(ticket);
            continue;
        }
        let Ok(file) = File::open(&ticket) else {
            continue;
        };
        match file.try_lock() {
            Ok(()) => {
                drop(file);
                let _ = std::fs::remove_file(&ticket);
            }
            Err(TryLockError::WouldBlock) => live.push(ticket),
            Err(TryLockError::Error(_)) => live.push(ticket),
        }
    }
    Ok(live)
}

/// The workspace's own package names, for `--clean-workspace` (`cargo metadata --no-deps`, run
/// in the current directory). `None` when the directory is no cargo workspace or cargo fails.
fn workspace_packages(cargo: &str) -> Option<Vec<String>> {
    let output = Command::new(cargo)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let packages: Vec<String> = value["packages"]
        .as_array()?
        .iter()
        .filter_map(|package| package["name"].as_str().map(str::to_owned))
        .collect();
    (!packages.is_empty()).then_some(packages)
}

/// `--clean-workspace` before the command: clean every workspace package out of the shared
/// target. Fails closed (#417 review): a clean that cannot run or fails is a refusal, never a
/// silent `cleaned: null` followed by a build against another lane's artifacts.
fn clean_workspace_packages(cargo: &str, target: &Path) -> Result<serde_json::Value, String> {
    let cwd = std::env::current_dir()
        .map_or_else(|_| "<unknown>".to_owned(), |dir| dir.display().to_string());
    let packages = workspace_packages(cargo).ok_or_else(|| {
        format!("--clean-workspace: no cargo workspace at the current directory {cwd}; run the slot from the worktree root")
    })?;
    let mut clean = Command::new(cargo);
    clean.arg("clean").env("CARGO_TARGET_DIR", target);
    for package in &packages {
        clean.args(["-p", package]);
    }
    match clean.status() {
        Ok(status) if status.success() => Ok(json!({"packages": packages.len(), "ok": true})),
        Ok(status) => Err(format!(
            "--clean-workspace: cargo clean failed ({status}) in {cwd}"
        )),
        Err(error) => Err(format!(
            "--clean-workspace: cargo clean could not start: {}",
            error.kind()
        )),
    }
}

pub(crate) fn run_slot(
    root: &Path,
    lane: &str,
    label: &str,
    jobs: u32,
    clean_workspace: bool,
    command: &[String],
) -> Outcome {
    if !super::workspace::valid_id(lane) {
        return refuse("lane must be a workspace id", "/lane");
    }
    let Some((program, arguments)) = command.split_first() else {
        return refuse("a command to run is required after --", "/command");
    };
    let dir = root.join(super::workspace::LEDGER).join("slot");
    if std::fs::create_dir_all(&dir).is_err() {
        return refuse("the slot directory could not be created", "/root");
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mine = dir.join(format!("{nanos:024}-{lane}-{}.ticket", std::process::id()));
    let Ok(ticket) = OpenOptions::new().write(true).create_new(true).open(&mine) else {
        return refuse("the slot ticket could not be created", "/root");
    };
    if ticket.lock().is_err() {
        let _ = std::fs::remove_file(&mine);
        return refuse("the slot ticket could not be locked", "/root");
    }
    let started = Instant::now();
    let slot = loop {
        if started.elapsed() > MAX_WAIT {
            drop(ticket);
            let _ = std::fs::remove_file(&mine);
            return refuse("not served within 120 minutes", "/slot");
        }
        let first = live_tickets(&dir, &mine)
            .ok()
            .and_then(|live| live.first().cloned());
        if first.as_deref() == Some(mine.as_path())
            && let Ok(slot) = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(dir.join("slot.lock"))
            && slot.try_lock().is_ok()
        {
            break slot;
        }
        std::thread::sleep(POLL);
    };
    let waited = started.elapsed().as_secs();
    let mut holder = &slot;
    let _ = holder.set_len(0);
    let _ = writeln!(holder, "{lane} {label} pid={}", std::process::id());
    let target = shared_target(root);
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cleaned = None;
    if clean_workspace {
        match clean_workspace_packages(&cargo, &target) {
            Ok(report) => cleaned = Some(report),
            Err(reason) => {
                drop(slot);
                drop(ticket);
                let _ = std::fs::remove_file(&mine);
                return refuse(&reason, "/cleanWorkspace");
            }
        }
    }
    let status = Command::new(program)
        .args(arguments)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_JOBS", jobs.to_string())
        .status();
    let held = started.elapsed().as_secs() - waited;
    drop(slot);
    drop(ticket);
    let _ = std::fs::remove_file(&mine);
    let Ok(status) = status else {
        return refuse("the command could not be started", "/command");
    };
    let code = status.code().unwrap_or(1);
    let mut outcome = Outcome::success(
        COMMAND,
        json!({"lane": lane, "label": label, "exitCode": code, "waitedSeconds": waited,
            "heldSeconds": held, "targetDir": target.to_string_lossy(), "cleaned": cleaned}),
    );
    outcome.exit_code = code;
    outcome
}
