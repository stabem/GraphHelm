//! `journey open` / `journey act` / `journey close` (#398, journey-first spec §5): an approved
//! flow opened live at one step. The walk replays the replay cache's resolved locators of every
//! edge before the step in a visible browser, with no model, through the same supervisor pieces
//! and driver as `journey replay` (`journey_replay.rs`). The browser then stays open, addressed
//! by a session id, until `journey close` or the replay run budget ends.
//!
//! Shape: `journey open` spawns this binary as a long-lived session host (`--live-host`, with a
//! start handshake like the replay worker). The host walks, records the step's `phase: live`
//! capture when recording is requested, prints ONE envelope line, then serves loopback requests
//! authenticated by a per-session secret file. `act`/`close` are short clients of that host.
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use graphhelm_protocols::Diagnostic;
use serde_json::{Value, json};

use super::journey_replay::{
    Driver, Failure, OP_BUDGET, RUN_BUDGET, Result, SURVIVABLE, TemporaryOutput, failure,
    load_cache, observe, observer_ready, preflight, record, safe_directory, safe_environment,
    safe_node, url_matches,
};
use crate::args::{
    JourneyActArgs, JourneyCloseArgs, JourneyOpenArgs, JourneyReplayArgs, JourneySessionsArgs,
    JourneyWatchArgs,
};
use crate::output::{CommandOutput, Outcome};

const OPEN: &str = "journey.open";
const ACT: &str = "journey.act";
const CLOSE: &str = "journey.close";
const WATCH: &str = "journey.watch";
/// The slowest pace a watch accepts; slower would not finish inside the run budget.
const MAX_PACE_MS: u64 = 10_000;
const HANDSHAKE: &[u8] = b"{\"protocol\":\"graphhelm-live-host/1\",\"start\":true}\n";
const SESSIONS: &str = ".graphhelm/journey-sessions";
const FRAME: usize = 64 * 1024;
/// Accessible names an act on a live session may not target unless the approved flow contains
/// that act on an edge leaving the current screen (journey-explore design §11).
const DESTRUCTIVE: [&str; 6] = ["delete", "remove", "pay", "purchase", "transfer", "send"];
/// The deny-list word a watch will not act on for a DRAFT's act (#515), if any. A draft is written
/// by agents and approved by nobody, so watching it must not delete, pay or send. Kinds that
/// commit nothing (typing into a field, waiting, inspecting) are never guarded: `wait_for "Order
/// sent"` only looks. `drop` is matched as a whole word, because as a substring it is every
/// "Dropdown".
pub(crate) fn would_destroy(act: &Value) -> Option<&'static str> {
    if matches!(
        act["kind"].as_str(),
        Some("enter_text" | "wait_for" | "inspect")
    ) {
        return None;
    }
    let lower = act["name"].as_str().unwrap_or_default().to_lowercase();
    DESTRUCTIVE
        .iter()
        .copied()
        .find(|word| lower.contains(word))
        .or_else(|| {
            lower
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word == "drop")
                .then_some("drop")
        })
}

const ACT_KINDS: [&str; 6] = [
    "activate",
    "submit",
    "enter_text",
    "navigate",
    "wait_for",
    "inspect",
];

fn report(command: &'static str, data: Value, failed: Option<Failure>) -> Outcome {
    let Some((code, path, exit_code)) = failed else {
        return Outcome::success(command, data);
    };
    let message = match code {
        "replay.observer_missing" | "driver.observer_missing" => {
            "OBSERVER_MISSING: run setup --install-observer playwright in this project, then retry with a runnable Node/Playwright/Chromium observer"
        }
        "replay.cache_missing" => {
            "open needs the replay cache of this approved flow; run `graphhelm journey replay <id>` first"
        }
        "live.host_exited" => {
            "the session host exited before reporting; data.hostExitCode is its exit status"
        }
        "live.step_unreachable" => "no path of this flow (or the named path) reaches the step",
        "live.session_gone" => {
            "no live session answers; it closed or its run budget ended. Open it again"
        }
        "live.act_refused_destructive" => {
            "a destructive-looking act is refused on a live session unless the approved flow has it on an edge leaving the current screen"
        }
        "watch.app_down" => {
            "the app under test does not answer on the flow's base, and the project declares no launcher (.graphhelm/journey-fixture.json); start it, then watch again"
        }
        "watch.launcher_invalid" => {
            "the project's launcher (.graphhelm/journey-fixture.json) is not a graphhelm-journey-fixture/1 file naming a script inside the project"
        }
        "watch.launch_failed" => {
            "the project's launcher did not bring the app under test up on the flow's base in time"
        }
        "watch.pace_invalid" => "paceMs must be between 0 and 10000",
        "watch.pace_too_slow" => {
            "this path's acts at this pace would wait longer than half the run budget; choose a shorter paceMs"
        }
        "watch.budget_exceeded" => {
            "the play reached the run budget before its last act; the app it launched has been stopped"
        }
        "watch.path_unknown" => "the flow has no path with that name",
        "watch.act_skipped_destructive" => {
            "this draft's next act looks destructive, so the watch did not perform it and stopped there (data.skipped); it is played only on the app the project's launcher starts, or once the flow is approved"
        }
        "live.recording_incomplete" => {
            "supply all of --events, --execution, --keyring and --key-id, or none"
        }
        c if c.starts_with("drift.") => {
            "the walk stopped at the broken edge; the browser stays open there"
        }
        "driver.expectation_failed" => {
            "the step was reached but its expectations do not hold; the browser stays open"
        }
        _ => "the live session refused or could not observe this request",
    };
    Outcome {
        output: CommandOutput {
            ok: false,
            command,
            data: Some(data),
            diagnostics: vec![Diagnostic::error(code, message, path, "graphhelm")],
        },
        exit_code,
    }
}

fn valid_session(id: &str) -> bool {
    id.len() <= 64
        && id.starts_with("live-")
        && id[5..]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && id.len() > 5
}

fn equal(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn project_of(project: Option<&Path>) -> Result<PathBuf> {
    project
        .unwrap_or(Path::new("."))
        .canonicalize()
        .map_err(|_| failure("replay.project_invalid", "/project", 3))
}

fn replay_args(args: &JourneyOpenArgs, project: &Path) -> JourneyReplayArgs {
    JourneyReplayArgs {
        id: args.id.clone(),
        project: Some(project.to_path_buf()),
        events: args.events.clone(),
        execution: args.execution.clone(),
        keyring: args.keyring.clone(),
        key_id: args.key_id.clone(),
        allow_origin: args.allow_origin.clone(),
        replay_worker: false,
        heal: false,
        model: Default::default(),
        allow_act: Vec::new(),
    }
}

/// The open arguments a `journey watch` plays with: no step (the path's last screen), no
/// recording, paced.
pub(crate) fn watch_args(args: &JourneyWatchArgs) -> JourneyOpenArgs {
    JourneyOpenArgs {
        id: args.id.clone(),
        step: String::new(),
        path: args.path.clone(),
        project: args.project.clone(),
        events: None,
        execution: None,
        keyring: None,
        key_id: None,
        allow_origin: args.allow_origin.clone(),
        live_host: false,
        watch: true,
        pace_ms: args.pace_ms,
        window: args.window,
    }
}

/// `journey watch` from the CLI: play the flow in a visible browser (see `JourneyCommand::Watch`).
pub(super) fn watch(args: &JourneyWatchArgs) -> Outcome {
    open_with(&watch_args(args))
}

/// `journey open` from the CLI. The Runtime's route calls [`open_in_runtime`] instead.
pub(super) fn open(args: &JourneyOpenArgs) -> Outcome {
    open_with(args)
}

/// The Runtime's `POST /v1/journeys/{contractId}/open` (#416 review): runs exactly the CLI
/// `journey open` as a child with its own explicit pipes, so the session host is spawned by a
/// process whose std handles are those pipes, never by the Runtime itself. A host spawned
/// straight from a Runtime started under MSYS `nohup` inherited that Runtime's std handles and
/// died before its first line; the CLI path never did. The Runtime's own handles are untouched.
pub(super) fn open_in_runtime(args: &JourneyOpenArgs) -> Outcome {
    let data = json!({"flowId":args.id,"step":args.step,"sessionId":null});
    let Ok(executable) = std::env::current_exe() else {
        return report(OPEN, data, Some(failure("live.host_invalid", "/host", 3)));
    };
    let mut command = Command::new(executable);
    if args.watch {
        command
            .args(["--json", "journey", "watch", &args.id])
            .args(["--pace-ms", &args.pace_ms.to_string()]);
    } else {
        command.args(["--json", "journey", "open", &args.id, "--step", &args.step]);
    }
    command
        .arg("--project")
        .arg(args.project.as_deref().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = &args.path {
        command.args(["--path", path]);
    }
    for origin in &args.allow_origin {
        command.args(["--allow-origin", origin]);
    }
    if let (Some(events), Some(execution), Some(keyring), Some(key_id)) =
        (&args.events, &args.execution, &args.keyring, &args.key_id)
    {
        command
            .arg("--events")
            .arg(events)
            .args(["--execution", execution])
            .arg("--keyring")
            .arg(keyring)
            .args(["--key-id", key_id]);
    }
    safe_environment(&mut command, args.events.is_some());
    for (key, value) in std::env::vars_os()
        .filter(|(key, _)| key.to_string_lossy().starts_with("GRAPHHELM_SECRET_"))
    {
        command.env(key, value);
    }
    let Ok(child) = command.spawn() else {
        return report(OPEN, data, Some(failure("live.host_invalid", "/host", 3)));
    };
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let Ok(Ok(output)) = rx.recv_timeout(caller_wait(args) + Duration::from_secs(5)) else {
        return report(OPEN, data, Some(failure("live.timeout", "/host", 1)));
    };
    let Ok(envelope) = serde_json::from_slice::<Value>(&output.stdout) else {
        return report(OPEN, data, Some(failure("live.host_invalid", "/host", 1)));
    };
    let diagnostics: Vec<Diagnostic> =
        serde_json::from_value(envelope["diagnostics"].clone()).unwrap_or_default();
    Outcome {
        output: CommandOutput {
            ok: envelope["ok"] == true,
            command: if args.watch { WATCH } else { OPEN },
            data: Some(envelope["data"].clone()),
            diagnostics,
        },
        exit_code: output.status.code().unwrap_or(1),
    }
}

fn open_with(args: &JourneyOpenArgs) -> Outcome {
    let command = if args.watch { WATCH } else { OPEN };
    let mut data = json!({"flowId":args.id,"step":args.step,"path":null,"sessionId":null,"state":"unobserved","at":null,"headed":true,"modelCalls":0,"liveCaptureSignalId":null});
    if args.watch {
        data["mode"] = "watch".into();
        data["proof"] = false.into();
        data["step"] = Value::Null;
    }
    if !graphhelm_execution::valid_journey_id(&args.id) {
        return report(command, data, Some(failure("replay.id_invalid", "/id", 3)));
    }
    if args.watch && args.pace_ms > MAX_PACE_MS {
        return report(
            command,
            data,
            Some(failure("watch.pace_invalid", "/paceMs", 3)),
        );
    }
    if !(args.watch && args.step.is_empty()) && !graphhelm_execution::valid_journey_id(&args.step)
        || args
            .path
            .as_deref()
            .is_some_and(|p| !graphhelm_execution::valid_journey_id(p))
    {
        return report(
            command,
            data,
            Some(failure("replay.id_invalid", "/step", 3)),
        );
    }
    let bundle = [
        args.events.is_some(),
        args.execution.is_some(),
        args.keyring.is_some(),
        args.key_id.is_some(),
    ];
    if bundle.iter().any(|v| *v) && !bundle.iter().all(|v| *v) {
        return report(
            OPEN,
            data,
            Some(failure("live.recording_incomplete", "/recording", 3)),
        );
    }
    if args.live_host {
        host(args, data);
    }
    spawn_host(args, data)
}

/// The caller half: start the host, hand it the start frame, return its first envelope line.
/// A CLI caller exits right after the host's first line, while the host lives on. On Windows a
/// child inherits every inheritable handle, so the host would hold the CALLER's stdout open and
/// whoever waits for its EOF would wait for the whole session. Mark this process's own std
/// handles non-inheritable first; the host's stdio is set explicitly below.
fn detach_std_handles() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{
            HANDLE_FLAG_INHERIT, INVALID_HANDLE_VALUE, SetHandleInformation,
        };
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: GetStdHandle has no preconditions; SetHandleInformation only clears the
            // inherit flag of this process's own std handle, checked non-null and valid.
            unsafe {
                let handle = GetStdHandle(which);
                if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }
}

fn spawn_host(args: &JourneyOpenArgs, data: Value) -> Outcome {
    detach_std_handles();
    let Ok(executable) = std::env::current_exe() else {
        return report(OPEN, data, Some(failure("live.host_invalid", "/host", 3)));
    };
    let mut command = Command::new(executable);
    command
        .args([
            "--json",
            "journey",
            "open",
            &args.id,
            "--step",
            &args.step,
            "--live-host",
        ])
        .args(if args.watch {
            vec![
                "--watch".to_owned(),
                "--pace-ms".to_owned(),
                args.pace_ms.to_string(),
            ]
        } else {
            Vec::new()
        })
        .args(if args.window {
            vec!["--window"]
        } else {
            Vec::new()
        })
        .arg("--project")
        .arg(args.project.as_deref().unwrap_or(Path::new(".")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(path) = &args.path {
        command.args(["--path", path]);
    }
    for origin in &args.allow_origin {
        command.args(["--allow-origin", origin]);
    }
    if let (Some(events), Some(execution), Some(keyring), Some(key_id)) =
        (&args.events, &args.execution, &args.keyring, &args.key_id)
    {
        command
            .arg("--events")
            .arg(events)
            .args(["--execution", execution])
            .arg("--keyring")
            .arg(keyring)
            .args(["--key-id", key_id]);
    }
    safe_environment(&mut command, args.events.is_some());
    command.env("GRAPHHELM_LIVE_HOST", "1");
    for (key, value) in std::env::vars_os()
        .filter(|(key, _)| key.to_string_lossy().starts_with("GRAPHHELM_SECRET_"))
    {
        command.env(key, value);
    }
    let Ok(mut child) = command.spawn() else {
        return report(OPEN, data, Some(failure("live.host_invalid", "/host", 3)));
    };
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    if stdin
        .write_all(HANDSHAKE)
        .and_then(|()| stdin.flush())
        .is_err()
    {
        let _ = child.kill();
        return report(OPEN, data, Some(failure("live.host_invalid", "/host", 1)));
    }
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut line = Vec::new();
        let read = BufReader::new(stdout)
            .take(4 * FRAME as u64)
            .read_until(b'\n', &mut line);
        let _ = tx.send(read.map(|_| line));
    });
    let line = match rx.recv_timeout(caller_wait(args)) {
        Ok(Ok(line)) if !line.is_empty() => line,
        // An empty first line is the host's stdout closing: it exited before its envelope.
        // Name its exit status, so nobody has to bisect a bare timeout (#416 review).
        Ok(_) => {
            let status = (0..50)
                .find_map(|_| match child.try_wait() {
                    Ok(Some(status)) => Some(status.code()),
                    _ => {
                        std::thread::sleep(Duration::from_millis(20));
                        None
                    }
                })
                .flatten();
            let _ = child.kill();
            let _ = child.wait();
            let mut data = data;
            data["hostExitCode"] = status.map_or(Value::Null, Value::from);
            return report(OPEN, data, Some(failure("live.host_exited", "/host", 1)));
        }
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return report(OPEN, data, Some(failure("live.timeout", "/host", 1)));
        }
    };
    // The host keeps running with the browser open; its handles are released, not killed.
    drop(stdin);
    std::mem::forget(child);
    let Ok(output) = serde_json::from_slice::<Value>(&line) else {
        return report(OPEN, data, Some(failure("live.host_invalid", "/host", 1)));
    };
    let diagnostics: Vec<Diagnostic> =
        serde_json::from_value(output["diagnostics"].clone()).unwrap_or_default();
    let ok = output["ok"] == true;
    let exit_code = output["exitCode"]
        .as_i64()
        .unwrap_or(if ok { 0 } else { 1 }) as i32;
    Outcome {
        output: CommandOutput {
            ok,
            command: if args.watch { WATCH } else { OPEN },
            data: Some(output["data"].clone()),
            diagnostics,
        },
        exit_code,
    }
}

struct Session {
    record: Value,
    id: String,
    project: PathBuf,
    flow: Value,
    base: String,
    contract: String,
    visited: Vec<String>,
    current: Option<String>,
    driver: Driver,
    _output: TemporaryOutput,
    recording: Option<JourneyReplayArgs>,
    token: Vec<u8>,
    listener: TcpListener,
    deadline: Instant,
    /// The app under test this watch started (and stops when the host ends), if it had to.
    launched: Option<Launched>,
}

/// The host half. Never returns: it prints its one envelope line, serves, and exits.
fn host(args: &JourneyOpenArgs, mut data: Value) -> ! {
    let emit = |outcome: Outcome| -> ! {
        let mut line = serde_json::to_value(&outcome.output).unwrap_or(Value::Null);
        line["exitCode"] = outcome.exit_code.into();
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(line.to_string().as_bytes());
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
        std::process::exit(outcome.exit_code);
    };
    let mut start = Vec::new();
    if std::env::var_os("GRAPHHELM_LIVE_HOST").is_none()
        || std::io::stdin()
            .lock()
            .take(HANDSHAKE.len() as u64 + 1)
            .read_until(b'\n', &mut start)
            .is_err()
        || start != HANDSHAKE
    {
        emit(report(
            OPEN,
            data,
            Some(failure("live.host_invalid", "/host", 3)),
        ));
    }
    let command = if args.watch { WATCH } else { OPEN };
    let (mut session, outcome) = match walk(args, &mut data) {
        Ok(walked) => walked,
        Err(failed) => {
            // A watch that failed before the host could serve leaves no record behind.
            let dir = project_of(args.project.as_deref())
                .map(|p| p.join(SESSIONS))
                .ok();
            if args.watch
                && let (Some(dir), Some(id)) = (dir, data["sessionId"].as_str())
            {
                let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
            }
            emit(report(command, data, Some(failed)))
        }
    };
    // One envelope line, then the host serves. Nothing else is ever written to stdout.
    {
        let mut line =
            serde_json::to_value(&report(command, data, outcome).output).unwrap_or(Value::Null);
        let exit = if line["ok"] == true { 0 } else { 1 };
        line["exitCode"] = exit.into();
        let mut stdout = std::io::stdout().lock();
        let _ = stdout.write_all(line.to_string().as_bytes());
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
    serve(&mut session);
    let dir = session.project.join(SESSIONS);
    let _ = std::fs::remove_file(dir.join(format!("{}.json", session.id)));
    let _ = std::fs::remove_file(dir.join(format!("{}.token", session.id)));
    let _ = session.driver.close();
    if let Some(launched) = session.launched.take() {
        launched.stop();
    }
    std::process::exit(0);
}

fn drift_code(code: &str) -> &'static str {
    match code {
        "driver.locator_missing" => "drift.locator_missing",
        "driver.locator_ambiguous" => "drift.locator_ambiguous",
        "driver.expectation_failed" => "drift.expectation_failed",
        "driver.action_failed" => "drift.action_failed",
        "driver.timeout" => "drift.timeout",
        _ => "drift.wrong_screen",
    }
}

fn survivable(code: &str) -> bool {
    SURVIVABLE.contains(&code) || code == "replay.wrong_screen"
}

/// Walks to the step. `Ok` means a session exists (the browser is open); its second element is
/// the step's failure, if any: the step's expectations, or the drift that stopped the walk.
/// With recording, the step is captured `phase: live` on `pass` AND on `fail` (a live look at
/// the step even when its expectations do not hold); a drift stopped before the step, so it
/// records nothing.
fn walk(args: &JourneyOpenArgs, data: &mut Value) -> Result<(Session, Option<Failure>)> {
    let project = project_of(args.project.as_deref())?;
    safe_directory(&project.join(".graphhelm"), false)?;
    safe_directory(&project.join(".graphhelm/journeys"), false)?;
    let file = project
        .join(".graphhelm/journeys")
        .join(format!("{}.journey.yaml", args.id));
    if !safe_node(&file) {
        return Err(failure("replay.flow_invalid", "/flow", 2));
    }
    let read = if args.watch {
        super::journey_flow::read_for_watch(&file, &project)
    } else {
        super::journey_flow::read_for_replay(&file, &project)
    };
    let flow = read.map_err(|findings| {
        let finding = findings.into_iter().find(|f| !f.is_warning()).unwrap();
        failure(finding.code, finding.pointer, 2)
    })?;
    let edges: BTreeMap<String, Value> = flow["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| (edge["id"].as_str().unwrap().to_owned(), edge.clone()))
        .collect();
    let screens: BTreeMap<String, Value> = flow["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|screen| (screen["id"].as_str().unwrap().to_owned(), screen.clone()))
        .collect();
    let mut paths: Vec<_> = flow["paths"].as_object().unwrap().iter().collect();
    paths.sort_by(|(a, _), (b, _)| (a.as_str() != "main", a).cmp(&(b.as_str() != "main", b)));
    let visited_of = |path_edges: &Value| -> Vec<String> {
        let ids = path_edges.as_array().unwrap();
        let first = edges[ids[0].as_str().unwrap()]["from"].as_str().unwrap();
        std::iter::once(first.to_owned())
            .chain(ids.iter().map(|id| {
                edges[id.as_str().unwrap()]["to"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            }))
            .collect()
    };
    let (name, path_edges) = paths
        .into_iter()
        .filter(|(name, _)| args.path.as_deref().is_none_or(|p| p == name.as_str()))
        .find(|(_, path_edges)| args.watch || visited_of(path_edges).contains(&args.step))
        .ok_or_else(|| {
            if args.watch {
                failure("watch.path_unknown", "/path", 2)
            } else {
                failure("live.step_unreachable", "/step", 2)
            }
        })?;
    let visited = visited_of(path_edges);
    // A watch plays the whole path; an open stops at its step.
    let step = if args.watch {
        visited.last().unwrap().clone()
    } else {
        args.step.clone()
    };
    let target = visited.iter().position(|s| *s == step).unwrap();
    data["step"] = step.clone().into();
    if args.watch {
        let acts: u64 = path_edges
            .as_array()
            .unwrap()
            .iter()
            .map(|id| {
                edges[id.as_str().unwrap()]["acts"]
                    .as_array()
                    .unwrap()
                    .len() as u64
            })
            .sum();
        // Half the run budget for waiting leaves the other half for the browser's own work.
        if acts.saturating_mul(args.pace_ms) > RUN_BUDGET.as_millis() as u64 / 2 {
            return Err(failure("watch.pace_too_slow", "/paceMs", 3));
        }
    }
    // #515: the acts of a DRAFT this watch will not perform, named before any browser starts. An
    // approved flow carries its owner's approval of every act (a stale approval never reaches
    // here: `read_for_watch` refuses it), so it is played whole.
    let mut guarded: Vec<Value> = Vec::new();
    if args.watch {
        if flow["status"] != "approved" {
            for id in path_edges.as_array().unwrap() {
                let id = id.as_str().unwrap();
                for (act_index, act) in edges[id]["acts"].as_array().unwrap().iter().enumerate() {
                    if let Some(word) = would_destroy(act) {
                        guarded.push(json!({"edge":id,"actIndex":act_index,"kind":act["kind"],
                            "role":act["role"],"name":act["name"],"would":word}));
                    }
                }
            }
        }
        data["guarded"] = guarded.clone().into();
    }
    data["path"] = name.as_str().into();
    let contract = if name == "main" {
        args.id.clone()
    } else {
        format!("{}.{}", args.id, name)
    };

    // An open is deterministic only through a current cache: absent or void, refuse. A watch
    // plays the flow's own role/name acts, so a draft that was never replayed can be watched.
    let cache = if args.watch {
        json!({"viewport":{"width":WATCH_WIDTH,"height":WATCH_HEIGHT},"edges":{}})
    } else {
        let cache_file = project
            .join(".graphhelm/journey-cache")
            .join(format!("{}.json", args.id));
        let cache = load_cache(&cache_file, &flow)?
            .ok_or_else(|| failure("replay.cache_missing", "/cache", 2))?;
        if cache["flowDigest"] != super::journey_flow::approval_digest(&flow) {
            return Err(failure("replay.cache_missing", "/cache/flowDigest", 2));
        }
        cache
    };
    let secrets = preflight(&flow)?;
    observer_ready(&project)?;
    let output = TemporaryOutput::create()?;
    let base = flow["base"].as_str().unwrap().to_owned();
    // The owner only clicks Watch: when the app under test is down, the project's declared
    // launcher brings it up (and the host stops it when the watch ends).
    let launched = if args.watch && !base_reachable(&base) {
        let launched = launch(&project, &base)?;
        data["launched"] = true.into();
        Some(launched)
    } else {
        None
    };
    // The app this watch started from the project's declared launcher is the disposable fixture,
    // so every act is played there. An app that was already answering cannot be told from a real
    // one by its address, and keeps the guard.
    if launched.is_some() {
        guarded.clear();
        data["guarded"] = json!([]);
    }
    let mut driver = Driver::start(&project, output.path(), &secrets)?;
    let entry = format!(
        "{}{}",
        base.trim_end_matches('/'),
        screens[&visited[0]]["url"].as_str().unwrap()
    );
    let streamed = args.watch && !args.window;
    // #519: the reply says how this play is seen: frames for the Studio, or a window here.
    data["headed"] = (!streamed).into();
    data["frame"] = streamed.into();
    let mut open = json!({"base":entry,"viewport":cache["viewport"],"allowOrigins":args.allow_origin,"headed":!streamed});
    if args.watch {
        // #491: the caption and outline are drawn in the page. #519: by default the page streams
        // as frames the Studio shows; `--window` keeps the maximized window instead.
        open["show"] = true.into();
        open["screencast"] = streamed.into();
    }
    match driver.call("open", open.clone(), "/entry") {
        Ok(_) => {}
        // A just-launched app (a dev server) builds its first page on the first request, which
        // can outlast one request budget. The browser ended with that request; one fresh browser
        // tries the now-built entry again. Only after a launch, and only once.
        Err((code, _, _))
            if launched.is_some() && matches!(code, "replay.timeout" | "driver.timeout") =>
        {
            data["entryRetried"] = true.into();
            driver = Driver::start(&project, output.path(), &secrets)?;
            driver.call("open", open, "/entry")?;
        }
        Err(failed) => return Err(failed),
    }
    let recording = args.events.is_some().then(|| replay_args(args, &project));
    // A watch is visible to the Studio from its first act: the session record exists while it
    // plays (`state: playing`) and names the screen and the act it is at.
    let session_id = format!("live-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
    let progress = Progress {
        dir: project.join(SESSIONS),
        id: session_id.clone(),
        base: json!({"sessionId":session_id,"contractId":contract,"flowId":args.id,"path":name,
            "stepId":step,"mode":"watch","proof":false,"state":"playing","code":null,"at":null,
            "since":chrono::Utc::now().to_rfc3339(),"stepCount":visited.len(),
            "frame":streamed,"frameDir":frame_dir(&output, streamed),
            "expiresAt":(chrono::Utc::now() + chrono::Duration::seconds(RUN_BUDGET.as_secs() as i64)).to_rfc3339()}),
    };
    if args.watch {
        safe_directory(&progress.dir, true)?;
        progress.show(&visited[0], 0, None, None, None);
        data["sessionId"] = session_id.clone().into();
    }
    let pace = Duration::from_millis(args.pace_ms);
    let play_deadline = Instant::now() + RUN_BUDGET;
    let mut current = Some(visited[0].clone());
    let mut step_failure = None;
    for (index, screen_id) in visited.iter().enumerate().take(target + 1) {
        if index > 0 {
            let edge_id = path_edges[index - 1].as_str().unwrap();
            let edge = &edges[edge_id];
            for (act_index, act) in edge["acts"].as_array().unwrap().iter().enumerate() {
                let at = format!("{edge_id}/{act_index}");
                if let Some(skip) = guarded
                    .iter()
                    .find(|g| g["edge"] == edge_id && g["actIndex"] == act_index)
                {
                    // Not performed: the play stops here, on the screen before the act, and
                    // says what it would have done. The browser stays open for the owner.
                    let caption = caption_bounded(format!(
                        "Skipped: would {} \"{}\"",
                        skip["would"].as_str().unwrap_or_default(),
                        act["name"].as_str().unwrap_or_default()
                    ));
                    progress.show(
                        &visited[index - 1],
                        index - 1,
                        Some(edge_id),
                        Some(act_index),
                        Some(&caption),
                    );
                    let _ = driver.call(
                        "show",
                        json!({"caption":caption,"role":act["role"],"name":act["name"]}),
                        &format!("/edges/{at}/show"),
                    );
                    data["state"] = "skipped".into();
                    data["at"] = at.clone().into();
                    data["skipped"] = skip.clone();
                    step_failure = Some(failure(
                        "watch.act_skipped_destructive",
                        format!("/edges/{at}"),
                        1,
                    ));
                    break;
                }
                if args.watch {
                    if Instant::now() + pace >= play_deadline {
                        return Err(failure("watch.budget_exceeded", format!("/edges/{at}"), 1));
                    }
                    // #491: caption the step and outline its control, then wait the pace. #519: the
                    // frames reach the owner in the Studio, so the caption says what the act does
                    // without the edge id (#520 review).
                    let caption = caption_bounded(act_caption(act));
                    progress.show(
                        &visited[index - 1],
                        index - 1,
                        Some(edge_id),
                        Some(act_index),
                        Some(&caption),
                    );
                    let _ = driver.call(
                        "show",
                        json!({"caption":caption,"role":act["role"],"name":act["name"]}),
                        &format!("/edges/{at}/show"),
                    );
                    std::thread::sleep(pace);
                }
                let mut request = json!({"kind":act["kind"],"role":act["role"],"name":act["name"]});
                if !args.watch {
                    request["locator"] = cache["edges"][edge_id][act_index].clone();
                }
                if let Some(text) = act.get("text") {
                    request["text"] = text.clone();
                }
                if let Some(secret) = act["secret"].as_str() {
                    request["secretEnv"] = format!("GRAPHHELM_SECRET_{secret}").into();
                }
                match driver.call("act", request, &format!("/edges/{at}")) {
                    Ok(_) => {}
                    Err((code, _, _)) if survivable(code) => {
                        data["state"] = "drift".into();
                        data["at"] = at.clone().into();
                        step_failure = Some(failure(drift_code(code), format!("/edges/{at}"), 1));
                        break;
                    }
                    Err(failed) => return Err(failed),
                }
            }
            if step_failure.is_some() {
                break;
            }
            current = None;
        }
        let pointer = format!("/screens/{screen_id}");
        match observe(&mut driver, &screens[screen_id], &base, &pointer) {
            Ok(_) => {
                current = Some(screen_id.clone());
                if args.watch {
                    progress.show(screen_id, index, None, None, None);
                }
            }
            Err((code, _, _)) if survivable(code) => {
                if index == target {
                    data["state"] = "fail".into();
                    data["at"] = screen_id.clone().into();
                    let code = if code == "replay.wrong_screen" {
                        "drift.wrong_screen"
                    } else {
                        code
                    };
                    step_failure = Some(failure(code, pointer, 1));
                } else if index == 0 {
                    // The entry screen itself does not match: there is no edge to blame yet.
                    data["state"] = "drift".into();
                    data["at"] = screen_id.clone().into();
                    step_failure = Some(failure(drift_code(code), pointer, 1));
                } else {
                    let edge_id = path_edges[index - 1].as_str().unwrap();
                    data["state"] = "drift".into();
                    data["at"] = edge_id.into();
                    step_failure = Some(failure(drift_code(code), format!("/edges/{edge_id}"), 1));
                }
                break;
            }
            Err(failed) => return Err(failed),
        }
        if index == target {
            data["state"] = "pass".into();
            data["at"] = screen_id.clone().into();
        }
    }
    if data["state"] != "drift"
        && let Some(recording) = &recording
    {
        let image = format!("live-{step}.png");
        driver.call(
            "capture",
            json!({"path":image,"maskSecrets":true}),
            "/capture",
        )?;
        let signal = record(
            recording,
            &contract,
            &step,
            &output.path().join(&image),
            Some("live"),
        )?;
        data["liveCaptureSignalId"] = signal.into();
    }
    data["code"] = step_failure
        .as_ref()
        .map_or(Value::Null, |(code, _, _)| Value::from(*code));
    let session = start_session(
        &project, args, flow, base, contract, visited, current, driver, output, recording, data,
        session_id,
    )?;
    let mut session = session;
    session.launched = launched;
    Ok((session, step_failure))
}

/// Removes the records and tokens of sessions whose run budget ended without a close (a killed
/// host never removes its own). Only `live-<hex>` names, only regular files, only expired ones.
fn sweep_expired(dir: &Path) {
    let now = chrono::Utc::now();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.strip_suffix(".json"))
            .filter(|id| valid_session(id))
            .map(str::to_owned)
        else {
            continue;
        };
        let expired = safe_node(&entry.path())
            && std::fs::read(entry.path())
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                .and_then(|record| {
                    chrono::DateTime::parse_from_rfc3339(record["expiresAt"].as_str()?).ok()
                })
                .is_some_and(|expires| expires < now);
        if expired {
            let _ = std::fs::remove_file(dir.join(format!("{id}.json")));
            let token = dir.join(format!("{id}.token"));
            if safe_node(&token) {
                let _ = std::fs::remove_file(token);
            }
        }
    }
}

/// The project's declaration of how to start the app its journey flows are written against
/// (`.graphhelm/journey-fixture.json`, schema `graphhelm-journey-fixture/1`): a POSIX shell
/// script run from the project root with `up <dir>` and `down <dir>`, `<dir>` a fresh directory
/// outside the checkout. Read only by `journey watch`, which is owner-only; it is the project's
/// own code, run as `npm test` would be.
const FIXTURE_FILE: &str = ".graphhelm/journey-fixture.json";
const FIXTURE_SCHEMA: &str = "graphhelm-journey-fixture/1";
/// How long a launched app may take to answer on the flow's base.
const LAUNCH_READY: Duration = Duration::from_secs(120);

/// How long a caller waits for the host's first line (#462 review). An open answers within the
/// run budget. A watch may first launch the app (`LAUNCH_READY`), retry a cold entry once (two
/// request budgets), then play inside the run budget, which the host enforces itself before every
/// act, so the host always ends (and stops what it launched) before any caller gives up on it.
fn caller_wait(args: &JourneyOpenArgs) -> Duration {
    if args.watch {
        LAUNCH_READY + OP_BUDGET * 2 + RUN_BUDGET + Duration::from_secs(10)
    } else {
        RUN_BUDGET
    }
}

struct Launched {
    project: PathBuf,
    script: String,
    dir: PathBuf,
}

impl Launched {
    fn stop(self) {
        drop(self);
    }
}

impl Drop for Launched {
    /// A launched app is stopped however the watch ends: the host's own end calls `stop`, and
    /// any failure between the launch and the session drops it here (#462: a cold first page
    /// that timed out left the app running).
    fn drop(&mut self) {
        let _ = posix_shell()
            .arg(&self.script)
            .arg("down")
            .arg(&self.dir)
            .current_dir(&self.project)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// `host:port` of an `http://` base, the port defaulting to 80.
fn base_address(base: &str) -> Option<String> {
    let rest = base.strip_prefix("http://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty() {
        return None;
    }
    Some(if authority.contains(':') {
        authority.to_owned()
    } else {
        format!("{authority}:80")
    })
}

fn base_reachable(base: &str) -> bool {
    use std::net::ToSocketAddrs;
    base_address(base)
        .and_then(|address| address.to_socket_addrs().ok())
        .into_iter()
        .flatten()
        .any(|address| TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_ok())
}

/// A POSIX shell: `GRAPHHELM_POSIX_SHELL`, else Git for Windows' bash on Windows, else `sh`.
fn posix_shell() -> Command {
    if let Some(shell) = std::env::var_os("GRAPHHELM_POSIX_SHELL") {
        return Command::new(shell);
    }
    #[cfg(windows)]
    {
        let git_bash = Path::new(r"C:\Program Files\Git\bin\bash.exe");
        if git_bash.is_file() {
            return Command::new(git_bash);
        }
    }
    Command::new("sh")
}

/// Starts the declared app under test and waits until the flow's base answers.
fn launch(project: &Path, base: &str) -> Result<Launched> {
    let declared = project.join(FIXTURE_FILE);
    if !safe_node(&declared) {
        return Err(failure("watch.app_down", "/base", 2));
    }
    let fixture: Value = std::fs::read(&declared)
        .ok()
        .filter(|bytes| bytes.len() <= FRAME)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or_else(|| failure("watch.launcher_invalid", "/launcher", 2))?;
    let script = fixture["script"]
        .as_str()
        .filter(|script| {
            fixture["schema"] == FIXTURE_SCHEMA
                && !script.is_empty()
                && !Path::new(script).is_absolute()
                && !script.split(['/', '\\']).any(|part| part == "..")
                && safe_node(&project.join(script))
        })
        .ok_or_else(|| failure("watch.launcher_invalid", "/launcher", 2))?
        .to_owned();
    let dir = std::env::temp_dir().join(format!(
        "graphhelm-watch-{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    ));
    let mut command = posix_shell();
    command
        .arg(&script)
        .arg("up")
        .arg(&dir)
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(executable) = std::env::current_exe() {
        command.env("GRAPHHELM_BIN", executable);
    }
    let launched = Launched {
        project: project.to_path_buf(),
        script,
        dir,
    };
    let started = command.status().map(|status| status.success());
    let ready = Instant::now() + LAUNCH_READY;
    while started.as_ref().is_ok_and(|ok| *ok) && !base_reachable(base) && Instant::now() < ready {
        std::thread::sleep(Duration::from_millis(250));
    }
    if !started.is_ok_and(|ok| ok) || !base_reachable(base) {
        launched.stop();
        return Err(failure("watch.launch_failed", "/launcher", 1));
    }
    Ok(launched)
}

/// The words a watch shows for an act (#491), the way the Studio's journey cards say it.
fn act_caption(act: &Value) -> String {
    let name = act["name"].as_str().unwrap_or_default();
    let verb = match act["kind"].as_str().unwrap_or_default() {
        "activate" => "Clicks",
        "submit" => "Submits",
        "enter_text" => "Types into",
        "navigate" => "Opens",
        "wait_for" => "Waits for",
        _ => "Checks",
    };
    format!("{verb} \"{name}\"")
}

/// The driver refuses a caption over 200 bytes (`show`, a protocol error that ends the session),
/// and a schema-valid control name reaches 256 bytes: cut on a character boundary, with an
/// ellipsis, never above the bound (#497 review).
const CAPTION_LIMIT: usize = 200;

fn caption_bounded(caption: String) -> String {
    if caption.len() <= CAPTION_LIMIT {
        return caption;
    }
    let mut end = CAPTION_LIMIT - '…'.len_utf8();
    while !caption.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &caption[..end])
}

/// The page size a watch plays at (#519): the frames the Studio shows are this size.
const WATCH_WIDTH: u32 = 1280;
const WATCH_HEIGHT: u32 = 800;
/// The largest frame `read_frame` serves; a JPEG of the watch page is far below it.
const FRAME_LIMIT: u64 = 8 * 1024 * 1024;

/// Where a streamed watch's driver writes its latest frame (#519), kept in the session record
/// for the Runtime's frame route and never listed (`journey sessions` drops it).
fn frame_dir(output: &TemporaryOutput, streamed: bool) -> Value {
    if streamed {
        Value::from(
            output
                .path()
                .join("screencast")
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        Value::Null
    }
}

/// What `GET /v1/journeys/sessions/{id}/frame` answers (#519).
#[derive(Debug, PartialEq)]
pub(crate) enum FrameRead {
    /// No such session, or it ended (`live.session_gone`).
    Gone,
    /// The session exists but has no frame yet (or is a window watch, which never has one).
    Pending,
    Frame {
        seq: u64,
        width: u64,
        height: u64,
        jpeg: Vec<u8>,
    },
}

/// The latest frame of a streamed watch. Only a `screencast` directory directly inside one of
/// this machine's `graphhelm-replay-*` outputs is read, whatever the record says, so a record
/// edited on disk cannot turn the route into a file reader. Never journaled, never proof: the
/// frame goes when the host removes its output on close.
pub(crate) fn read_frame(project: &Path, id: &str) -> FrameRead {
    if !valid_session(id) {
        return FrameRead::Gone;
    }
    let file = project.join(SESSIONS).join(format!("{id}.json"));
    if !safe_node(&file) {
        return FrameRead::Gone;
    }
    let Some(record) = std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return FrameRead::Gone;
    };
    let live = record["expiresAt"]
        .as_str()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .is_some_and(|at| at >= chrono::Utc::now());
    if !live {
        return FrameRead::Gone;
    }
    let Some(dir) = record["frameDir"].as_str().map(PathBuf::from) else {
        return FrameRead::Pending;
    };
    let inside_an_output = std::env::temp_dir()
        .canonicalize()
        .ok()
        .is_some_and(|temp| {
            dir.file_name().is_some_and(|name| name == "screencast")
                && dir.parent().is_some_and(|output| {
                    output.parent() == Some(temp.as_path())
                        && output
                            .file_name()
                            .and_then(|name| name.to_str())
                            .is_some_and(|name| name.starts_with("graphhelm-replay-"))
                })
        });
    // A junction or symlink named like an output (or its `screencast`) resolves elsewhere, so its
    // canonical path differs from the one recorded; only a real directory is read (#559 review).
    let real = |path: &Path| path.canonicalize().ok().as_deref() == Some(path);
    if !inside_an_output || !real(&dir) || !dir.parent().is_some_and(real) {
        return FrameRead::Pending;
    }
    let read = |name: &str, limit: u64| -> Option<Vec<u8>> {
        let path = dir.join(name);
        if !safe_node(&path) || std::fs::metadata(&path).ok()?.len() > limit {
            return None;
        }
        std::fs::read(path).ok()
    };
    // The meta is written after its JPEG, so it never names a frame that is not on disk yet.
    let Some(meta) =
        read("frame.json", 1024).and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
    else {
        return FrameRead::Pending;
    };
    let (Some(seq), Some(width), Some(height)) = (
        meta["seq"].as_u64(),
        meta["width"].as_u64(),
        meta["height"].as_u64(),
    ) else {
        return FrameRead::Pending;
    };
    match read("frame.jpg", FRAME_LIMIT) {
        Some(jpeg) if !jpeg.is_empty() => FrameRead::Frame {
            seq,
            width,
            height,
            jpeg,
        },
        _ => FrameRead::Pending,
    }
}

/// The session record a `journey watch` keeps current while it plays (the Studio's "current
/// step" signal, read through `journey sessions`). It has no port until the play ends and the
/// host starts serving, so `act` and `close` answer `live.session_gone` during the play.
struct Progress {
    dir: PathBuf,
    id: String,
    base: Value,
}

impl Progress {
    fn show(
        &self,
        screen: &str,
        index: usize,
        edge: Option<&str>,
        act: Option<usize>,
        caption: Option<&str>,
    ) {
        let mut record = self.base.clone();
        // #505: the words the watch browser shows for this act, for the Studio's readout.
        record["caption"] = caption.map_or(Value::Null, Value::from);
        record["screen"] = screen.into();
        record["stepIndex"] = index.into();
        record["edge"] = edge.map_or(Value::Null, Value::from);
        record["actIndex"] = act.map_or(Value::Null, Value::from);
        let _ = save_record(&self.dir.join(format!("{}.json", self.id)), &record);
    }
}

fn save_record(path: &Path, record: &Value) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(record).unwrap();
    bytes.push(b'\n');
    super::journey_flow::atomic_write(path, &bytes)
        .map_err(|_| failure("live.session_unwritable", "/session", 1))
}

#[allow(clippy::too_many_arguments)]
fn start_session(
    project: &Path,
    args: &JourneyOpenArgs,
    flow: Value,
    base: String,
    contract: String,
    visited: Vec<String>,
    current: Option<String>,
    driver: Driver,
    output: TemporaryOutput,
    recording: Option<JourneyReplayArgs>,
    data: &mut Value,
    id: String,
) -> Result<Session> {
    let dir = project.join(SESSIONS);
    safe_directory(&dir, true)?;
    sweep_expired(&dir);
    let streamed = args.watch && !args.window;
    let (_, token) = crate::commands::secret_file::ensure(
        &dir.join(format!("{id}.token")),
        "live session token",
    )
    .map_err(|_| failure("live.session_unwritable", "/session", 1))?;
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|_| failure("live.session_unwritable", "/session/port", 1))?;
    let port = listener
        .local_addr()
        .map_err(|_| failure("live.session_unwritable", "/session/port", 1))?
        .port();
    let expires = RUN_BUDGET.as_secs();
    let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires as i64);
    // The session's readable state (the Studio chip, #409, reads it through `journey
    // sessions`): updated after every act, removed on close. Port and pid address the host.
    let record = json!({"sessionId":id,"contractId":contract,"flowId":args.id,"path":data["path"],
        "stepId":data["step"],"mode":if args.watch {"watch"} else {"open"},"proof":false,
        "stepIndex":visited.iter().position(|s| Some(s) == current.as_ref()),"stepCount":visited.len(),
        "edge":null,"actIndex":null,"skipped":data["skipped"],
        "state":data["state"],"code":data["code"],"at":data["at"],
        "screen":current,"since":chrono::Utc::now().to_rfc3339(),"lastActAt":null,"lastActState":null,"lastActCode":null,
        "frame":streamed,"frameDir":frame_dir(&output, streamed),
        "expiresAt":expires_at.to_rfc3339(),"port":port,"pid":std::process::id()});
    save_record(&dir.join(format!("{id}.json")), &record)?;
    data["sessionId"] = id.clone().into();
    data["contractId"] = contract.clone().into();
    data["expiresInSeconds"] = expires.into();
    Ok(Session {
        record,
        id,
        project: project.to_path_buf(),
        flow,
        base,
        contract,
        visited,
        current,
        driver,
        _output: output,
        recording,
        token: token.into_bytes(),
        listener,
        deadline: Instant::now() + RUN_BUDGET,
        launched: None,
    })
}

fn serve(session: &mut Session) {
    if session.listener.set_nonblocking(true).is_err() {
        return;
    }
    while Instant::now() < session.deadline {
        let stream = match session.listener.accept() {
            Ok((stream, _)) => stream,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(_) => return,
        };
        let (reply, keep) = answer(session, stream_request(&stream));
        let mut stream = stream;
        let _ = stream.write_all(format!("{reply}\n").as_bytes());
        let _ = stream.flush();
        if !keep {
            return;
        }
    }
}

fn stream_request(stream: &TcpStream) -> Option<Value> {
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(OP_BUDGET)).ok()?;
    let mut line = Vec::new();
    BufReader::new(stream)
        .take(FRAME as u64 + 1)
        .read_until(b'\n', &mut line)
        .ok()?;
    if line.len() > FRAME {
        return None;
    }
    serde_json::from_slice(&line).ok()
}

/// One request: the reply line and whether the session keeps serving.
fn answer(session: &mut Session, request: Option<Value>) -> (Value, bool) {
    let refuse = |code: &str| json!({"ok":false,"code":code});
    let Some(request) = request else {
        return (refuse("live.request_invalid"), true);
    };
    let token = request["token"].as_str().unwrap_or("");
    if !equal(token.as_bytes(), &session.token) {
        return (refuse("live.unauthorized"), true);
    }
    match request["op"].as_str() {
        Some("close") => (json!({"ok":true,"closed":true}), false),
        Some("act") => act_on(session, &request),
        _ => (refuse("live.request_invalid"), true),
    }
}

fn act_on(session: &mut Session, request: &Value) -> (Value, bool) {
    let refuse = |code: &str| json!({"ok":false,"code":code});
    let (Some(kind), Some(role), Some(name)) = (
        request["kind"].as_str(),
        request["role"].as_str(),
        request["name"].as_str(),
    ) else {
        return (refuse("live.request_invalid"), true);
    };
    if !ACT_KINDS.contains(&kind) || role.is_empty() || name.is_empty() || name.len() > 256 {
        return (refuse("driver.unsupported_act"), true);
    }
    let lower = name.to_lowercase();
    if DESTRUCTIVE.iter().any(|word| lower.contains(word)) {
        let approved = session.current.as_deref().is_some_and(|current| {
            session.flow["edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|edge| {
                    edge["from"] == current
                        && edge["acts"].as_array().unwrap().iter().any(|act| {
                            act["kind"] == kind && act["role"] == role && act["name"] == name
                        })
                })
        });
        if !approved {
            return (refuse("live.act_refused_destructive"), true);
        }
    }
    let mut driver_request = json!({"kind":kind,"role":role,"name":name});
    if let Some(text) = request["text"].as_str() {
        driver_request["text"] = text.into();
    }
    if let Some(secret) = request["secret"].as_str() {
        let known = session.flow["secrets"]
            .as_array()
            .is_some_and(|names| names.iter().any(|n| n == secret));
        if !known {
            return (refuse("live.secret_unknown"), true);
        }
        driver_request["secretEnv"] = format!("GRAPHHELM_SECRET_{secret}").into();
    }
    if kind == "enter_text" && driver_request.get("text").is_none() && request["secret"].is_null() {
        return (refuse("replay.act_value_missing"), true);
    }
    match session.driver.call("act", driver_request, "/act") {
        Ok(_) => {}
        Err((code, _, _)) if survivable(code) => return (refuse(code), true),
        Err((code, _, _)) => return (refuse(code), false),
    }
    let observed = match session
        .driver
        .call("snapshot", json!({"expect":[]}), "/act/snapshot")
    {
        Ok(snapshot) => snapshot,
        Err((code, _, _)) => return (refuse(code), survivable(code)),
    };
    let url = observed["url"].as_str().unwrap_or("").to_owned();
    let matches: Vec<String> = session.flow["screens"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|screen| url_matches(&session.base, screen["url"].as_str().unwrap(), &url))
        .map(|screen| screen["id"].as_str().unwrap().to_owned())
        .collect();
    let mut reply = json!({"ok":true,"url":url,"screen":null,"state":"unknown","code":null,"liveCaptureSignalId":null});
    session.current = None;
    if let [screen_id] = matches.as_slice() {
        let screen = session.flow["screens"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == screen_id.as_str())
            .unwrap()
            .clone();
        reply["screen"] = screen_id.as_str().into();
        match observe(
            &mut session.driver,
            &screen,
            &session.base,
            &format!("/screens/{screen_id}"),
        ) {
            Ok(_) => {
                reply["state"] = "pass".into();
                session.current = Some(screen_id.clone());
            }
            Err((code, _, _)) if survivable(code) => {
                reply["state"] = "fail".into();
                reply["code"] = code.into();
            }
            Err((code, _, _)) => return (refuse(code), false),
        }
        if reply["state"] == "pass"
            && session.visited.contains(screen_id)
            && let Some(recording) = session.recording.clone()
        {
            let image = format!("live-{screen_id}-{}.png", uuid::Uuid::new_v4().simple());
            if session
                .driver
                .call(
                    "capture",
                    json!({"path":image,"maskSecrets":true}),
                    "/act/capture",
                )
                .is_ok()
                && let Ok(signal) = record(
                    &recording,
                    &session.contract,
                    screen_id,
                    &session._output.path().join(&image),
                    Some("live"),
                )
            {
                reply["liveCaptureSignalId"] = signal.into();
            }
        }
    }
    // `state`, `code` and `at` stay the OPEN result (a drift keeps naming its edge); an act
    // only moves the screen and the `lastAct*` fields.
    session.record["screen"] = reply["screen"].clone();
    session.record["lastActState"] = reply["state"].clone();
    session.record["lastActCode"] = reply["code"].clone();
    session.record["lastActAt"] = chrono::Utc::now().to_rfc3339().into();
    let path = session
        .project
        .join(SESSIONS)
        .join(format!("{}.json", session.id));
    let _ = save_record(&path, &session.record);
    (reply, true)
}

/// `journey sessions` (#398; the Studio's live chip, #409, reads it): every live session record
/// of the project whose run budget has not ended, newest first. Read-only; no host is contacted.
pub(super) fn sessions(args: &JourneySessionsArgs) -> Outcome {
    const SESSIONS_COMMAND: &str = "journey.sessions";
    let project = match project_of(args.project.as_deref()) {
        Ok(project) => project,
        Err(failed) => return report(SESSIONS_COMMAND, json!({"sessions": []}), Some(failed)),
    };
    let now = chrono::Utc::now();
    let mut sessions: Vec<Value> = std::fs::read_dir(project.join(SESSIONS))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .and_then(|name| name.strip_suffix(".json"))
                .is_some_and(valid_session)
                && safe_node(&entry.path())
        })
        .filter_map(|entry| {
            let bytes = std::fs::read(entry.path()).ok()?;
            let mut record: Value = serde_json::from_slice(&bytes).ok()?;
            let expires =
                chrono::DateTime::parse_from_rfc3339(record["expiresAt"].as_str()?).ok()?;
            if expires < now {
                return None;
            }
            let object = record.as_object_mut()?;
            object.remove("port");
            object.remove("pid");
            object.remove("frameDir");
            Some(record)
        })
        .collect();
    sessions.sort_by(|a, b| b["since"].as_str().cmp(&a["since"].as_str()));
    Outcome::success(SESSIONS_COMMAND, json!({"sessions": sessions}))
}

/// The client half of `act` and `close`: one request to the session host, one reply.
fn request(command: &'static str, session: &str, project: Option<&Path>, body: Value) -> Outcome {
    let data = json!({"sessionId":session});
    if !valid_session(session) {
        return report(
            command,
            data,
            Some(failure("live.request_invalid", "/session", 3)),
        );
    }
    let project = match project_of(project) {
        Ok(project) => project,
        Err(failed) => return report(command, data, Some(failed)),
    };
    let dir = project.join(SESSIONS);
    let record: Option<Value> = std::fs::read(dir.join(format!("{session}.json")))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let token = crate::commands::secret_file::read_existing(
        &dir.join(format!("{session}.token")),
        "live session token",
    );
    let (Some(record), Ok(token)) = (record, token) else {
        return report(
            command,
            data,
            Some(failure("live.session_gone", "/session", 2)),
        );
    };
    let Some(port) = record["port"].as_u64().and_then(|p| u16::try_from(p).ok()) else {
        return report(
            command,
            data,
            Some(failure("live.session_gone", "/session", 2)),
        );
    };
    let mut body = body;
    body["token"] = token.into();
    let reply = (|| -> Option<Value> {
        let mut stream = TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(2),
        )
        .ok()?;
        stream.set_read_timeout(Some(OP_BUDGET * 2)).ok()?;
        stream
            .write_all(format!("{body}\n").as_bytes())
            .and_then(|()| stream.flush())
            .ok()?;
        let mut line = Vec::new();
        BufReader::new(&stream)
            .take(FRAME as u64 + 1)
            .read_until(b'\n', &mut line)
            .ok()?;
        serde_json::from_slice(&line).ok()
    })();
    let Some(mut reply) = reply else {
        return report(
            command,
            data,
            Some(failure("live.session_gone", "/session", 2)),
        );
    };
    reply["sessionId"] = session.into();
    if reply["ok"] == true {
        if let Some(object) = reply.as_object_mut() {
            object.remove("ok");
        }
        return Outcome::success(command, reply);
    }
    let code: &'static str = match reply["code"].as_str().unwrap_or("") {
        "live.unauthorized" => "live.unauthorized",
        "live.request_invalid" => "live.request_invalid",
        "live.act_refused_destructive" => "live.act_refused_destructive",
        "live.secret_unknown" => "live.secret_unknown",
        "replay.act_value_missing" => "replay.act_value_missing",
        "driver.unsupported_act" => "driver.unsupported_act",
        "driver.locator_missing" => "driver.locator_missing",
        "driver.locator_ambiguous" => "driver.locator_ambiguous",
        "driver.expectation_failed" => "driver.expectation_failed",
        "driver.action_failed" => "driver.action_failed",
        "driver.timeout" => "driver.timeout",
        "driver.secret_literal" => "driver.secret_literal",
        "driver.secret_missing" => "driver.secret_missing",
        _ => "live.request_failed",
    };
    report(command, reply, Some(failure(code, "/act", 1)))
}

pub(super) fn act(args: &JourneyActArgs) -> Outcome {
    let mut body = json!({"op":"act","kind":args.kind,"role":args.role,"name":args.name});
    if let Some(text) = &args.text {
        body["text"] = text.as_str().into();
    }
    if let Some(secret) = &args.secret {
        body["secret"] = secret.as_str().into();
    }
    request(ACT, &args.session, args.project.as_deref(), body)
}

pub(super) fn close(args: &JourneyCloseArgs) -> Outcome {
    request(
        CLOSE,
        &args.session,
        args.project.as_deref(),
        json!({"op":"close"}),
    )
}

#[cfg(test)]
mod watch_guard_tests {
    use serde_json::json;

    /// #515: which acts of a draft a watch refuses to perform. Defects named: a guard that also
    /// stops acts that commit nothing (typing a password, waiting for "Order sent"), one that
    /// reads "Dropdown" as `drop`, and one that misses a deny word inside a longer name or in
    /// another case. Cost: microseconds.
    #[test]
    fn only_committing_acts_with_a_deny_word_are_guarded() {
        let would = |kind: &str, name: &str| {
            super::would_destroy(&json!({"kind":kind,"role":"button","name":name}))
        };
        assert_eq!(would("submit", "Pay now"), Some("pay"));
        assert_eq!(would("activate", "DELETE account"), Some("delete"));
        assert_eq!(would("activate", "Resend invoice"), Some("send"));
        assert_eq!(would("activate", "Drop table"), Some("drop"));
        assert_eq!(would("activate", "Open dropdown"), None);
        assert_eq!(would("activate", "Checkout"), None);
        assert_eq!(would("enter_text", "Send to"), None);
        assert_eq!(would("wait_for", "Order sent"), None);
        assert_eq!(would("inspect", "Remove"), None);
    }
}

#[cfg(test)]
mod caption_tests {
    /// #497 review: a long control name must not end a watch at the act that names it. The
    /// caption stays within the driver's bound, on a character boundary, for ASCII and for
    /// multi-byte names. Cost: microseconds.
    #[test]
    fn a_long_caption_is_cut_within_the_driver_bound_on_a_char_boundary() {
        for name in ["x".repeat(256), "ç".repeat(128), "日本".repeat(43)] {
            let caption = super::caption_bounded(format!("cart.checkout: Clicks \"{name}\""));
            assert!(
                caption.len() <= super::CAPTION_LIMIT,
                "{} bytes",
                caption.len()
            );
            assert!(caption.ends_with('…'));
        }
        assert_eq!(super::caption_bounded("short".into()), "short");
    }

    /// #505: the watch row carries the caption of the act about to run, and null between acts,
    /// so the Studio can show the same words as the watch browser. Cost: one temp file.
    #[test]
    fn the_watch_row_carries_the_caption_of_the_act_about_to_run() {
        let dir = tempfile::tempdir().unwrap();
        let progress = super::Progress {
            dir: dir.path().to_path_buf(),
            id: "live-0123456789abcdef".into(),
            base: serde_json::json!({"sessionId": "live-0123456789abcdef", "mode": "watch"}),
        };
        let row = || -> serde_json::Value {
            serde_json::from_slice(
                &std::fs::read(dir.path().join("live-0123456789abcdef.json")).unwrap(),
            )
            .unwrap()
        };
        progress.show(
            "run",
            0,
            Some("run.details"),
            Some(0),
            Some("run.details: Clicks \"Details\""),
        );
        assert_eq!(row()["caption"], "run.details: Clicks \"Details\"");
        progress.show("bot", 1, None, None, None);
        assert_eq!(row()["caption"], serde_json::Value::Null);
    }
}

#[cfg(test)]
mod frame_tests {
    use super::{FrameRead, SESSIONS, TemporaryOutput, read_frame};
    use serde_json::json;

    const ID: &str = "live-0123456789abcdef";

    fn session(project: &std::path::Path, frame_dir: serde_json::Value, minutes: i64) {
        let dir = project.join(SESSIONS);
        std::fs::create_dir_all(&dir).unwrap();
        let expires = chrono::Utc::now() + chrono::Duration::minutes(minutes);
        let record = json!({"sessionId":ID,"frame":!frame_dir.is_null(),"frameDir":frame_dir,
            "expiresAt":expires.to_rfc3339()});
        std::fs::write(dir.join(format!("{ID}.json")), record.to_string()).unwrap();
    }

    fn frame(dir: &std::path::Path, seq: u64) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("frame.jpg"), [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        std::fs::write(
            dir.join("frame.json"),
            json!({"seq":seq,"width":1280,"height":800}).to_string(),
        )
        .unwrap();
    }

    /// #519: the frame route serves the latest frame of a live streamed watch, nothing before
    /// it exists, and nothing once the session is gone. Cost: milliseconds, temp files only.
    #[test]
    fn a_streamed_watch_serves_its_latest_frame_and_nothing_once_gone() {
        let project = tempfile::tempdir().unwrap();
        assert_eq!(read_frame(project.path(), ID), FrameRead::Gone);
        assert_eq!(read_frame(project.path(), "../escape"), FrameRead::Gone);

        let output = TemporaryOutput::create().unwrap();
        let screencast = output.path().join("screencast");
        session(project.path(), json!(screencast.to_string_lossy()), 5);
        assert_eq!(read_frame(project.path(), ID), FrameRead::Pending);

        frame(&screencast, 7);
        assert_eq!(
            read_frame(project.path(), ID),
            FrameRead::Frame {
                seq: 7,
                width: 1280,
                height: 800,
                jpeg: vec![0xFF, 0xD8, 0xFF, 0xD9]
            }
        );

        session(project.path(), json!(screencast.to_string_lossy()), -1);
        assert_eq!(read_frame(project.path(), ID), FrameRead::Gone);

        session(project.path(), serde_json::Value::Null, 5);
        assert_eq!(read_frame(project.path(), ID), FrameRead::Pending);
    }

    /// #559 review: a junction in the temp directory named like a replay output, pointing at a
    /// directory that holds a frame, is not read: its canonical path is not the recorded one.
    /// Cost: milliseconds, temp files only. Windows only (junctions).
    #[cfg(windows)]
    #[test]
    fn a_junction_named_like_a_replay_output_reads_nothing() {
        let project = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        frame(&target.path().join("screencast"), 1);
        let temp = std::env::temp_dir().canonicalize().unwrap();
        let link = temp.join(format!(
            "graphhelm-replay-{}",
            uuid::Uuid::new_v4().simple()
        ));
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link.to_string_lossy().trim_start_matches(r"\\?\"))
            .arg(target.path())
            .output()
            .unwrap();
        assert!(
            made.status.success(),
            "ARRANGEMENT: junction made: {made:?}"
        );
        session(
            project.path(),
            json!(link.join("screencast").to_string_lossy()),
            5,
        );
        let read = read_frame(project.path(), ID);
        let _ = std::fs::remove_dir(&link);
        assert_eq!(read, FrameRead::Pending);
    }

    /// #519: the record names the frame directory, but only a `screencast` directly inside one of
    /// this machine's replay outputs is read; a record edited to point anywhere else gets no
    /// bytes. Cost: milliseconds, temp files only.
    #[test]
    fn a_record_pointing_outside_a_replay_output_reads_nothing() {
        let project = tempfile::tempdir().unwrap();
        let elsewhere = project.path().join("screencast");
        frame(&elsewhere, 1);
        session(project.path(), json!(elsewhere.to_string_lossy()), 5);
        assert_eq!(read_frame(project.path(), ID), FrameRead::Pending);

        let output = TemporaryOutput::create().unwrap();
        let wrong_name = output.path().join("other");
        frame(&wrong_name, 1);
        session(project.path(), json!(wrong_name.to_string_lossy()), 5);
        assert_eq!(read_frame(project.path(), ID), FrameRead::Pending);
    }
}
