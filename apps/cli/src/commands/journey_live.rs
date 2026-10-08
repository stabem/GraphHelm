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
};
use crate::output::{CommandOutput, Outcome};

const OPEN: &str = "journey.open";
const ACT: &str = "journey.act";
const CLOSE: &str = "journey.close";
const HANDSHAKE: &[u8] = b"{\"protocol\":\"graphhelm-live-host/1\",\"start\":true}\n";
const SESSIONS: &str = ".graphhelm/journey-sessions";
const FRAME: usize = 64 * 1024;
/// Accessible names an act on a live session may not target unless the approved flow contains
/// that act on an edge leaving the current screen (journey-explore design §11).
const DESTRUCTIVE: [&str; 6] = ["delete", "remove", "pay", "purchase", "transfer", "send"];
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
    }
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
    command
        .args(["--json", "journey", "open", &args.id, "--step", &args.step])
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
    let Ok(Ok(output)) = rx.recv_timeout(RUN_BUDGET + Duration::from_secs(5)) else {
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
            command: OPEN,
            data: Some(envelope["data"].clone()),
            diagnostics,
        },
        exit_code: output.status.code().unwrap_or(1),
    }
}

fn open_with(args: &JourneyOpenArgs) -> Outcome {
    let data = json!({"flowId":args.id,"step":args.step,"path":null,"sessionId":null,"state":"unobserved","at":null,"headed":true,"modelCalls":0,"liveCaptureSignalId":null});
    if !graphhelm_execution::valid_journey_id(&args.id) {
        return report(OPEN, data, Some(failure("replay.id_invalid", "/id", 3)));
    }
    if !graphhelm_execution::valid_journey_id(&args.step)
        || args
            .path
            .as_deref()
            .is_some_and(|p| !graphhelm_execution::valid_journey_id(p))
    {
        return report(OPEN, data, Some(failure("replay.id_invalid", "/step", 3)));
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
    let line = match rx.recv_timeout(RUN_BUDGET) {
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
            command: OPEN,
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
    let (mut session, outcome) = match walk(args, &mut data) {
        Ok(walked) => walked,
        Err(failed) => emit(report(OPEN, data, Some(failed))),
    };
    // One envelope line, then the host serves. Nothing else is ever written to stdout.
    {
        let mut line =
            serde_json::to_value(&report(OPEN, data, outcome).output).unwrap_or(Value::Null);
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
    let flow = super::journey_flow::read_for_replay(&file, &project).map_err(|findings| {
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
        .find(|(_, path_edges)| visited_of(path_edges).contains(&args.step))
        .ok_or_else(|| failure("live.step_unreachable", "/step", 2))?;
    let visited = visited_of(path_edges);
    let target = visited.iter().position(|s| *s == args.step).unwrap();
    data["path"] = name.as_str().into();
    let contract = if name == "main" {
        args.id.clone()
    } else {
        format!("{}.{}", args.id, name)
    };

    // The walk is deterministic only through a current cache: absent or void, refuse.
    let cache_file = project
        .join(".graphhelm/journey-cache")
        .join(format!("{}.json", args.id));
    let cache = load_cache(&cache_file, &flow)?
        .ok_or_else(|| failure("replay.cache_missing", "/cache", 2))?;
    if cache["flowDigest"] != super::journey_flow::approval_digest(&flow) {
        return Err(failure("replay.cache_missing", "/cache/flowDigest", 2));
    }
    let secrets = preflight(&flow)?;
    observer_ready(&project)?;
    let output = TemporaryOutput::create()?;
    let mut driver = Driver::start(&project, output.path(), &secrets)?;
    let base = flow["base"].as_str().unwrap().to_owned();
    let entry = format!(
        "{}{}",
        base.trim_end_matches('/'),
        screens[&visited[0]]["url"].as_str().unwrap()
    );
    driver.call(
        "open",
        json!({"base":entry,"viewport":cache["viewport"],"allowOrigins":args.allow_origin,"headed":true}),
        "/entry",
    )?;
    let recording = args.events.is_some().then(|| replay_args(args, &project));
    let mut current = Some(visited[0].clone());
    let mut step_failure = None;
    for (index, screen_id) in visited.iter().enumerate().take(target + 1) {
        if index > 0 {
            let edge_id = path_edges[index - 1].as_str().unwrap();
            let edge = &edges[edge_id];
            for (act_index, act) in edge["acts"].as_array().unwrap().iter().enumerate() {
                let at = format!("{edge_id}/{act_index}");
                let mut request = json!({"kind":act["kind"],"role":act["role"],"name":act["name"],
                    "locator":cache["edges"][edge_id][act_index]});
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
            Ok(_) => current = Some(screen_id.clone()),
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
        let image = format!("live-{}.png", args.step);
        driver.call(
            "capture",
            json!({"path":image,"maskSecrets":true}),
            "/capture",
        )?;
        let signal = record(
            recording,
            &contract,
            &args.step,
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
    )?;
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
) -> Result<Session> {
    let dir = project.join(SESSIONS);
    safe_directory(&dir, true)?;
    sweep_expired(&dir);
    let id = format!("live-{}", &uuid::Uuid::new_v4().simple().to_string()[..16]);
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
        "stepId":args.step,"state":data["state"],"code":data["code"],"at":data["at"],
        "screen":current,"since":chrono::Utc::now().to_rfc3339(),"lastActAt":null,"lastActState":null,"lastActCode":null,
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
