//! `journey preview` (#519): opening a journey in the Studio runs it, so the owner sees each screen
//! as it really renders and whether each step passes, before deciding anything.
//!
//! One headless run plays every path of the flow from its entry, and at each screen records the
//! result (`pass`, `fail`, `drift`), the reason, and a frame (a masked screenshot, the same capture
//! replay takes). The result is kept per flow, keyed by the flow's digest and the project's commit,
//! so opening the journey again shows it without a new run; a changed flow or a new commit makes it
//! stale, and `--force` (the Studio's Run again) runs it anew.
//!
//! A draft's preview is never proof: it records no capture signal, walks no arrow and writes no
//! cache. An approved flow's run is the real replay (#519 slice 3): when the Runtime names an
//! execution, each screen reached is recorded as a capture and each arrow as walked, exactly as
//! `journey replay` records them. Drafts are played too; an act that would destroy something on a draft is not sent unless the
//! preview started the app under test itself (#515's rule, `would_destroy`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::journey_live::{
    Launched, LaunchedStop, base_reachable, launch, launcher_isolated, would_destroy,
};
use super::journey_replay::{
    Driver, Failure, Result, SURVIVABLE, TemporaryOutput, declared_browser, failure, observe,
    observer_ready, preflight, record, safe_directory, safe_node, walked, with_storage,
};
use crate::args::{JourneyPreviewArgs, JourneyReplayArgs};
use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "journey.preview";
const PREVIEWS: &str = ".graphhelm/journey-previews";
const STATE: &str = "state.json";
/// The page size a preview plays at unless the flow declares one (#585); the frames the Studio
/// shows are this size.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
/// No path of a preview BEGINS after this from its start; the path in flight is bounded by the
/// driver's own per-call timeouts. A `running` state older than this plus `RUNNER_GRACE` has no
/// live runner, whatever its pid now names.
const PREVIEW_BUDGET: Duration = Duration::from_secs(300);
/// The largest frame the route serves.
const FRAME_LIMIT: u64 = 8 * 1024 * 1024;

fn project_of(args: &JourneyPreviewArgs) -> std::result::Result<PathBuf, Failure> {
    args.project
        .clone()
        .unwrap_or_else(|| PathBuf::from("."))
        .canonicalize()
        .map_err(|_| failure("preview.project_invalid", "/project", 2))
}

/// A flow id or screen id used as a file name: the schema's own identifier characters only.
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn dir_of(project: &Path, id: &str) -> PathBuf {
    project.join(PREVIEWS).join(id)
}

/// The flow as the preview plays it, and the key its result is kept under.
fn flow_and_key(project: &Path, id: &str) -> Result<(Value, String, String)> {
    if !plain_id(id) {
        return Err(failure("preview.flow_unknown", "/id", 2));
    }
    let file = project
        .join(".graphhelm/journeys")
        .join(format!("{id}.journey.yaml"));
    if !safe_node(&file) {
        return Err(failure("preview.flow_unknown", "/id", 2));
    }
    let flow = super::journey_flow::read_for_watch(&file, project).map_err(|findings| {
        let finding = findings.into_iter().find(|f| !f.is_warning()).unwrap();
        failure(finding.code, finding.pointer, 2)
    })?;
    let digest = super::journey_flow::approval_digest(&flow);
    Ok((flow, digest, commit_of(project)))
}

/// The project's commit, part of the key: the same flow on new code is a new preview.
fn commit_of(project: &Path) -> String {
    Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "HEAD"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_owned())
        .filter(|sha| sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".to_owned())
}

fn read_state(dir: &Path) -> Option<Value> {
    let file = dir.join(STATE);
    if !safe_node(&file) {
        return None;
    }
    std::fs::read(file)
        .ok()
        .filter(|bytes| bytes.len() <= 1024 * 1024)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// The stored state as a reader judges it (#586), from `read` (the state file). A runner writes
/// its last state and only then exits, so a reader that saw `running` and then finds the runner
/// gone reads once more: what is stored now is what the runner left (`ready`, `failed`), and only
/// a state still `running` belongs to a runner that died. Without the second read a run that
/// finished between the read and the liveness check answered `failed / internal`.
fn read_settled(mut read: impl FnMut() -> Option<Value>) -> Option<Value> {
    let state = read()?;
    if state["state"] == "running" && !runner_alive(&state) {
        return read();
    }
    Some(state)
}

fn save_state(dir: &Path, state: &Value) {
    let mut bytes = serde_json::to_vec_pretty(state).unwrap();
    bytes.push(b'\n');
    let _ = super::journey_flow::atomic_write(&dir.join(STATE), &bytes);
}

/// Whether a `running` state still has a runner. The runner writes its own pid when it starts;
/// before that, a just-started run counts as alive for `RUNNER_GRACE`. Past the budget plus that
/// grace no runner is alive, even when the pid now names another process (#560 review: a hard
/// kill, then pid reuse, would otherwise read `running` for ever).
fn runner_alive(state: &Value) -> bool {
    if !within_budget(state) {
        return false;
    }
    match state["pid"].as_u64() {
        Some(pid) => alive(pid),
        None => younger_than(state, RUNNER_GRACE),
    }
}

/// Whether a run started less than its budget plus the grace ago.
fn within_budget(state: &Value) -> bool {
    younger_than(state, PREVIEW_BUDGET + RUNNER_GRACE)
}

fn younger_than(state: &Value, limit: Duration) -> bool {
    state["startedAt"]
        .as_str()
        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .is_some_and(|at| {
            chrono::Utc::now().signed_duration_since(at)
                < chrono::Duration::from_std(limit).unwrap()
        })
}

/// How long a started run may take to write its own pid.
const RUNNER_GRACE: Duration = Duration::from_secs(60);

fn alive(pid: u64) -> bool {
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .is_some_and(|text| text.contains(&format!("\"{pid}\"")))
    }
    #[cfg(not(windows))]
    {
        Path::new(&format!("/proc/{pid}")).exists()
    }
}

/// The body every preview answer carries: the stored run when it is for this flow and commit,
/// else `state: none`. Private fields (the runner's pid, frame files) never leave this module.
fn body(state: Option<Value>, flow: &Value, digest: &str, commit: &str) -> Value {
    let kind = if flow["status"] == "approved" {
        "replay"
    } else {
        "preview"
    };
    let none = json!({"preview":true,"kind":kind,"digest":digest,"commit":commit,"state":"none",
        "screens":{},"edges":{}});
    let Some(mut state) = state else { return none };
    if state["digest"] != digest || state["commit"] != commit {
        return none;
    }
    // The kind follows the flow as it is now: approving it turns the same run into a replay's.
    state["kind"] = kind.into();
    if state["state"] == "running" && !runner_alive(&state) {
        // A runner that died without finishing never reports; say so instead of "running" forever.
        // Past its budget the run did not finish in time, whatever became of the runner (#560
        // review): its own watchdog ends it there.
        state["state"] = "failed".into();
        state["reason"] = if within_budget(&state) {
            "internal"
        } else {
            "preview.budget_exceeded"
        }
        .into();
    }
    if let Some(object) = state.as_object_mut() {
        object.remove("pid");
        object.remove("launched");
        if let Some(screens) = object.get_mut("screens").and_then(Value::as_object_mut) {
            for screen in screens.values_mut() {
                if let Some(screen) = screen.as_object_mut() {
                    screen.remove("file");
                }
            }
        }
    }
    state
}

fn answer(data: Value, failed: Option<Failure>) -> Outcome {
    let Some((code, path, exit_code)) = failed else {
        return Outcome::success(COMMAND, data);
    };
    let message = match code {
        "preview.flow_unknown" => "no journey flow with this id",
        "preview.project_invalid" => "the project directory cannot be read",
        "preview.keyring_missing" => {
            "recording an approved flow's proof requires the Runtime's sealed keyring"
        }
        _ => "the journey flow cannot be previewed",
    };
    Outcome {
        output: CommandOutput {
            ok: false,
            command: COMMAND,
            data: Some(data),
            diagnostics: vec![graphhelm_protocols::Diagnostic::error(
                code,
                message,
                path,
                "journey-cli",
            )],
        },
        exit_code,
    }
}

/// `GET /v1/journey-flows/{id}/preview`: the stored preview for this flow and commit, if any.
pub(crate) fn status(args: &JourneyPreviewArgs) -> Outcome {
    let project = match project_of(args) {
        Ok(project) => project,
        Err(failed) => return answer(json!({}), Some(failed)),
    };
    match flow_and_key(&project, &args.id) {
        Ok((flow, digest, commit)) => answer(
            body(
                reap_lost(&project, &dir_of(&project, &args.id)),
                &flow,
                &digest,
                &commit,
            ),
            None,
        ),
        Err(failed) => answer(json!({}), Some(failed)),
    }
}

/// `journey preview <id>` / `POST /v1/journey-flows/{id}/preview`: answer the stored preview when
/// it is current, else start one in the background and answer `running`. `--force` runs anew.
pub(crate) fn start(args: &JourneyPreviewArgs) -> Outcome {
    if args.run {
        return run(args);
    }
    let project = match project_of(args) {
        Ok(project) => project,
        Err(failed) => return answer(json!({}), Some(failed)),
    };
    let (flow, digest, commit) = match flow_and_key(&project, &args.id) {
        Ok(found) => found,
        Err(failed) => return answer(json!({}), Some(failed)),
    };
    let dir = dir_of(&project, &args.id);
    let stored = reap_lost(&project, &dir);
    let running_now = stored
        .as_ref()
        .is_some_and(|state| state["state"] == "running" && runner_alive(state));
    let current = body(stored.clone(), &flow, &digest, &commit);
    if running_now {
        if current["state"] == "running" {
            return answer(current, None);
        }
        // A run for an older flow or commit is still going: one runner per flow at a time.
        let mut busy = current;
        busy["state"] = "failed".into();
        busy["reason"] = "preview.busy".into();
        return answer(busy, None);
    }
    if !args.force && !args.confirm && matches!(current["state"].as_str(), Some("ready" | "failed"))
    {
        return answer(current, None);
    }
    if unsealed_proof(args, &flow) {
        return answer(
            current,
            Some(failure("preview.keyring_missing", "/keyring", 2)),
        );
    }
    if safe_directory(&project.join(".graphhelm"), false).is_err()
        || std::fs::create_dir_all(&dir).is_err()
        || safe_directory(&dir, false).is_err()
    {
        return answer(current, Some(failure("preview.unwritable", "/preview", 1)));
    }
    let _ = std::fs::remove_dir_all(dir.join("frames"));
    let Ok(executable) = std::env::current_exe() else {
        return answer(current, Some(failure("preview.unwritable", "/runner", 1)));
    };
    let mut command = Command::new(executable);
    command
        .args([
            "--json",
            "journey",
            "preview",
            &args.id,
            "--run",
            "--project",
        ])
        .arg(&project);
    if args.confirm {
        command.arg("--confirm");
    }
    if let Some(proof) = proof_args(args, &flow, &project) {
        command
            .arg("--events")
            .arg(proof.events.as_ref().unwrap())
            .args(["--execution", proof.execution.as_deref().unwrap()])
            .arg("--keyring")
            .arg(proof.keyring.as_ref().unwrap())
            .args(["--key-id", proof.key_id.as_deref().unwrap()]);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (key, value) in std::env::vars_os()
        .filter(|(key, _)| key.to_string_lossy().starts_with("GRAPHHELM_SECRET_"))
    {
        command.env(key, value);
    }
    // The running state is written before the runner starts and never after: a runner that ends
    // quickly (no observer, app down) must not have its result overwritten by this answer. The
    // runner writes its own pid.
    let running = json!({"preview":true,"kind":current["kind"],"digest":digest,"commit":commit,
        "state":"running","startedAt":chrono::Utc::now().to_rfc3339(),"current":null,
        "screens":{},"edges":{}});
    save_state(&dir, &running);
    match command.spawn() {
        Ok(_) => answer(body(Some(running), &flow, &digest, &commit), None),
        Err(_) => {
            let _ = std::fs::remove_file(dir.join(STATE));
            answer(current, Some(failure("preview.unwritable", "/runner", 1)))
        }
    }
}

/// The budget watchdog (#560 review): a thread that, at `deadline`, records `failed /
/// preview.budget_exceeded` (only while the state is still this runner's `running` run), stops
/// the app the run launched, and then calls `end` (in the runner: exit the process). Nothing
/// the main thread is blocked on (a launcher, a browser call) can keep the run past its budget.
fn watch_budget(
    dir: PathBuf,
    pid: u32,
    deadline: Instant,
    launched: std::sync::Arc<std::sync::Mutex<Option<LaunchedStop>>>,
    end: impl FnOnce() + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        let Some(mut state) = read_state(&dir) else {
            return;
        };
        if state["state"] != "running" || state["pid"].as_u64() != Some(u64::from(pid)) {
            return;
        }
        state["state"] = "failed".into();
        state["reason"] = "preview.budget_exceeded".into();
        state["current"] = Value::Null;
        state["ranAt"] = chrono::Utc::now().to_rfc3339().into();
        save_state(&dir, &state);
        if let Some(stop) = launched.lock().ok().and_then(|mut slot| slot.take()) {
            stop.stop();
        }
        end();
    })
}

/// The stored state, after stopping the app of a run whose runner is gone (#593 review: a killed
/// runner left its isolated fixture running, and every new preview launches another). A state
/// still `running` whose runner is dead and that names a launched app: run that app's `down`,
/// then store the run as `failed / preview.runner_lost` (`preview.budget_exceeded` past the
/// budget) without the record, so it is stopped once.
fn reap_lost(project: &Path, dir: &Path) -> Option<Value> {
    reap_lost_from(project, dir, || read_state(dir))
}

/// [`reap_lost`] from `read`, the state file's reads in order (a test supplies them).
fn reap_lost_from(
    project: &Path,
    dir: &Path,
    read: impl FnMut() -> Option<Value>,
) -> Option<Value> {
    // Settled first (#586): a run that finished between the read and the liveness check is read
    // as it finished, and is never reaped.
    let mut state = read_settled(read)?;
    if state["state"] != "running" || runner_alive(&state) || state["launched"].is_null() {
        return Some(state);
    }
    if let Some(stop) = LaunchedStop::from_record(project, &state["launched"]) {
        stop.stop();
    }
    let reason = if within_budget(&state) {
        "preview.runner_lost"
    } else {
        "preview.budget_exceeded"
    };
    state["state"] = "failed".into();
    state["reason"] = reason.into();
    state["current"] = Value::Null;
    if let Some(object) = state.as_object_mut() {
        object.remove("launched");
    }
    save_state(dir, &state);
    Some(state)
}

/// The top-level `reason` a failed preview carries (the closed list agreed on #519).
fn failed_reason(code: &str) -> &'static str {
    match code {
        "watch.app_down" => "watch.app_down",
        "watch.launch_failed" => "watch.launch_failed",
        "watch.launcher_invalid" => "watch.launcher_invalid",
        "driver.observer_missing" | "replay.observer_missing" => "driver.observer_missing",
        "preview.budget_exceeded" => "preview.budget_exceeded",
        // #586: what `preflight` refuses before any browser starts keeps its own code, so the
        // owner reads which thing is missing or wrong in the flow.
        "driver.secret_missing" => "driver.secret_missing",
        "driver.secret_literal" => "driver.secret_literal",
        "driver.unsupported_act" => "driver.unsupported_act",
        "replay.act_value_missing" => "replay.act_value_missing",
        "replay.entry_missing" => "replay.entry_missing",
        "preview.runner_lost" => "preview.runner_lost",
        _ => "internal",
    }
}

/// What a refused act or observation means for one step (the closed list agreed on #519).
fn step_reason(code: &str) -> &'static str {
    match code {
        "driver.timeout" | "replay.timeout" => "timeout",
        "driver.expectation_failed" | "driver.locator_missing" | "driver.locator_ambiguous" => {
            "expect_missing"
        }
        "replay.wrong_screen" => "page_error",
        _ => "act_failed",
    }
}

/// How bad a result is; a screen reached on several paths keeps its worst.
fn severity(result: &str) -> u8 {
    match result {
        "fail" => 3,
        "drift" => 2,
        "pass" => 1,
        _ => 0,
    }
}

struct Run {
    dir: PathBuf,
    state: Value,
    frames: usize,
    /// Set only for an approved flow with an execution to record into (slice 3).
    proof: Option<JourneyReplayArgs>,
    /// The path being played: its contract id and the last capture recorded on it.
    contract: String,
    last: Option<(String, String)>,
    /// The owner clicked "Run it?" for this run: an approved flow's destructive acts are played.
    confirmed: bool,
    /// #585: the page size and seeded localStorage the flow declares (else the preview's size).
    viewport: Value,
    storage: Option<Value>,
}

/// An approved flow asked to record into an execution on a Runtime with no sealed keyring: refused,
/// as `journey open` refuses it. A draft never records, so its execution is ignored, not refused.
fn unsealed_proof(args: &JourneyPreviewArgs, flow: &Value) -> bool {
    flow["status"] == "approved"
        && args.execution.is_some()
        && (args.keyring.is_none() || args.key_id.is_none())
}

/// The replay arguments an approved flow's run records with, or `None` (a draft, or no execution
/// named: all four recording arguments or none, as replay requires).
fn proof_args(
    args: &JourneyPreviewArgs,
    flow: &Value,
    project: &Path,
) -> Option<JourneyReplayArgs> {
    if flow["status"] != "approved" {
        return None;
    }
    Some(JourneyReplayArgs {
        id: args.id.clone(),
        project: Some(project.to_path_buf()),
        events: Some(args.events.clone()?),
        execution: Some(args.execution.clone()?),
        keyring: Some(args.keyring.clone()?),
        key_id: Some(args.key_id.clone()?),
        allow_origin: Vec::new(),
        replay_worker: false,
        heal: false,
        model: Default::default(),
        allow_act: Vec::new(),
    })
}

impl Run {
    /// Slice 3: an approved flow's screen, reached and observed, is recorded as a capture of its
    /// frame, and the arrow from the previous one on this path as walked (replay's order: the
    /// capture commits before the walk).
    fn prove(&mut self, screen: &str, frame: Option<&Value>) -> Result<()> {
        let Some(args) = self.proof.as_ref() else {
            return Ok(());
        };
        let Some(file) = frame.and_then(|frame| frame["file"].as_str()) else {
            return Err(failure(
                "replay.record_failed",
                format!("/screens/{screen}/capture"),
                1,
            ));
        };
        let image = self.dir.join("frames").join(file);
        let signal = record(args, &self.contract, screen, &image, None)?;
        if let Some((from, from_capture)) = &self.last {
            walked(args, &self.contract, from, screen, from_capture, &signal)?;
        }
        self.last = Some((screen.to_owned(), signal));
        Ok(())
    }

    fn save(&self) {
        save_state(&self.dir, &self.state);
    }

    /// The masked frame of the page as it is now, kept for `screen`. `None` when the page cannot
    /// be captured (the result still stands; the card shows no frame).
    fn capture(&mut self, driver: &mut Driver, output: &Path, screen: &str) -> Option<Value> {
        self.frames += 1;
        let name = format!("frame-{}.png", self.frames);
        let shot = driver
            .call(
                "capture",
                json!({"path":name,"maskSecrets":true}),
                &format!("/screens/{screen}/capture"),
            )
            .ok()?;
        let frames = self.dir.join("frames");
        std::fs::create_dir_all(&frames).ok()?;
        let file = format!("{screen}.png");
        std::fs::copy(output.join(&name), frames.join(&file)).ok()?;
        Some(json!({"frame":true,"width":shot["width"],"height":shot["height"],"file":file}))
    }

    /// Records one screen's result, keeping the worst one a screen got on any path.
    fn screen(&mut self, id: &str, result: &str, reason: Option<&str>, frame: Option<Value>) {
        let previous = self.state["screens"][id]["result"]
            .as_str()
            .map_or(0, severity);
        if previous >= severity(result) && previous > 0 {
            return;
        }
        let mut entry = frame.unwrap_or_else(|| json!({"frame":false}));
        entry["result"] = result.into();
        if let Some(reason) = reason {
            entry["reason"] = reason.into();
        }
        self.state["screens"][id] = entry;
        self.state["current"] = id.into();
        self.save();
    }

    fn edge(&mut self, id: &str, result: &str, reason: Option<&str>) {
        let previous = self.state["edges"][id]["result"]
            .as_str()
            .map_or(0, severity);
        if previous >= severity(result) && previous > 0 {
            return;
        }
        let mut entry = json!({"result":result});
        if let Some(reason) = reason {
            entry["reason"] = reason.into();
        }
        self.state["edges"][id] = entry;
    }
}

/// The background runner: plays every path, records each screen and edge, then `ready`.
fn run(args: &JourneyPreviewArgs) -> Outcome {
    let project = match project_of(args) {
        Ok(project) => project,
        Err(failed) => return answer(json!({}), Some(failed)),
    };
    let (flow, digest, commit) = match flow_and_key(&project, &args.id) {
        Ok(found) => found,
        Err(failed) => return answer(json!({}), Some(failed)),
    };
    let dir = dir_of(&project, &args.id);
    let mut state = read_state(&dir).unwrap_or_else(|| json!({}));
    if state["digest"] != digest.as_str() || state["state"] != "running" {
        state = json!({"preview":true,"digest":digest,"commit":commit,"state":"running",
            "startedAt":chrono::Utc::now().to_rfc3339(),"screens":{},"edges":{}});
    }
    state["kind"] = if flow["status"] == "approved" {
        "replay"
    } else {
        "preview"
    }
    .into();
    state["pid"] = std::process::id().into();
    if let Some(object) = state.as_object_mut() {
        object.remove("held");
    }
    let mut run = Run {
        dir,
        state,
        frames: 0,
        proof: proof_args(args, &flow, &project),
        contract: String::new(),
        last: None,
        confirmed: args.confirm,
        viewport: Value::Null,
        storage: None,
    };
    (run.viewport, run.storage) = declared_browser(&flow, json!({"width":WIDTH,"height":HEIGHT}));
    run.save();
    let deadline = Instant::now() + PREVIEW_BUDGET;
    let launched = std::sync::Arc::new(std::sync::Mutex::new(None));
    watch_budget(
        run.dir.clone(),
        std::process::id(),
        deadline,
        launched.clone(),
        || std::process::exit(3),
    );
    let outcome = play(&project, &flow, &mut run, deadline, &launched);
    // Every screen and edge no path reached says so; the run's result is its worst step.
    for screen in flow["screens"].as_array().unwrap() {
        let id = screen["id"].as_str().unwrap();
        if run.state["screens"][id].is_null() {
            run.state["screens"][id] = json!({"frame":false,"reason":"not_reached"});
        }
    }
    for edge in flow["edges"].as_array().unwrap() {
        let id = edge["id"].as_str().unwrap();
        if run.state["edges"][id].is_null() {
            run.state["edges"][id] = json!({"result":null,"reason":"not_reached"});
        }
    }
    let worst = run.state["screens"]
        .as_object()
        .unwrap()
        .values()
        .chain(run.state["edges"].as_object().unwrap().values())
        .filter_map(|entry| entry["result"].as_str())
        .filter(|result| severity(result) > 0)
        .max_by_key(|result| severity(result))
        .unwrap_or("pass")
        .to_owned();
    run.state["current"] = Value::Null;
    run.state["ranAt"] = chrono::Utc::now().to_rfc3339().into();
    if let Some(object) = run.state.as_object_mut() {
        object.remove("launched");
    }
    match outcome {
        Ok(()) => {
            run.state["state"] = "ready".into();
            run.state["result"] = worst.into();
        }
        Err((code, _, _)) => {
            run.state["state"] = "failed".into();
            run.state["reason"] = failed_reason(code).into();
        }
    }
    run.save();
    answer(json!({"state":run.state["state"]}), None)
}

fn play(
    project: &Path,
    flow: &Value,
    run: &mut Run,
    deadline: Instant,
    stopper: &std::sync::Arc<std::sync::Mutex<Option<LaunchedStop>>>,
) -> Result<()> {
    let secrets = preflight(flow)?;
    observer_ready(project)?;
    let mut base = flow["base"].as_str().unwrap().to_owned();
    // The app under test is started when it is down, and stopped when the preview ends. An
    // isolated launcher always starts its own on free ports (#585), and the preview plays there.
    let launched: Option<Launched> = if !launcher_isolated(project) && base_reachable(&base) {
        None
    } else {
        Some(launch(project, &base)?)
    };
    if let Some(own) = launched.as_ref().and_then(|app| app.base.clone()) {
        base = own;
    }
    if let (Some(app), Ok(mut slot)) = (&launched, stopper.lock()) {
        *slot = Some(app.stopper());
    }
    // #593 review: the state names the app this run launched, so a reader that finds the runner
    // gone can still stop it (a killed runner runs neither its own stop nor its watchdog).
    if let Some(app) = &launched {
        run.state["launched"] = app.stopper().record();
        run.save();
    }
    let guard = Guard {
        approved: flow["status"] == "approved",
        launched: launched.is_some(),
        confirmed: run.confirmed,
    };
    let screens: std::collections::BTreeMap<&str, &Value> = flow["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|screen| (screen["id"].as_str().unwrap(), screen))
        .collect();
    let edges: std::collections::BTreeMap<&str, &Value> = flow["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| (edge["id"].as_str().unwrap(), edge))
        .collect();
    let mut paths: Vec<_> = flow["paths"].as_object().unwrap().iter().collect();
    paths.sort_by(|(a, _), (b, _)| (a.as_str() != "main", a).cmp(&(b.as_str() != "main", b)));
    for (name, path_edges) in paths {
        // Replay's contract id: the flow for `main`, `<flow>.<path>` for any other path.
        let id = run.proof.as_ref().map_or("", |proof| proof.id.as_str());
        run.contract = if name == "main" {
            id.to_owned()
        } else {
            format!("{id}.{name}")
        };
        run.last = None;
        if Instant::now() >= deadline {
            return Err(failure("preview.budget_exceeded", "/paths", 1));
        }
        let ids: Vec<&str> = path_edges
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect();
        play_path(project, &base, &secrets, &screens, &edges, &ids, guard, run)?;
        if !run.state["held"].is_null() {
            // Waiting for the owner's click: nothing after the held act is played.
            break;
        }
    }
    if let Ok(mut slot) = stopper.lock() {
        slot.take();
    }
    drop(launched);
    Ok(())
}

/// Which acts a run plays. A draft keeps #515's rule: an act that would destroy something is not
/// sent unless the preview started the app itself. An approved flow (#519, coordinator's decision)
/// stops before its first destructive act on any base and waits for the owner's click, which comes
/// back as `--confirm`; only that run plays it.
#[derive(Clone, Copy)]
struct Guard {
    approved: bool,
    launched: bool,
    confirmed: bool,
}

impl Guard {
    /// The step reason an act is not sent with, or `None` when it is played.
    fn stops_before(self, act: &Value) -> Option<&'static str> {
        would_destroy(act)?;
        if self.approved {
            (!self.confirmed).then_some("confirm_needed")
        } else {
            (!self.launched).then_some("guard_refused")
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn play_path(
    project: &Path,
    base: &str,
    secrets: &std::collections::BTreeMap<String, String>,
    screens: &std::collections::BTreeMap<&str, &Value>,
    edges: &std::collections::BTreeMap<&str, &Value>,
    ids: &[&str],
    guard: Guard,
    run: &mut Run,
) -> Result<()> {
    let output = TemporaryOutput::create()?;
    let mut driver = Driver::start(project, output.path(), secrets)?;
    let first = edges[ids[0]]["from"].as_str().unwrap();
    let entry = format!(
        "{}{}",
        base.trim_end_matches('/'),
        screens[first]["url"].as_str().unwrap()
    );
    // `survive` keeps the page after a step fails, so the failure's frame can be taken.
    driver.call(
        "open",
        with_storage(
            json!({"base":entry,"viewport":run.viewport,"allowOrigins":[],"survive":true}),
            run.storage.as_ref(),
        ),
        "/entry",
    )?;
    let arrived = |driver: &mut Driver, run: &mut Run, screen: &str| -> Result<bool> {
        match observe(driver, screens[screen], base, &format!("/screens/{screen}")) {
            Ok(_) => {
                let frame = run.capture(driver, output.path(), screen);
                run.prove(screen, frame.as_ref())?;
                run.screen(screen, "pass", None, frame);
                Ok(true)
            }
            Err((code, _, _)) if SURVIVABLE.contains(&code) || code == "replay.wrong_screen" => {
                let frame = run.capture(driver, output.path(), screen);
                run.screen(screen, "drift", Some(step_reason(code)), frame);
                Ok(false)
            }
            Err(failed) => Err(failed),
        }
    };
    if arrived(&mut driver, run, first)? {
        'edges: for id in ids {
            let edge = edges[id];
            let to = edge["to"].as_str().unwrap();
            for act in edge["acts"].as_array().unwrap() {
                if let Some(reason) = guard.stops_before(act) {
                    run.edge(id, "skipped", Some(reason));
                    if reason == "confirm_needed" {
                        run.state["held"] = json!({"edge":id,"act":act["name"],"base":base});
                        run.save();
                    }
                    break 'edges;
                }
                let mut request = json!({"kind":act["kind"],"role":act["role"],"name":act["name"]});
                if let Some(text) = act.get("text") {
                    request["text"] = text.clone();
                }
                if let Some(secret) = act["secret"].as_str() {
                    request["secretEnv"] = format!("GRAPHHELM_SECRET_{secret}").into();
                }
                match driver.call("act", request, &format!("/edges/{id}")) {
                    Ok(_) => {}
                    Err((code, _, _)) if SURVIVABLE.contains(&code) => {
                        let reason = step_reason(code);
                        run.edge(id, "fail", Some(reason));
                        // The page as it was when the act failed, on the screen it was meant to
                        // reach: that is where the owner looks for what went wrong.
                        let frame = run.capture(&mut driver, output.path(), to);
                        run.screen(to, "fail", Some(reason), frame);
                        break 'edges;
                    }
                    Err(failed) => return Err(failed),
                }
            }
            if arrived(&mut driver, run, to)? {
                run.edge(id, "pass", None);
            } else {
                run.edge(id, "drift", Some("expect_missing"));
                break;
            }
        }
    }
    let _ = driver.close();
    Ok(())
}

/// `GET /v1/journey-flows/{id}/screens/{screen}/frame`: one screen's frame from the current
/// preview, with the tag it is served under (the flow digest's first twelve hex digits plus the
/// commit's, so a new run of the same flow and commit keeps the tag only when nothing changed).
pub(crate) fn screen_frame(
    project: &Path,
    id: &str,
    screen: &str,
) -> Option<(Vec<u8>, String, u64, u64)> {
    if !plain_id(id) || !plain_id(screen) {
        return None;
    }
    let (_, digest, commit) = flow_and_key(project, id).ok()?;
    let dir = dir_of(project, id);
    let state = read_state(&dir)?;
    if state["digest"] != digest.as_str() || state["commit"] != commit.as_str() {
        return None;
    }
    let entry = &state["screens"][screen];
    if entry["frame"] != true || entry["file"] != format!("{screen}.png").as_str() {
        return None;
    }
    let file = dir.join("frames").join(format!("{screen}.png"));
    if !safe_node(&file) || std::fs::metadata(&file).ok()?.len() > FRAME_LIMIT {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    let started = state["startedAt"].as_str().unwrap_or_default();
    let digest_hex = digest.trim_start_matches("sha256:");
    let tag = format!(
        "{}-{}",
        &digest_hex[..12.min(digest_hex.len())],
        &sha_of(started)[..8]
    );
    Some((
        bytes,
        tag,
        entry["width"].as_u64().unwrap_or(0),
        entry["height"].as_u64().unwrap_or(0),
    ))
}

fn sha_of(text: &str) -> String {
    use sha2::Digest as _;
    hex::encode(sha2::Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::{
        Guard, JourneyPreviewArgs, body, failed_reason, plain_id, proof_args, read_settled,
        read_state, reap_lost, reap_lost_from, runner_alive, save_state, severity, step_reason,
        unsealed_proof, watch_budget,
    };
    use serde_json::{Value, json};
    use std::path::Path;

    /// #519: a stored preview answers only for the flow and commit it ran on; a changed flow or a
    /// new commit reads as `none` (the Studio then starts a new run), and the runner's pid and the
    /// frame files never reach the caller. Cost: microseconds.
    #[test]
    fn a_stored_preview_answers_only_for_its_own_flow_and_commit() {
        let flow = json!({"status":"draft"});
        let stored = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"ready",
            "pid":1,"result":"pass","screens":{"cart":{"frame":true,"file":"cart.png","result":"pass"}},"edges":{}});
        let same = body(Some(stored.clone()), &flow, "sha256:aa", "c1");
        assert_eq!(same["state"], "ready");
        assert_eq!(same["kind"], "preview");
        assert!(same.get("pid").is_none());
        assert!(same["screens"]["cart"].get("file").is_none());
        assert_eq!(same["screens"]["cart"]["result"], "pass");
        assert_eq!(
            body(Some(stored.clone()), &flow, "sha256:bb", "c1")["state"],
            "none"
        );
        assert_eq!(
            body(Some(stored), &flow, "sha256:aa", "c2")["state"],
            "none"
        );
        assert_eq!(
            body(None, &json!({"status":"approved"}), "sha256:aa", "c1")["kind"],
            "replay"
        );
    }

    /// #560 review: a `running` state past the budget reads as a dead runner even when its pid is
    /// alive (here: this test's own process, as after pid reuse), so the next start runs anew; a
    /// fresh one with a live pid is still running. Cost: one process listing.
    #[test]
    fn a_running_preview_older_than_its_budget_has_no_runner() {
        let pid = std::process::id();
        let fresh =
            json!({"state":"running","pid":pid,"startedAt":chrono::Utc::now().to_rfc3339()});
        assert!(runner_alive(&fresh));
        let old = json!({"state":"running","pid":pid,
            "startedAt":(chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339()});
        assert!(!runner_alive(&old));
        let flow = json!({"status":"draft"});
        let mut stored = old.clone();
        stored["digest"] = "sha256:aa".into();
        stored["commit"] = "c1".into();
        let answer = body(Some(stored), &flow, "sha256:aa", "c1");
        assert_eq!(answer["state"], "failed");
        // #560 review (gh-claude-3's real run): past its budget the run did not finish in time.
        assert_eq!(answer["reason"], "preview.budget_exceeded");
    }

    /// #560 review (gh-claude-3's real run: a runner still alive 15 minutes after a 300 s budget,
    /// blocked in a launcher that never returned): at its deadline the watchdog records
    /// `failed / preview.budget_exceeded` and ends the runner, whatever the main thread is blocked
    /// on; and it never touches a run that already ended or belongs to another runner.
    /// Cost: about half a second, temp files only.
    #[test]
    fn the_budget_watchdog_ends_a_run_that_outlives_its_budget() {
        use std::sync::{Arc, Mutex, mpsc};
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        let me = std::process::id();
        let state = |pid: u32, state: &str| {
            json!({"preview":true,"state":state,"pid":pid,"startedAt":chrono::Utc::now().to_rfc3339(),
                "digest":"sha256:aa","commit":"c1","screens":{},"edges":{}})
        };
        let watch = |expected_end: bool| {
            let (ended, rx) = mpsc::channel();
            watch_budget(
                dir.path().to_path_buf(),
                me,
                Instant::now() + Duration::from_millis(200),
                Arc::new(Mutex::new(None)),
                move || ended.send(()).unwrap(),
            )
            .join()
            .unwrap();
            assert_eq!(rx.try_recv().is_ok(), expected_end);
        };

        save_state(dir.path(), &state(me, "running"));
        watch(true);
        let ended = read_state(dir.path()).unwrap();
        assert_eq!(ended["state"], "failed");
        assert_eq!(ended["reason"], "preview.budget_exceeded");
        assert!(ended["ranAt"].is_string());

        save_state(dir.path(), &state(me, "ready"));
        watch(false);
        assert_eq!(read_state(dir.path()).unwrap()["state"], "ready");

        save_state(dir.path(), &state(me.wrapping_add(1), "running"));
        watch(false);
        assert_eq!(read_state(dir.path()).unwrap()["state"], "running");
    }

    /// #519 (coordinator's decision): an approved flow never plays a destructive act without the
    /// owner's click, on any base, even when the run started the app itself; with the click it
    /// does. A draft keeps #515's rule. Fails if a destructive act on an approved flow is sent
    /// without `--confirm`. Cost: microseconds.
    #[test]
    fn an_approved_flow_holds_before_a_destructive_act_until_the_owner_confirms() {
        let pay = json!({"kind":"click","role":"button","name":"Pay now"});
        let look = json!({"kind":"click","role":"link","name":"Details"});
        assert!(
            super::would_destroy(&pay).is_some(),
            "the fixture must be destructive"
        );
        for launched in [false, true] {
            let held = Guard {
                approved: true,
                launched,
                confirmed: false,
            };
            assert_eq!(held.stops_before(&pay), Some("confirm_needed"));
            assert_eq!(held.stops_before(&look), None);
            let clicked = Guard {
                confirmed: true,
                ..held
            };
            assert_eq!(clicked.stops_before(&pay), None);
        }
        let draft = |launched| Guard {
            approved: false,
            launched,
            confirmed: false,
        };
        assert_eq!(draft(false).stops_before(&pay), Some("guard_refused"));
        assert_eq!(draft(true).stops_before(&pay), None);
    }

    /// #519 slice 3: only an approved flow with all four recording arguments records proof; a
    /// draft never does, and a partial set records nothing rather than half a replay.
    #[test]
    fn only_an_approved_flow_with_an_execution_records_proof() {
        let project = Path::new("p");
        let full = JourneyPreviewArgs {
            id: "checkout".into(),
            project: None,
            force: false,
            read: false,
            run: true,
            confirm: false,
            events: Some("e".into()),
            execution: Some("x".into()),
            keyring: Some("k".into()),
            key_id: Some("id".into()),
        };
        let approved = json!({"status":"approved"});
        let proof = proof_args(&full, &approved, project).expect("approved + execution records");
        assert_eq!(proof.id, "checkout");
        assert_eq!(proof.execution.as_deref(), Some("x"));
        assert!(proof_args(&full, &json!({"status":"draft"}), project).is_none());
        let partial = JourneyPreviewArgs {
            key_id: None,
            ..full
        };
        assert!(proof_args(&partial, &approved, project).is_none());
        // A Runtime with no keyring: an approved flow is refused, a draft plays as a preview.
        assert!(unsealed_proof(&partial, &approved));
        assert!(!unsealed_proof(&partial, &json!({"status":"draft"})));
        let no_execution = JourneyPreviewArgs {
            execution: None,
            ..partial
        };
        assert!(!unsealed_proof(&no_execution, &approved));
    }

    /// #586 (the #585 sweep on main: five flows read `failed / internal` while their stored state
    /// was `ready` seconds later): a reader that saw `running` and then finds the runner gone must
    /// answer what the runner stored on its way out, never a failure. Catches the reader judging
    /// a finished run by the state it read before the run finished. A runner that is really dead
    /// still reads `failed / internal`, and a live one is read once. Cost: four process listings.
    #[test]
    fn a_reader_racing_a_finishing_runner_answers_what_the_runner_stored() {
        let flow = json!({"status":"draft"});
        let now = chrono::Utc::now().to_rfc3339();
        let running = |pid: u32| {
            json!({"preview":true,"digest":"sha256:aa","commit":"c1",
            "state":"running","pid":pid,"startedAt":now,"screens":{},"edges":{}})
        };
        // No process has this pid, here or on Linux: the runner is gone.
        let gone = running(u32::MAX);
        let ready = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"ready",
            "result":"pass","pid":u32::MAX,"startedAt":now,"ranAt":now,"screens":{},"edges":{}});
        let answer = |reads: Vec<Value>| {
            let mut reads = reads.into_iter();
            let mut count = 0;
            let stored = read_settled(|| {
                count += 1;
                reads.next()
            });
            (body(stored, &flow, "sha256:aa", "c1"), count)
        };

        let (finished, _) = answer(vec![gone.clone(), ready]);
        assert_eq!(finished["state"], "ready", "{finished}");
        assert_eq!(finished["result"], "pass", "{finished}");

        let (dead, _) = answer(vec![gone.clone(), gone]);
        assert_eq!(dead["state"], "failed", "{dead}");
        assert_eq!(dead["reason"], "internal", "{dead}");

        let (live, reads) = answer(vec![running(std::process::id())]);
        assert_eq!(live["state"], "running", "{live}");
        assert_eq!(reads, 1, "a live runner's state is read once");
    }

    /// #586: a flow the preview refuses before any browser starts says why, in the refusal's own
    /// code, so the owner reads "a secret is missing" instead of "something went wrong". Catches
    /// a preflight code falling through to `internal` (seen: `studio-connect` without its
    /// `studio_token`). Anything unknown stays `internal`. Cost: microseconds.
    #[test]
    fn a_preflight_refusal_keeps_its_own_reason() {
        for code in [
            "driver.secret_missing",
            "driver.secret_literal",
            "driver.unsupported_act",
            "replay.act_value_missing",
            "replay.entry_missing",
        ] {
            assert_eq!(failed_reason(code), code);
        }
        assert_eq!(failed_reason("anything.else"), "internal");
    }

    /// #593 review (gh-claude-3 killed a runner after its isolated fixture was up: the fixture
    /// stayed alive, and every new preview launches another): a reader that finds the runner gone
    /// runs the launched app's `down` once, then stores `failed / preview.runner_lost` without the
    /// record; a record naming another script, or a directory outside the temp directory, stops
    /// nothing. Cost: a few shell runs (Git Bash on Windows), temp files only.
    #[test]
    fn a_reader_stops_the_app_of_a_runner_that_died() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".graphhelm")).unwrap();
        std::fs::write(
            project.path().join(".graphhelm/journey-fixture.json"),
            r#"{"schema":"graphhelm-journey-fixture/1","script":"fake.sh","isolated":true}"#,
        )
        .unwrap();
        std::fs::write(
            project.path().join("fake.sh"),
            "#!/usr/bin/env bash\nif [ \"$1\" = down ]; then echo down >> down.log; fi\n",
        )
        .unwrap();
        let previews = tempfile::tempdir().unwrap();
        let app = std::env::temp_dir().join(format!(
            "graphhelm-watch-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        ));
        std::fs::create_dir_all(&app).unwrap();
        let state = |launched: Value| {
            json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"running",
                "pid":u32::MAX,"startedAt":chrono::Utc::now().to_rfc3339(),
                "screens":{},"edges":{},"launched":launched})
        };
        let downs = || {
            std::fs::read_to_string(project.path().join("down.log"))
                .map_or(0, |log| log.lines().count())
        };

        save_state(
            previews.path(),
            &state(json!({"script":"fake.sh","dir":app.to_string_lossy()})),
        );
        let reaped = reap_lost(project.path(), previews.path()).unwrap();
        assert_eq!(reaped["state"], "failed", "{reaped}");
        assert_eq!(reaped["reason"], "preview.runner_lost", "{reaped}");
        assert!(reaped.get("launched").is_none(), "{reaped}");
        assert_eq!(downs(), 1, "the launched app's down ran once");
        assert!(!app.exists(), "the fixture directory is removed");
        reap_lost(project.path(), previews.path()).unwrap();
        assert_eq!(downs(), 1, "a reaped run is not stopped again");

        for record in [
            json!({"script":"other.sh","dir":app.to_string_lossy()}),
            json!({"script":"fake.sh","dir":project.path().join("graphhelm-watch-x").to_string_lossy()}),
        ] {
            save_state(previews.path(), &state(record));
            reap_lost(project.path(), previews.path()).unwrap();
        }
        assert_eq!(
            downs(),
            1,
            "a record naming another script or directory stops nothing"
        );
    }

    /// #593 review (gh-claude-3): the reaper settles before it reaps. A reader that first sees
    /// `running` with a dead runner and then, on the settling read, the `ready` the runner wrote
    /// on its way out, answers `ready`: no `down`, no `preview.runner_lost`, and nothing stored
    /// over the finished run. Catches a reaper that reads once (and kills the fixture of a run
    /// that just finished). Cost: one process listing, temp files only.
    #[test]
    fn a_reaper_racing_a_finishing_runner_reaps_nothing() {
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join(".graphhelm")).unwrap();
        std::fs::write(
            project.path().join(".graphhelm/journey-fixture.json"),
            r#"{"schema":"graphhelm-journey-fixture/1","script":"fake.sh","isolated":true}"#,
        )
        .unwrap();
        std::fs::write(
            project.path().join("fake.sh"),
            "#!/usr/bin/env bash\nif [ \"$1\" = down ]; then echo down >> down.log; fi\n",
        )
        .unwrap();
        let previews = tempfile::tempdir().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let app = std::env::temp_dir().join("graphhelm-watch-composition");
        let running = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"running",
            "pid":u32::MAX,"startedAt":now,"screens":{},"edges":{},
            "launched":{"script":"fake.sh","dir":app.to_string_lossy()}});
        let ready = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"ready",
            "result":"pass","pid":u32::MAX,"startedAt":now,"ranAt":now,"screens":{},"edges":{}});
        // The file holds what the runner left; the reads show the race.
        save_state(previews.path(), &ready);
        let mut reads = vec![running, ready.clone()].into_iter();
        let answered = reap_lost_from(project.path(), previews.path(), || reads.next()).unwrap();
        assert_eq!(answered["state"], "ready", "{answered}");
        assert!(answered.get("reason").is_none(), "{answered}");
        assert!(
            !project.path().join("down.log").exists(),
            "a finished run's fixture is not stopped by a reader"
        );
        assert_eq!(
            read_state(previews.path()).unwrap(),
            ready,
            "the finished run is not overwritten"
        );
    }

    /// #519: ids become file names only within the schema's identifier characters, and step
    /// reasons and severities stay within the closed lists agreed with the Studio.
    #[test]
    fn file_names_and_reasons_stay_within_their_closed_sets() {
        assert!(plain_id("studio-see-team"));
        assert!(plain_id("run.details"));
        for bad in ["", "..", "../x", "a/b", "a\\b", ".hidden", "a b"] {
            assert!(!plain_id(bad), "{bad}");
        }
        for code in [
            "driver.timeout",
            "driver.expectation_failed",
            "replay.wrong_screen",
            "driver.action_failed",
            "anything.else",
        ] {
            assert!(
                ["expect_missing", "act_failed", "page_error", "timeout"]
                    .contains(&step_reason(code))
            );
        }
        assert!(severity("fail") > severity("drift") && severity("drift") > severity("pass"));
        assert_eq!(severity("skipped"), 0);
    }
}
