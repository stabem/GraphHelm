//! The build slot (#360 phase 2, #361): one cargo build at a time per workspace root, served in
//! arrival order. Each build uses its own worktree's target (`<cwd>/target`); `--shared-target`
//! keeps the old `<root>/target-shared`, unsafe for tests.
//!
//! Each waiter creates a ticket file under `<root>/.graphhelm-workspaces/slot/` and holds an
//! exclusive OS lock on it for as long as it lives. A ticket whose lock can be taken belongs to a
//! process that has gone (the OS releases a dead process's locks), so it is removed; no process id
//! is read and no process table is asked, which is what lost a live waiter's place in the
//! script this replaces. The oldest live ticket takes `slot.lock`, also held by an OS lock, runs the
//! command with its `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS`, and releases both.
//!
//! #540: a locked file cannot be read on Windows, so each ticket has an unlocked `.info` beside it
//! (lane, label, arrival) and the holder writes `holder.json`; `slot status` reads those, and asks
//! the locks, not the files, whether anyone is alive. A waiter waits without limit unless it asks
//! (`--max-wait`); one that gives up leaves a `.resume` reservation, so the same lane and label
//! queueing again within `RESUME_GRACE` are served at their first arrival. `--priority` tickets
//! sort before every other waiter (`!` precedes the digits of an arrival key) and are only for
//! lanes the owner listed in `slot-priority-lanes`.
use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use graphhelm_protocols::Diagnostic;
use serde_json::json;

use crate::output::Outcome;

const COMMAND: &str = "workspace.slot";
const STATUS_COMMAND: &str = "workspace.slot.status";
const POLL: Duration = Duration::from_millis(500);
/// How long a waiter that gave up keeps its place for the same lane and label (#540).
const RESUME_GRACE: Duration = Duration::from_secs(30 * 60);
/// The owner's list of lanes allowed `--priority`, one per line, under the ledger directory.
const PRIORITY_LANES: &str = "slot-priority-lanes";
const HOLDER: &str = "holder.json";
/// Prefix of a priority ticket: sorts before the digits of every normal arrival key.
const PRIORITY_MARK: &str = "!";

fn refuse(message: &str, path: &str) -> Outcome {
    refuse_as(COMMAND, message, path)
}

fn refuse_as(command: &'static str, message: &str, path: &str) -> Outcome {
    Outcome::application(
        command,
        Diagnostic::error(
            crate::error_codes::GHCLI037_WORKSPACE_REFUSED,
            message,
            path,
            "graphhelm",
        ),
    )
}

/// clap requires `--root` and `--lane` without a subcommand; kept for the exhaustive match.
pub(crate) fn refuse_args() -> Outcome {
    refuse("--root and --lane are required to run a command", "/lane")
}

/// The opt-in shared target directory (`--shared-target`).
pub(crate) fn shared_target(root: &Path) -> PathBuf {
    root.join("target-shared")
}

fn slot_dir(root: &Path) -> PathBuf {
    root.join(super::workspace::LEDGER).join("slot")
}

fn unix_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}

/// The unlocked description beside a ticket (`<ticket>.info`).
fn info_path(ticket: &Path) -> PathBuf {
    ticket.with_extension("info")
}

fn remove_ticket(ticket: &Path) {
    let _ = std::fs::remove_file(ticket);
    let _ = std::fs::remove_file(info_path(ticket));
}

/// Live tickets in serving order (priority first, then arrival); a ticket whose lock can be taken
/// is a dead waiter's and is removed with its `.info`. Our own ticket is never probed (we hold
/// its lock).
fn live_tickets(dir: &Path, mine: Option<&Path>) -> std::io::Result<Vec<PathBuf>> {
    let mut tickets: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ticket"))
        .collect();
    tickets.sort();
    let mut live = Vec::new();
    for ticket in tickets {
        if Some(ticket.as_path()) == mine {
            live.push(ticket);
            continue;
        }
        let Ok(file) = File::open(&ticket) else {
            continue;
        };
        match file.try_lock() {
            Ok(()) => {
                drop(file);
                remove_ticket(&ticket);
            }
            Err(TryLockError::WouldBlock) => live.push(ticket),
            Err(TryLockError::Error(_)) => live.push(ticket),
        }
    }
    Ok(live)
}

/// A file-name-safe form of a lane and label, for the reservation's name.
fn reservation_path(dir: &Path, lane: &str, label: &str) -> PathBuf {
    let safe: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir.join(format!("{lane}--{safe}.resume"))
}

/// The arrival key a reservation keeps, when it exists and has not expired; it is consumed.
fn take_reservation(path: &Path, now: u128) -> Option<u128> {
    let text = std::fs::read_to_string(path).ok()?;
    let _ = std::fs::remove_file(path);
    let mut parts = text.split_whitespace();
    let arrival: u128 = parts.next()?.parse().ok()?;
    let expires: u128 = parts.next()?.parse().ok()?;
    (now <= expires).then_some(arrival)
}

/// Whether the owner listed `lane` for `--priority`.
fn priority_allowed(root: &Path, lane: &str) -> bool {
    std::fs::read_to_string(root.join(super::workspace::LEDGER).join(PRIORITY_LANES))
        .is_ok_and(|text| text.lines().any(|line| line.trim() == lane))
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

/// One `workspace slot` run, as the command line gave it.
pub(crate) struct SlotRequest<'a> {
    pub root: &'a Path,
    pub lane: &'a str,
    pub label: &'a str,
    pub jobs: u32,
    pub clean_workspace: bool,
    pub shared: bool,
    /// Minutes; `None` waits as long as it takes (#540).
    pub max_wait: Option<f64>,
    pub priority: bool,
    pub command: &'a [String],
}

pub(crate) fn run_slot(request: &SlotRequest<'_>) -> Outcome {
    let (root, lane, label) = (request.root, request.lane, request.label);
    if !super::workspace::valid_id(lane) {
        return refuse("lane must be a workspace id", "/lane");
    }
    let Some((program, arguments)) = request.command.split_first() else {
        return refuse("a command to run is required after --", "/command");
    };
    let max_wait = match request.max_wait {
        None => None,
        // `try_from`: a finite but huge number (`1e300`) overflows a Duration, and the plain
        // constructor panics on it (#557 review); it is refused like any other bad value.
        Some(minutes) if minutes.is_finite() && minutes > 0.0 => {
            match Duration::try_from_secs_f64(minutes * 60.0) {
                Ok(limit) => Some(limit),
                Err(_) => {
                    return refuse(
                        "--max-wait must be a positive number of minutes",
                        "/maxWait",
                    );
                }
            }
        }
        Some(_) => {
            return refuse(
                "--max-wait must be a positive number of minutes",
                "/maxWait",
            );
        }
    };
    if request.priority && !priority_allowed(root, lane) {
        return refuse(
            &format!(
                "--priority is only for lanes the owner listed in {}/{PRIORITY_LANES}; {lane} is not listed",
                super::workspace::LEDGER
            ),
            "/priority",
        );
    }
    let dir = slot_dir(root);
    if std::fs::create_dir_all(&dir).is_err() {
        return refuse("the slot directory could not be created", "/root");
    }
    let now = unix_now();
    let reservation = reservation_path(&dir, lane, label);
    let arrival = take_reservation(&reservation, now).unwrap_or(now);
    let mark = if request.priority { PRIORITY_MARK } else { "" };
    let mine = dir.join(format!(
        "{mark}{arrival:024}-{lane}-{}.ticket",
        std::process::id()
    ));
    let Ok(ticket) = OpenOptions::new().write(true).create_new(true).open(&mine) else {
        return refuse("the slot ticket could not be created", "/root");
    };
    if ticket.lock().is_err() {
        remove_ticket(&mine);
        return refuse("the slot ticket could not be locked", "/root");
    }
    let info = json!({"lane": lane, "label": label, "pid": std::process::id(),
        "arrivedNanos": arrival.to_string(), "priority": request.priority});
    let _ = std::fs::write(info_path(&mine), info.to_string());
    let started = Instant::now();
    let slot = loop {
        if let Some(limit) = max_wait
            && started.elapsed() > limit
        {
            drop(ticket);
            remove_ticket(&mine);
            let expires = unix_now() + RESUME_GRACE.as_nanos();
            let _ = std::fs::write(&reservation, format!("{arrival} {expires}"));
            return refuse(
                &format!(
                    "not served within --max-wait; this lane and label keep their place for {} minutes if they queue again",
                    RESUME_GRACE.as_secs() / 60
                ),
                "/slot",
            );
        }
        let first = live_tickets(&dir, Some(&mine))
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
    let held_since = unix_now();
    let _ = std::fs::write(
        dir.join(HOLDER),
        json!({"lane": lane, "label": label, "pid": std::process::id(),
            "sinceNanos": held_since.to_string(), "ticket": mine.file_name().map(|n| n.to_string_lossy().into_owned())})
        .to_string(),
    );
    let release = |slot: File, ticket: File| {
        let _ = std::fs::remove_file(dir.join(HOLDER));
        drop(slot);
        drop(ticket);
        remove_ticket(&mine);
    };
    let target = if request.shared {
        shared_target(root)
    } else {
        match std::env::current_dir() {
            Ok(dir) => dir.join("target"),
            Err(_) => {
                release(slot, ticket);
                return refuse("the current directory could not be read", "/target");
            }
        }
    };
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cleaned = None;
    if request.clean_workspace {
        match clean_workspace_packages(&cargo, &target) {
            Ok(report) => cleaned = Some(report),
            Err(reason) => {
                release(slot, ticket);
                return refuse(&reason, "/cleanWorkspace");
            }
        }
    }
    let status = Command::new(program)
        .args(arguments)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_JOBS", request.jobs.to_string())
        .status();
    let held = started.elapsed().as_secs() - waited;
    release(slot, ticket);
    let Ok(status) = status else {
        return refuse("the command could not be started", "/command");
    };
    let code = status.code().unwrap_or(1);
    let mut outcome = Outcome::success(
        COMMAND,
        json!({"lane": lane, "label": label, "exitCode": code, "waitedSeconds": waited,
            "heldSeconds": held, "targetDir": target.to_string_lossy(), "cleaned": cleaned,
            "priority": request.priority}),
    );
    outcome.exit_code = code;
    outcome
}

fn seconds_since(nanos: Option<u128>, now: u128) -> Option<u64> {
    nanos.map(|at| u64::try_from(now.saturating_sub(at) / 1_000_000_000).unwrap_or(u64::MAX))
}

fn nanos_of(value: &serde_json::Value) -> Option<u128> {
    value.as_str().and_then(|text| text.parse().ok())
}

/// `workspace slot status` (#540): the holder, if a live process holds `slot.lock`, and the live
/// waiters in serving order with their wait since first arrival. Liveness comes from the locks;
/// the `.info` and `holder.json` files only describe.
pub(crate) fn run_status(root: &Path) -> Outcome {
    let dir = slot_dir(root);
    if !dir.is_dir() {
        return Outcome::success(STATUS_COMMAND, json!({"holder": null, "waiting": []}));
    }
    let now = unix_now();
    let held = match OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join("slot.lock"))
    {
        Ok(lock) => match lock.try_lock() {
            Ok(()) => false, // nobody holds it; our lock drops with `lock`
            Err(_) => true,
        },
        Err(_) => false,
    };
    let Ok(live) = live_tickets(&dir, None) else {
        return refuse_as(
            STATUS_COMMAND,
            "the slot directory could not be read",
            "/root",
        );
    };
    let names: Vec<String> = live
        .iter()
        .filter_map(|ticket| ticket.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    // `holder.json` is trusted only while the ticket it names is live (#557 review): a holder
    // killed hard leaves the file behind, and a next holder on an older binary never rewrites it,
    // so a stale file would name a dead lane and list the real holder as a waiter.
    let holder_info = held
        .then(|| std::fs::read_to_string(dir.join(HOLDER)).ok())
        .flatten()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(|info| {
            info["ticket"]
                .as_str()
                .is_some_and(|t| names.iter().any(|n| n == t))
        });
    let holder_ticket = holder_info
        .as_ref()
        .and_then(|info| info["ticket"].as_str().map(str::to_owned));
    let holder = holder_info.as_ref().map(|info| {
        json!({"lane": info["lane"], "label": info["label"], "pid": info["pid"],
            "heldSeconds": seconds_since(nanos_of(&info["sinceNanos"]), now)})
    });
    let waiting: Vec<serde_json::Value> = live
        .iter()
        .filter(|ticket| {
            ticket.file_name().map(|n| n.to_string_lossy().into_owned()) != holder_ticket
        })
        .map(|ticket| {
            let info = std::fs::read_to_string(info_path(ticket))
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .unwrap_or(serde_json::Value::Null);
            json!({"lane": info["lane"], "label": info["label"], "pid": info["pid"],
                "priority": info["priority"].as_bool().unwrap_or(false),
                "waitedSeconds": seconds_since(nanos_of(&info["arrivedNanos"]), now),
                "ticket": ticket.file_name().map(|n| n.to_string_lossy().into_owned())})
        })
        .collect();
    let holder = holder.unwrap_or(if held {
        json!({"lane": null, "label": null, "pid": null, "heldSeconds": null})
    } else {
        serde_json::Value::Null
    });
    Outcome::success(
        STATUS_COMMAND,
        json!({"holder": holder, "waiting": waiting}),
    )
}
