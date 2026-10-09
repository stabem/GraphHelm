//! `journey preview` (#519): opening a journey in the Studio runs it, so the owner sees each screen
//! as it really renders and whether each step passes, before deciding anything.
//!
//! One headless run plays every path of the flow from its entry, and at each screen records the
//! result (`pass`, `fail`, `drift`), the reason, and a frame (a masked screenshot, the same capture
//! replay takes). The result is kept per flow, keyed by the flow's digest and the project's commit,
//! so opening the journey again shows it without a new run; a changed flow or a new commit makes it
//! stale, and `--force` (the Studio's Run again) runs it anew.
//!
//! A preview is never proof: it records no capture signal, walks no arrow and writes no cache.
//! Drafts are played too; an act that would destroy something on a draft is not sent unless the
//! preview started the app under test itself (#515's rule, `would_destroy`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::journey_live::{Launched, base_reachable, launch, would_destroy};
use super::journey_replay::{
    Driver, Failure, Result, SURVIVABLE, TemporaryOutput, failure, observe, observer_ready,
    preflight, safe_directory, safe_node,
};
use crate::args::JourneyPreviewArgs;
use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "journey.preview";
const PREVIEWS: &str = ".graphhelm/journey-previews";
const STATE: &str = "state.json";
/// The page size a preview plays at; the frames the Studio shows are this size.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
/// A whole preview, every path, ends within this.
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

fn save_state(dir: &Path, state: &Value) {
    let mut bytes = serde_json::to_vec_pretty(state).unwrap();
    bytes.push(b'\n');
    let _ = super::journey_flow::atomic_write(&dir.join(STATE), &bytes);
}

/// Whether a `running` state still has a runner. The runner writes its own pid when it starts;
/// before that, a just-started run counts as alive for `RUNNER_GRACE`.
fn runner_alive(state: &Value) -> bool {
    match state["pid"].as_u64() {
        Some(pid) => alive(pid),
        None => state["startedAt"]
            .as_str()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .is_some_and(|at| {
                chrono::Utc::now().signed_duration_since(at)
                    < chrono::Duration::from_std(RUNNER_GRACE).unwrap()
            }),
    }
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
        state["state"] = "failed".into();
        state["reason"] = "internal".into();
    }
    if let Some(object) = state.as_object_mut() {
        object.remove("pid");
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
                read_state(&dir_of(&project, &args.id)),
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
    let stored = read_state(&dir);
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
    if !args.force && matches!(current["state"].as_str(), Some("ready" | "failed")) {
        return answer(current, None);
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
        .arg(&project)
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

/// The top-level `reason` a failed preview carries (the closed list agreed on #519).
fn failed_reason(code: &str) -> &'static str {
    match code {
        "watch.app_down" => "watch.app_down",
        "watch.launcher_invalid" => "watch.launcher_invalid",
        "driver.observer_missing" | "replay.observer_missing" => "driver.observer_missing",
        "preview.budget_exceeded" => "preview.budget_exceeded",
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
}

impl Run {
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
    let mut run = Run {
        dir,
        state,
        frames: 0,
    };
    run.save();
    let deadline = Instant::now() + PREVIEW_BUDGET;
    let outcome = play(&project, &flow, &mut run, deadline);
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

fn play(project: &Path, flow: &Value, run: &mut Run, deadline: Instant) -> Result<()> {
    let secrets = preflight(flow)?;
    observer_ready(project)?;
    let base = flow["base"].as_str().unwrap().to_owned();
    // The app under test is started when it is down, and stopped when the preview ends.
    let launched: Option<Launched> = if base_reachable(&base) {
        None
    } else {
        Some(launch(project, &base)?)
    };
    let guarded = flow["status"] != "approved" && launched.is_none();
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
    for (_, path_edges) in paths {
        if Instant::now() >= deadline {
            return Err(failure("preview.budget_exceeded", "/paths", 1));
        }
        let ids: Vec<&str> = path_edges
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap())
            .collect();
        play_path(
            project, &base, &secrets, &screens, &edges, &ids, guarded, run,
        )?;
    }
    drop(launched);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn play_path(
    project: &Path,
    base: &str,
    secrets: &std::collections::BTreeMap<String, String>,
    screens: &std::collections::BTreeMap<&str, &Value>,
    edges: &std::collections::BTreeMap<&str, &Value>,
    ids: &[&str],
    guarded: bool,
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
        json!({"base":entry,"viewport":{"width":WIDTH,"height":HEIGHT},"allowOrigins":[],"survive":true}),
        "/entry",
    )?;
    let arrived = |driver: &mut Driver, run: &mut Run, screen: &str| -> Result<bool> {
        match observe(driver, screens[screen], base, &format!("/screens/{screen}")) {
            Ok(_) => {
                let frame = run.capture(driver, output.path(), screen);
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
                if guarded && would_destroy(act).is_some() {
                    run.edge(id, "skipped", Some("guard_refused"));
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
    use super::{body, plain_id, severity, step_reason};
    use serde_json::json;

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
