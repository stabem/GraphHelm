//! The build slot (#360 phase 2, #361): one cargo build at a time per workspace root, served in
//! arrival order. Each build uses its own worktree's target (`<cwd>/target`); `--shared-target`
//! keeps the old `<root>/target-shared`, unsafe for tests. With the owner's rule file
//! (`slot-targets.json`, #360) the build directory is `<targetRoot>/<lane>/<worktree>/target`
//! instead, recorded, capped per lane, and reclaimed when its worktree is gone.
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
fn workspace_packages(cargo: &str, cwd: &Path) -> Option<Vec<String>> {
    let output = Command::new(cargo)
        .current_dir(cwd)
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
fn clean_workspace_packages(
    cargo: &str,
    target: &Path,
    cwd: &Path,
) -> Result<serde_json::Value, String> {
    let directory = cwd.display();
    let packages = workspace_packages(cargo, cwd).ok_or_else(|| {
        format!("--clean-workspace: no cargo workspace at the current directory {directory}; run the slot from the worktree root")
    })?;
    let mut clean = Command::new(cargo);
    clean
        .current_dir(cwd)
        .arg("clean")
        .env("CARGO_TARGET_DIR", target);
    for package in &packages {
        clean.args(["-p", package]);
    }
    match clean.status() {
        Ok(status) if status.success() => Ok(json!({"packages": packages.len(), "ok": true})),
        Ok(status) => Err(format!(
            "--clean-workspace: cargo clean failed ({status}) in {directory}"
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

/// The slot is a Cargo build resource, not a general purpose process queue. Keep this check pure
/// and before target discovery, ticket creation, or any filesystem effect. Callers that need a
/// script must split it into direct, reached steps and acquire the slot for each Cargo step.
fn admit_command(command: &[String]) -> Result<(), String> {
    let Some(program) = command.first() else {
        return Err("a direct Cargo command is required after --".into());
    };
    let basename = program.rsplit(['/', '\\']).next().unwrap_or(program);
    if !matches!(basename, "cargo" | "cargo.exe") {
        return Err("workspace slot accepts direct cargo commands only; run the outer script outside the slot".into());
    }
    let mut cursor = 1;
    if command
        .get(cursor)
        .is_some_and(|arg| arg.starts_with('+') && arg.len() > 1)
    {
        cursor += 1;
    }
    let Some(kind @ ("build" | "test" | "clippy")) = command.get(cursor).map(String::as_str) else {
        return Err("workspace slot accepts cargo build, test, or clippy only".into());
    };
    cursor += 1;
    let mut package = false;
    let mut target = false;
    let mut filter = false;
    while let Some(raw) = command.get(cursor) {
        cursor += 1;
        if raw == "--" {
            // Cargo stops parsing here. Remaining arguments belong to libtest or clippy.
            break;
        }
        let (flag, inline) = if let Some((flag, value)) = raw.split_once('=') {
            (flag, Some(value))
        } else if raw.starts_with("-p") && raw.len() > 2 {
            ("-p", Some(&raw[2..]))
        } else if raw.starts_with("-j") && raw.len() > 2 {
            ("-j", Some(&raw[2..]))
        } else if raw.starts_with("-F") && raw.len() > 2 {
            ("-F", Some(&raw[2..]))
        } else {
            (raw.as_str(), None)
        };
        match flag {
            "--workspace" | "--all" | "--target-dir" | "--manifest-path" | "--config"
            | "--exclude" | "-j" | "--jobs" | "--tests" | "--bins" | "--examples" | "--benches" => {
                return Err("workspace-wide commands and target overrides cannot occupy the ordinary slot; name each package and test target".into());
            }
            "--lib" | "--doc" if inline.is_none() => target = true,
            "--all-targets" if kind == "clippy" && inline.is_none() => {}
            "--locked"
            | "--offline"
            | "--frozen"
            | "--release"
            | "-r"
            | "--all-features"
            | "--no-default-features"
            | "--quiet"
            | "-q"
            | "--verbose"
            | "-v"
            | "-vv"
            | "--no-run"
            | "--keep-going"
                if inline.is_none() => {}
            "--timings" => {}
            "-p" | "--package" | "--test" | "--bin" | "--example" | "--bench" | "-F"
            | "--features" | "--target" | "--profile" | "--message-format" | "--color" => {
                let value = if let Some(value) = inline {
                    value
                } else {
                    let value = command
                        .get(cursor)
                        .ok_or("Cargo option is missing its value")?;
                    cursor += 1;
                    value.as_str()
                };
                if value.is_empty() || value.starts_with('-') {
                    return Err("Cargo option is missing its value".into());
                }
                if matches!(
                    flag,
                    "-p" | "--package" | "--test" | "--bin" | "--example" | "--bench"
                ) && value.contains(['*', '?', '['])
                {
                    return Err("package and target globs are not bounded slot commands".into());
                }
                if matches!(flag, "-p" | "--package") {
                    package = true;
                } else if matches!(flag, "--test" | "--bin" | "--example" | "--bench") {
                    target = true;
                }
            }
            _ if !raw.starts_with('-') && kind == "test" && !filter => filter = true,
            _ => {
                return Err(format!(
                    "unsupported Cargo slot option {raw}; use direct scoped Cargo commands"
                ));
            }
        }
    }
    if !package {
        return Err("workspace slot requires an explicit package".into());
    }
    if kind == "test" && !target {
        return Err("workspace slot test requires an explicit test target".into());
    }
    Ok(())
}

#[cfg(test)]
mod admission_tests {
    use super::admit_command;

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn admits_one_explicit_cargo_test_target() {
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "--locked",
                "-p",
                "graphhelm-cli",
                "--test",
                "workspace_cli",
            ]))
            .is_ok()
        );
    }

    #[test]
    fn refuses_script_before_any_slot_effect() {
        let error = admit_command(&argv(&["powershell", "-Command", "marker"])).unwrap_err();
        assert!(error.contains("direct cargo"));
    }

    #[test]
    fn refuses_unscoped_and_target_overrides() {
        assert!(admit_command(&argv(&["cargo", "+1.97.1", "test", "--workspace"])).is_err());
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "-p",
                "graphhelm-cli",
                "--target-dir",
                "else",
                "--test",
                "cli",
            ]))
            .is_err()
        );
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "-p",
                "graphhelm-cli",
                "--test",
            ]))
            .is_err()
        );
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "--test",
                "workspace_cli"
            ]))
            .is_err()
        );
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "-pfoo",
                "--test",
                "workspace_cli",
            ]))
            .is_ok()
        );
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "--package=foo",
                "--test",
                "workspace_cli",
            ]))
            .is_ok()
        );
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "-p",
                "foo",
                "--test",
                "workspace_cli",
                "--",
                "--test-threads=2",
            ]))
            .is_ok()
        );
        assert!(
            admit_command(&argv(&[
                "cargo",
                "+1.97.1",
                "test",
                "-p",
                "foo",
                "--test",
                "workspace_cli",
                "--config",
                "target-dir=bad",
            ]))
            .is_err()
        );
    }
    #[test]
    fn parses_scope_without_treating_option_values_as_filters() {
        for args in [
            vec![
                "C:/toolchain/bin/cargo.exe",
                "test",
                "-pfoo",
                "--test=workspace_cli",
            ],
            vec!["cargo", "test", "-p", "foo", "--lib"],
            vec!["cargo", "test", "-p", "foo", "--doc"],
            vec![
                "cargo",
                "clippy",
                "-pfoo",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ] {
            assert!(admit_command(&argv(&args)).is_ok(), "{args:?}");
        }
        for args in [
            vec!["cargo", "test", "-pfoo", "--features", "feature"],
            vec!["cargo", "test", "-pfoo", "filter_only"],
            vec!["cargo", "test", "-pfoo", "--all-targets"],
            vec!["cargo", "test", "-pfoo", "--test=*"],
            vec!["cargo", "test", "-pfoo", "--test", "--lib"],
            vec!["cargo", "test", "-pfoo", "--lib", "-j32"],
        ] {
            assert!(admit_command(&argv(&args)).is_err(), "{args:?}");
        }
    }
}

#[cfg(test)]
#[path = "workspace_slot_target_tests.rs"]
mod workspace_slot_target_tests;
#[cfg(test)]
#[path = "workspace_slot_tests.rs"]
mod workspace_slot_tests;

pub(crate) fn run_slot(request: &SlotRequest<'_>) -> Outcome {
    if let Err(message) = admit_command(request.command) {
        return refuse(&message, "/command");
    }
    run_admitted_slot(request)
}

fn run_admitted_slot(request: &SlotRequest<'_>) -> Outcome {
    let Ok(cwd) = std::env::current_dir() else {
        return refuse("the current directory could not be read", "/target");
    };
    run_admitted_slot_in(request, &cwd)
}

fn run_admitted_slot_in(request: &SlotRequest<'_>, cwd: &Path) -> Outcome {
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

    // #360: the owner's build-directory rule. A lane over its cap or below the floor refuses before it
    // queues, so nobody waits for a turn the slot would then refuse.
    let rule = if request.shared {
        None
    } else {
        match super::workspace::target_rule(root) {
            Ok(rule) => rule,
            Err(message) => return refuse(&message, "/targetRoot"),
        }
    };
    let worktree = match &rule {
        None => None,
        Some(_) => Some(cwd.to_owned()),
    };
    if let (Some(rule), Some(worktree)) = (&rule, &worktree)
        && let Err(message) = super::workspace::slot_target(root, rule, lane, worktree, false)
    {
        return refuse(&message, "/target");
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
        "arrivedNanos": arrival.to_string(), "priority": request.priority,
        "worktree": cwd});
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
            "sinceNanos": held_since.to_string(), "worktree": cwd, "ticket": mine.file_name().map(|n| n.to_string_lossy().into_owned())})
        .to_string(),
    );
    let release = |slot: File, ticket: File| {
        let _ = std::fs::remove_file(dir.join(HOLDER));
        drop(slot);
        drop(ticket);
        remove_ticket(&mine);
    };
    let mut reclaimed = Vec::new();
    let target = if request.shared {
        shared_target(root)
    } else if let (Some(rule), Some(worktree)) = (&rule, &worktree) {
        // Holding the slot: reclaim, recheck the free-space floor and cap, then record.
        match super::workspace::slot_target(root, rule, lane, worktree, true) {
            Ok((target, gone)) => {
                reclaimed = gone;
                target
            }
            Err(message) => {
                release(slot, ticket);
                return refuse(&message, "/target");
            }
        }
    } else {
        cwd.join("target")
    };
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let mut cleaned = None;
    if request.clean_workspace {
        match clean_workspace_packages(&cargo, &target, cwd) {
            Ok(report) => cleaned = Some(report),
            Err(reason) => {
                release(slot, ticket);
                return refuse(&reason, "/cleanWorkspace");
            }
        }
    }
    let status = Command::new(program)
        .current_dir(cwd)
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
            "priority": request.priority, "reclaimedTargets": reclaimed}),
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
    let (targets, target_space) = super::workspace::target_counts(root);
    if !dir.is_dir() {
        return Outcome::success(
            STATUS_COMMAND,
            json!({"holder": null, "waiting": [], "targets": targets, "targetSpace": target_space}),
        );
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
            "worktree": info["worktree"], "heldSeconds": seconds_since(nanos_of(&info["sinceNanos"]), now)})
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
                "worktree": info["worktree"], "priority": info["priority"].as_bool().unwrap_or(false),
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
        json!({"holder": holder, "waiting": waiting, "targets": targets, "targetSpace": target_space}),
    )
}
