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
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::journey_live::{
    Launched, LaunchedStop, base_reachable, launch, launcher_isolated, would_destroy,
};
use super::journey_replay::{
    Driver, Failure, Result, SURVIVABLE, TemporaryOutput, declared_browser, failure, observe,
    preflight, record, safe_directory, safe_node, walked, with_declared,
};
use crate::args::{JourneyPreviewArgs, JourneyReplayArgs};
use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "journey.preview";
const PREVIEWS: &str = ".graphhelm/journey-previews";
const STATE: &str = "state.json";
/// The runner's own stderr (#1298): why a runner that died before finishing stopped.
const RUNNER_LOG: &str = "runner.log";
/// The last bytes of `RUNNER_LOG` a lost run carries as its `launchLog.tail`.
const RUNNER_LOG_TAIL: usize = 4096;
/// The page size a preview plays at unless the flow declares one (#585); the frames the Studio
/// shows are this size.
const WIDTH: u32 = 1280;
const HEIGHT: u32 = 800;
/// #1777: the mobile viewport a Run proves; the driver pins the matching device profile.
const MOBILE_WIDTH: u32 = 390;
const MOBILE_HEIGHT: u32 = 844;

/// #1777: one viewport a Run plays: its name, page size and the driver device it asks for.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct View {
    pub(crate) name: &'static str,
    pub(crate) viewport: Value,
    pub(crate) device: Option<&'static str>,
}

/// #1777: the viewports a flow's Run proves, in play order. A fixed `viewport` (#585) is one size,
/// played alone and named by its width; `viewports` names the ones kept; neither is both.
pub(crate) fn views_of(flow: &Value) -> Vec<View> {
    let web = View {
        name: "web",
        viewport: json!({"width":WIDTH,"height":HEIGHT}),
        device: None,
    };
    let mobile = View {
        name: "mobile",
        viewport: json!({"width":MOBILE_WIDTH,"height":MOBILE_HEIGHT}),
        device: Some("mobile"),
    };
    if let Some(fixed) = flow.get("viewport").filter(|v| v.is_object()) {
        let narrow = fixed["width"].as_u64().is_some_and(|width| width < 768);
        return vec![View {
            name: if narrow { "mobile" } else { "web" },
            viewport: fixed.clone(),
            device: None,
        }];
    }
    match flow.get("viewports").and_then(Value::as_array) {
        Some(names) => [web, mobile]
            .into_iter()
            .filter(|view| names.iter().any(|name| name == view.name))
            .collect(),
        None => vec![web, mobile],
    }
}

/// #1777: a run's budget is the preview budget for each viewport it plays.
fn budget_of(state: &Value) -> Duration {
    let views = state["viewportsPlanned"]
        .as_array()
        .map_or(1, Vec::len)
        .clamp(1, 2);
    PREVIEW_BUDGET * u32::try_from(views).unwrap_or(1)
}
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

fn dir_for(project: &Path, id: &str, validation_dir: Option<&Path>) -> PathBuf {
    validation_dir
        .map(|dir| dir.join(id))
        .unwrap_or_else(|| dir_of(project, id))
}

/// The flow as the preview plays it, and the key its result is kept under.
fn flow_and_key(project: &Path, id: &str) -> Result<(Value, String, String)> {
    flow_key_and_source(project, id).map(|(flow, digest, commit, _)| (flow, digest, commit))
}

/// `flow_and_key` plus the exact text the flow was parsed from (#1911: the reviewed check compares
/// these bytes, never a second read of the file).
fn flow_key_and_source(project: &Path, id: &str) -> Result<(Value, String, String, String)> {
    if !plain_id(id) {
        return Err(failure("preview.flow_unknown", "/id", 2));
    }
    let file = project
        .join(".graphhelm/journeys")
        .join(format!("{id}.journey.yaml"));
    if !safe_node(&file) {
        return Err(failure("preview.flow_unknown", "/id", 2));
    }
    let (flow, source) =
        super::journey_flow::read_for_preview_source(&file, project).map_err(|findings| {
            findings.into_iter().find(|f| !f.is_warning()).map_or_else(
                || failure("preview.flow_unknown", "/id", 2),
                |finding| failure(finding.code, finding.pointer, 2),
            )
        })?;
    let digest = super::journey_flow::approval_digest(&flow);
    let commit = scope_commit_of(project, id, &flow);
    Ok((flow, digest, commit, source))
}

/// The flow's scope commit, part of the key (#1309 card 1c): the latest commit that touched the
/// flow file or any path the flow's screens scope. The same flow on unrelated new code keeps its
/// preview; a change to what the flow covers is a new preview. The paths are the draft's own, so a
/// scope path since deleted or renamed still names the commit that last touched it.
pub(crate) fn scope_commit_of(project: &Path, id: &str, flow: &Value) -> String {
    let mut paths = vec![format!(".graphhelm/journeys/{id}.journey.yaml")];
    for screen in flow["screens"].as_array().into_iter().flatten() {
        for path in screen["scope"].as_array().into_iter().flatten() {
            if let Some(path) = path.as_str() {
                paths.push(path.to_owned());
            }
        }
    }
    graphhelm_process::command("git")
        .arg("--literal-pathspecs")
        .arg("-C")
        .arg(project)
        .args(["log", "-1", "--format=%H", "--"])
        .args(&paths)
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

/// #1911: the URL of the repository whose `main` is the reviewed source, set once by the Runtime
/// from its own startup argument (`serve --reviewed-remote`), never read from the project's
/// `.git/config`, which a lane can edit.
static REVIEWED_REMOTE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// The variable the Runtime hands the reviewed remote to its own preview runner through.
const REVIEWED_REMOTE_ENV: &str = "GRAPHHELM_REVIEWED_REMOTE";

/// #1911: called once by `serve` with its `--reviewed-remote`.
pub(crate) fn set_reviewed_remote(url: Option<String>) {
    if let Some(url) = url.filter(|url| !url.trim().is_empty()) {
        drop(REVIEWED_REMOTE.set(url));
    }
}

/// #1911: whether `parsed`, the exact bytes the run parsed its flow from, is the reviewed flow:
/// equal to the blob at `.graphhelm/journeys/<id>.journey.yaml` on the `main` the remote serves
/// NOW. The anchor is the remote, not any local ref: the Runtime's remote URL is fetched
/// (`refs/heads/main`, depth 1) into a fresh repository of its own under the temp directory, with
/// system and global git config ignored, and the blob is read there. A lane can move
/// `refs/remotes/origin/main` or edit `.git/config`; neither is consulted. No URL, a network
/// failure, a timeout, a missing blob or different bytes all answer `false`: the commands wait.
pub(crate) fn reviewed_on_main(remote: Option<&str>, id: &str, parsed: &[u8]) -> bool {
    let Some(remote) = remote.filter(|url| !url.is_empty() && !url.starts_with('-')) else {
        return false;
    };
    if !plain_id(id) || parsed.is_empty() {
        return false;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let scratch =
        std::env::temp_dir().join(format!("graphhelm-reviewed-{}-{nonce}", std::process::id()));
    if std::fs::create_dir(&scratch).is_err() {
        return false;
    }
    let git = |args: &[&str]| {
        let mut command = graphhelm_process::command("git");
        command
            .arg("--git-dir")
            .arg(&scratch)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_OBJECT_DIRECTORY")
            .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
            .env_remove("GIT_CONFIG_PARAMETERS");
        bounded_stdout(command, REVIEWED_FETCH_LIMIT)
    };
    let answer = git(&["init", "-q", "--bare"]).is_some()
        && git(&[
            "fetch",
            "-q",
            "--no-tags",
            "--depth=1",
            "--",
            remote,
            "+refs/heads/main:refs/reviewed/main",
        ])
        .is_some()
        && git(&[
            "cat-file",
            "blob",
            &format!("refs/reviewed/main:.graphhelm/journeys/{id}.journey.yaml"),
        ])
        .is_some_and(|blob| blob == parsed);
    drop(std::fs::remove_dir_all(&scratch));
    answer
}

/// How long each git step of the reviewed check may take (#1911).
const REVIEWED_FETCH_LIMIT: Duration = Duration::from_secs(30);

/// Runs `command` to its end within `limit` and returns its stdout when it succeeded. A spawn
/// failure, a non-zero exit, or a timeout (the child is killed) answers `None`.
fn bounded_stdout(mut command: std::process::Command, limit: Duration) -> Option<Vec<u8>> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // Stdout is read on its own thread so a full pipe never stalls the bounded wait below.
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut out, &mut bytes).map(|_| bytes)
        })
    });
    let deadline = Instant::now() + limit;
    let succeeded = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            // A hung or unreadable child: kill it. A failed kill changes nothing, the answer is None.
            _ => {
                drop(child.kill());
                break false;
            }
        }
    };
    let out = reader
        .and_then(|thread| thread.join().ok())
        .and_then(|read| read.ok());
    out.filter(|_| succeeded)
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
    let _ = super::atomic::atomic_write(&dir.join(STATE), &bytes);
}

/// Whether a `running` state still has a runner. The runner writes its own pid when it starts;
/// before that, a just-started run counts as alive for `RUNNER_GRACE`. Past the budget plus that
/// grace no runner is alive, even when the pid now names another process (#560 review: a hard
/// kill, then pid reuse, would otherwise read `running` for ever).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Liveness {
    Alive,
    Dead,
    Unknown,
}

fn runner_liveness(state: &Value) -> Liveness {
    runner_liveness_with(state, alive)
}

fn runner_liveness_with(state: &Value, probe: impl FnOnce(u64) -> Liveness) -> Liveness {
    if !within_budget(state) {
        return Liveness::Dead;
    }
    match state["pid"].as_u64() {
        Some(pid) => probe(pid),
        None if younger_than(state, RUNNER_GRACE) => Liveness::Alive,
        None => Liveness::Unknown,
    }
}

/// Whether a run started less than its budget plus the grace ago.
fn within_budget(state: &Value) -> bool {
    younger_than(state, budget_of(state) + RUNNER_GRACE)
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

fn alive(pid: u64) -> Liveness {
    #[cfg(windows)]
    {
        // 0 and the all-ones sentinel cannot name a Windows process. `tasklist` rejects
        // the latter as an invalid filter rather than reporting a missing process.
        if pid == 0 || pid >= u32::MAX as u64 {
            return Liveness::Dead;
        }
        let Ok(output) = graphhelm_process::command("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
        else {
            return Liveness::Unknown;
        };
        if !output.status.success() {
            return Liveness::Unknown;
        }
        let Ok(text) = String::from_utf8(output.stdout) else {
            return Liveness::Unknown;
        };
        tasklist_liveness(&text, pid)
    }
    #[cfg(not(windows))]
    {
        let path = format!("/proc/{pid}");
        match std::fs::metadata(&path) {
            Ok(_) => Liveness::Alive,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Liveness::Dead,
            Err(_) => Liveness::Unknown,
        }
    }
}

#[cfg(windows)]
fn tasklist_liveness(text: &str, pid: u64) -> Liveness {
    if text
        .lines()
        .any(|line| line.starts_with('"') && line.contains(&format!(",\"{pid}\"")))
    {
        Liveness::Alive
    } else if text.contains("INFO: No tasks are running which match the specified criteria.") {
        Liveness::Dead
    } else {
        Liveness::Unknown
    }
}

/// The body every preview answer carries: the stored run when it is for this flow and commit,
/// else `state: none`. Private fields (the runner's pid, frame files) never leave this module.
/// Every caller hands it a state `reap_lost` already settled, or the `running` state `start` just
/// wrote; the reaper probed the runner once and stored a dead one as failed. So a `running` here is
/// answered as running (#1610): a second probe could see a runner that finished in between as dead
/// and answer the stale copy as `failed / internal` while the stored run is `ready`. The budget
/// still ends a run that never finishes.
fn body(state: Option<Value>, flow: &Value, digest: &str, commit: &str) -> Value {
    body_with(state, flow, digest, commit, &|_| Liveness::Alive)
}

fn body_with(
    state: Option<Value>,
    flow: &Value,
    digest: &str,
    commit: &str,
    liveness: &impl Fn(&Value) -> Liveness,
) -> Value {
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
    if state["state"] == "running" && (!within_budget(&state) || liveness(&state) == Liveness::Dead)
    {
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

/// The flow and the kept preview body for this flow and commit (`state: none` when there is
/// none): what `journey report` renders, read in this process.
pub(crate) fn kept(project: &Path, id: &str) -> Result<(Value, Value)> {
    let (flow, digest, commit) = flow_and_key(project, id)?;
    let stored = reap_lost(project, &dir_of(project, id));
    Ok((flow.clone(), body(stored, &flow, &digest, &commit)))
}

/// `journey preview <id>` / `POST /v1/journey-flows/{id}/preview`: answer the stored preview when
/// it is current, else start one in the background and answer `running`. `--force` runs anew.
pub(crate) fn start(args: &JourneyPreviewArgs) -> Outcome {
    start_with(args, runner_liveness)
}

/// #1662: the owner's confirm that a draft's commands may run. A confirm of a draft that has
/// `cli` acts comes only through the owner's door (the Runtime's owner-only route, or the CLI
/// with the owner's token), names the flow digest the owner saw beside the argv, and is recorded
/// as its own owner event before the run starts. A lane's preview cannot confirm, so it stays at
/// `confirm_needed`. Anything else passes through unchanged.
fn confirm_commands(args: &JourneyPreviewArgs, project: &Path, flow: &Value) -> Result<()> {
    let argv = super::journey_backend::cli_argv(flow);
    if !args.confirm
        || flow["status"] == "approved"
        || argv.is_empty()
        || args.validation_dir.is_some()
    {
        return Ok(());
    }
    if !args.owner && !super::journey_flow::owner_token_matches(project, args.token_file.as_deref())
    {
        return Err(failure("preview.confirm_owner_only", "/confirm", 2));
    }
    let digest = super::journey_flow::approval_digest(flow);
    if args.confirm_digest.as_deref() != Some(digest.as_str()) {
        return Err(failure("preview.confirm_stale", "/confirmDigest", 2));
    }
    super::journey_owner::record_cli_confirmed(project, &args.id, &digest, &argv)
        .map_err(|_| failure("preview.confirm_unrecorded", "/confirm", 1))
}

/// [`start`] with the runner probe the reaper uses (a test supplies it).
fn start_with(args: &JourneyPreviewArgs, liveness: impl Fn(&Value) -> Liveness) -> Outcome {
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
    if let Err(failed) = confirm_commands(args, &project, &flow) {
        return answer(json!({}), Some(failed));
    }
    let dir = dir_for(&project, &args.id, args.validation_dir.as_deref());
    let stored = reap_lost_from_with(&project, &dir, || read_state(&dir), &liveness);
    // The reaper probed the runner once and stored a dead one as failed, so a `running` it hands
    // back is going. A second probe could see a runner that finished in between as dead, and a
    // forced start would then overwrite its result and start another run (#1610).
    let running_now = stored
        .as_ref()
        .is_some_and(|state| state["state"] == "running");
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
    if let Some(previous) = stored
        .as_ref()
        .filter(|state| matches!(state["state"].as_str(), Some("ready" | "failed")))
    {
        archive(&dir, previous);
    }
    let _ = std::fs::remove_dir_all(dir.join("frames"));
    let Ok(executable) = std::env::current_exe() else {
        return answer(current, Some(failure("preview.unwritable", "/runner", 1)));
    };
    let mut command = graphhelm_process::command(executable);
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
    // #1911: only the Runtime's own owner route hands its reviewed remote to the runner; any other
    // start (a lane's CLI, validation) carries none, so its drafts' commands wait for review.
    command.env_remove(REVIEWED_REMOTE_ENV);
    if let Some(url) = REVIEWED_REMOTE
        .get()
        .filter(|_| args.owner && args.validation_dir.is_none())
    {
        command.env(REVIEWED_REMOTE_ENV, url);
    }
    if let Some(validation_dir) = args.validation_dir.as_deref() {
        command.arg("--validation-dir").arg(validation_dir);
    }
    if args.confirm {
        command.arg("--confirm");
    }
    if let Some(proof) = proof_args(args, &flow, &project) {
        command
            .arg("--events")
            .arg(proof.record.events.as_ref().unwrap())
            .args(["--execution", proof.record.execution.as_deref().unwrap()])
            .arg("--keyring")
            .arg(proof.record.keyring.as_ref().unwrap())
            .args(["--key-id", proof.record.key_id.as_deref().unwrap()]);
    }
    // #1298: the runner's stderr is kept, truncated per run, so a runner that dies says why.
    // A link planted at that name is never followed.
    let log = dir.join(RUNNER_LOG);
    let stderr = (std::fs::symlink_metadata(&log).is_err() || safe_node(&log))
        .then(|| std::fs::File::create(&log).ok())
        .flatten()
        .map_or_else(Stdio::null, Stdio::from);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
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
    reap_lost_from_with(project, dir, read, runner_liveness)
}

fn reap_lost_from_with(
    project: &Path,
    dir: &Path,
    read: impl FnMut() -> Option<Value>,
    liveness: impl Fn(&Value) -> Liveness,
) -> Option<Value> {
    // Settled first (#586): a run that finished between the read and the liveness check is read
    // as it finished, and is never reaped. One probe only (#1448): a second probe could see a
    // runner that wrote its result after the read as dead, and store the stale `running` copy
    // over that result. A dead runner writes nothing more, so the read after its probe is final;
    // it is reaped only while it is still that runner's `running` run.
    let dead = std::cell::RefCell::new(None);
    let mut state = read_settled_with(read, &|state: &Value| {
        let verdict = liveness(state);
        if verdict == Liveness::Dead {
            *dead.borrow_mut() = Some(state.clone());
        }
        verdict
    })?;
    let Some(probed) = dead.into_inner() else {
        return Some(state);
    };
    if state["state"] != "running"
        || state["pid"] != probed["pid"]
        || state["startedAt"] != probed["startedAt"]
    {
        return Some(state);
    }
    // #1298: a runner that died before it launched any app is lost all the same; storing it
    // `failed` lets the next run archive it instead of erasing it.
    if !state["launched"].is_null()
        && let Some(stop) = LaunchedStop::from_record(project, &state["launched"])
    {
        stop.stop();
    }
    if let Some(log) = runner_log(dir) {
        state["launchLog"] = log;
    }
    state["ranAt"] = chrono::Utc::now().to_rfc3339().into();
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

/// The runner's own stderr as a lost run's `launchLog` (#1298): its path and its last
/// `RUNNER_LOG_TAIL` bytes, or nothing when the log is missing, unsafe or empty.
fn runner_log(dir: &Path) -> Option<Value> {
    let path = dir.join(RUNNER_LOG);
    if !safe_node(&path) {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let start = bytes.len().saturating_sub(RUNNER_LOG_TAIL);
    let tail = String::from_utf8_lossy(&bytes[start..]).trim().to_owned();
    (!tail.is_empty()).then(|| json!({"path": path.to_string_lossy(), "tail": tail}))
}

fn read_settled_with(
    mut read: impl FnMut() -> Option<Value>,
    liveness: &impl Fn(&Value) -> Liveness,
) -> Option<Value> {
    let state = read()?;
    if state["state"] == "running" && liveness(&state) == Liveness::Dead {
        return read();
    }
    Some(state)
}

/// The top-level `reason` a failed preview carries (the closed list agreed on #519).
fn failed_reason(code: &str) -> &'static str {
    match code {
        "watch.app_down" => "watch.app_down",
        "watch.launch_failed" => "watch.launch_failed",
        "watch.launcher_invalid" => "watch.launcher_invalid",
        "observer.setup_failed" => "observer.setup_failed",
        "observer.setup_timeout" => "observer.setup_timeout",
        // #1722: a stale copy keeps the closed reason list (#519); `detail.code` says which.
        "driver.observer_missing" | "replay.observer_missing" | "replay.observer_stale" => {
            "driver.observer_missing"
        }
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

/// A screen entry carrying the driver's per-assertion judgement (#1286). The entry is the
/// captured frame when there is one, else the same `{"frame":false}` a bare screen records.
fn with_checks(frame: Option<Value>, checks: Option<Value>) -> Option<Value> {
    let Some(checks) = checks else { return frame };
    let mut entry = frame.unwrap_or_else(|| json!({"frame":false}));
    entry["checks"] = checks;
    Some(entry)
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

/// A run's result (#1633): its worst step result, and `pass` only when every screen and edge
/// passed. A step not reached or skipped proves nothing, so a run with one and no failure is
/// `fail` with the reason `preview.incomplete`, and records no proof. A path's entry screen is
/// reached when an edge leaving it passed: a backend flow observes no screen before its first act,
/// so its start screen carries no result of its own. No other screen is excused.
/// Every screen and edge no path reached says so, in the top-level view and in each viewport's.
fn fill_not_reached(flow: &Value, state: &mut Value) {
    let fill = |group: &mut Value, steps: &str, empty: Value| {
        if !group.is_object() {
            *group = json!({});
        }
        for step in flow[steps].as_array().unwrap() {
            let id = step["id"].as_str().unwrap();
            if group[id].is_null() {
                group[id] = empty.clone();
            }
        }
    };
    let screen = json!({"frame":false,"reason":"not_reached"});
    let edge = json!({"result":null,"reason":"not_reached"});
    fill(&mut state["screens"], "screens", screen.clone());
    fill(&mut state["edges"], "edges", edge.clone());
    if let Some(views) = state["viewports"].as_object_mut() {
        for view in views.values_mut() {
            fill(&mut view["screens"], "screens", screen.clone());
            fill(&mut view["edges"], "edges", edge.clone());
        }
    }
}

/// #1777: keeps what one viewport recorded under `viewports.<name>` before the next one plays.
fn stash_view(flow: &Value, state: &mut Value, name: &str) {
    if !state["viewports"].is_object() {
        state["viewports"] = json!({});
    }
    let mut view = json!({"screens": state["screens"].clone(), "edges": state["edges"].clone()});
    fill_not_reached(flow, &mut view);
    let (result, _) = run_verdict(flow, &view["screens"], &view["edges"]);
    view["result"] = result.into();
    state["viewports"][name] = view;
}

/// #1777: the top-level `screens` and `edges` are the first viewport's, so every older reader
/// sees today's shape and frames; each step's `result` is its worst at any viewport, and a step
/// that failed at some viewports but not all names them in `failedAt`.
fn combine_views(state: &mut Value) {
    let Some(views) = state["viewports"].as_object().cloned() else {
        return;
    };
    let order: Vec<String> = state["viewportsPlanned"]
        .as_array()
        .map(|names| {
            names
                .iter()
                .filter_map(|name| name.as_str().map(str::to_owned))
                .filter(|name| views.contains_key(name))
                .collect()
        })
        .unwrap_or_default();
    let Some(first) = order.first().and_then(|name| views.get(name)) else {
        return;
    };
    let mut combined =
        json!({"screens": first["screens"].clone(), "edges": first["edges"].clone()});
    for group in ["screens", "edges"] {
        let Some(entries) = combined[group].as_object_mut() else {
            continue;
        };
        for (id, entry) in entries.iter_mut() {
            let mut failed_at = Vec::new();
            for name in &order {
                let other = &views[name][group][id];
                let result = other["result"].as_str();
                if result.is_some_and(|result| result != "pass") {
                    failed_at.push(name.clone());
                }
                if result.map_or(0, severity) > entry["result"].as_str().map_or(0, severity) {
                    entry["result"] = other["result"].clone();
                    if !other["reason"].is_null() {
                        entry["reason"] = other["reason"].clone();
                    }
                }
            }
            if !failed_at.is_empty() && failed_at.len() < order.len() {
                entry["failedAt"] = failed_at.into();
            }
        }
    }
    state["screens"] = combined["screens"].take();
    state["edges"] = combined["edges"].take();
}

fn run_verdict(flow: &Value, screens: &Value, edges: &Value) -> (String, Option<&'static str>) {
    let edge_of = |id: &Value| {
        flow["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|edge| edge["id"] == *id)
    };
    // A path's entry screen: where its first edge starts.
    let entry = |screen: &str| {
        flow["paths"]
            .as_object()
            .into_iter()
            .flatten()
            .any(|(_, path)| {
                path.get(0)
                    .and_then(edge_of)
                    .is_some_and(|edge| edge["from"] == screen)
            })
    };
    let left_by_a_pass = |screen: &str| {
        entry(screen)
            && flow["edges"].as_array().into_iter().flatten().any(|edge| {
                edge["from"] == screen
                    && edge["id"]
                        .as_str()
                        .is_some_and(|id| edges[id]["result"] == "pass")
            })
    };
    let screen_steps = screens.as_object().into_iter().flatten();
    let edge_steps = edges.as_object().into_iter().flatten();
    let worst = screen_steps
        .clone()
        .chain(edge_steps.clone())
        .filter_map(|(_, entry)| entry["result"].as_str())
        .filter(|result| severity(result) > 1)
        .max_by_key(|result| severity(result));
    if let Some(worst) = worst {
        return (worst.to_owned(), None);
    }
    let screens_pass = screen_steps
        .clone()
        .all(|(id, entry)| entry["result"] == "pass" || left_by_a_pass(id));
    let edges_pass = edge_steps
        .clone()
        .all(|(_, entry)| entry["result"] == "pass");
    if screens_pass && edges_pass {
        ("pass".to_owned(), None)
    } else {
        ("fail".to_owned(), Some("preview.incomplete"))
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
    /// #585: the page size the flow declares (else the preview's size), and the fields `open` adds
    /// for it: seeded localStorage and, #1782, the fixed clock.
    viewport: Value,
    declared: Option<Value>,
    /// #1777: the viewport being played: the driver device it asks for, where its frames go
    /// (`frames` for the first, `frames/<name>` after), and whether it records the proof.
    device: Option<&'static str>,
    frames_sub: PathBuf,
    first_view: bool,
    /// Validation owns a separate result tree and asks the driver for JS coverage per path.
    validation: bool,
    flow_id: String,
    /// #1911: the parsed flow bytes equal the blob on the reviewed remote's `main`.
    reviewed: bool,
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
        record: crate::args::OptionalRecordArgs {
            events: Some(args.events.clone()?),
            execution: Some(args.execution.clone()?),
            keyring: Some(args.keyring.clone()?),
            key_id: Some(args.key_id.clone()?),
        },
        allow_origin: Vec::new(),
        replay_worker: false,
        viewport: None,
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
        let Some(args) = self.proof.as_ref().filter(|_| self.first_view) else {
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
        let frames = self.dir.join(&self.frames_sub);
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
    let (flow, digest, commit, source) = match flow_key_and_source(&project, &args.id) {
        Ok(found) => found,
        Err(failed) => return answer(json!({}), Some(failed)),
    };
    // #1911: the reviewed check runs on the bytes just parsed, before anything plays. Only a
    // draft with commands needs it, so other runs never touch the network.
    let reviewed = flow["status"] != "approved"
        && !super::journey_backend::cli_argv(&flow).is_empty()
        && reviewed_on_main(
            std::env::var(REVIEWED_REMOTE_ENV).ok().as_deref(),
            &args.id,
            source.as_bytes(),
        );
    // #1662 review: the runner does not re-confirm. The door recorded the owner's confirm before
    // spawning it, and `play` lets a draft's command run only when the owner store holds that
    // exact digest and argv, so a forged `--run --confirm` still holds.
    let dir = dir_for(&project, &args.id, args.validation_dir.as_deref());
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
        proof: if args.validation_dir.is_some() {
            None
        } else {
            proof_args(args, &flow, &project)
        },
        contract: String::new(),
        last: None,
        confirmed: args.confirm && args.validation_dir.is_none(),
        viewport: Value::Null,
        declared: None,
        device: None,
        frames_sub: PathBuf::from("frames"),
        first_view: true,
        validation: args.validation_dir.is_some(),
        flow_id: args.id.clone(),
        reviewed,
    };
    (run.viewport, run.declared) = declared_browser(&flow, json!({"width":WIDTH,"height":HEIGHT}));
    let views = if super::journey_backend::is_backend(&flow) {
        vec![View {
            name: "web",
            viewport: run.viewport.clone(),
            device: None,
        }]
    } else {
        views_of(&flow)
    };
    run.state["viewportsPlanned"] = views
        .iter()
        .map(|view| view.name)
        .collect::<Vec<_>>()
        .into();
    if run.validation {
        run.state["validationPaths"] = json!({});
        for name in flow["paths"].as_object().unwrap().keys() {
            run.state["validationPaths"][name] =
                json!({"execution":"not_run","collection":"unavailable"});
        }
    }
    run.save();
    let deadline = Instant::now() + budget_of(&run.state);
    let launched = std::sync::Arc::new(std::sync::Mutex::new(None));
    watch_budget(
        run.dir.clone(),
        std::process::id(),
        deadline,
        launched.clone(),
        || std::process::exit(3),
    );
    let outcome = play(&project, &flow, &mut run, deadline, &launched, &views);
    // Every screen and edge no path reached says so; the run's result is its worst step at any
    // viewport (#1777).
    fill_not_reached(&flow, &mut run.state);
    combine_views(&mut run.state);
    let (worst, incomplete) = run_verdict(&flow, &run.state["screens"], &run.state["edges"]);
    run.state["current"] = Value::Null;
    run.state["ranAt"] = chrono::Utc::now().to_rfc3339().into();
    if let Some(object) = run.state.as_object_mut() {
        object.remove("launched");
    }
    let app = match outcome {
        Ok(app) => {
            run.state["state"] = "ready".into();
            run.state["result"] = worst.into();
            if let Some(reason) = incomplete {
                run.state["reason"] = reason.into();
            }
            app
        }
        Err((code, pointer, _)) => {
            run.state["state"] = "failed".into();
            run.state["reason"] = failed_reason(code).into();
            if let Some(log) = super::journey_live::take_launch_log() {
                run.state["launchLog"] = log;
            }
            // The driver's own masked stderr tail when it died before answering (#1448).
            if let Some(log) = super::journey_replay::take_driver_log() {
                run.state["driverLog"] = log;
            }
            let detail = json!({"code":code,"pointer":pointer});
            run.state["detail"] = detail.clone();
            // The pointer may name a flow array index (preflight) or a stable step id (play).
            // Keep only the refusal's code and pointer, never driver output or page text.
            let mut parts = pointer.split('/').skip(1);
            if let (Some(kind @ ("screens" | "edges")), Some(step)) = (parts.next(), parts.next()) {
                let steps = flow[kind].as_array().unwrap();
                let stopped = if kind == "edges" && parts.next() == Some("acts") {
                    step.parse::<usize>()
                        .ok()
                        .and_then(|index| steps.get(index))
                } else {
                    steps.iter().find(|entry| entry["id"] == step)
                };
                if let Some(id) = stopped.and_then(|entry| entry["id"].as_str()) {
                    let mut entry =
                        json!({"result":"fail","reason":step_reason(code),"detail":detail});
                    if kind == "screens" {
                        entry["frame"] = false.into();
                    }
                    run.state[kind][id] = entry;
                }
            }
            None
        }
    };
    // #1309 card 1c: a passing preview is the owner's proof for approve; the runner that played
    // it records it, over the evidence it read, before it publishes completion.
    if run.state["state"] == "ready" && run.state["result"] == "pass" && !run.validation {
        let recorded = record_pass(&run, &project, &digest, &commit);
        run.state["recorded"] = recorded.is_ok().into();
        // #1672: who observed this run, as the record names it: the runner actor, the sha256 of
        // its binary and its version. Absent when nothing was recorded.
        if let Ok(observer) = recorded {
            run.state["observer"] = observer;
        }
    }
    // Publish completion before down: teardown can block or end the runner (#676).
    run.save();
    if let Ok(mut slot) = launched.lock() {
        slot.take();
    }
    drop(app);
    answer(
        json!({"state":run.state["state"],"result":run.state["result"],"paths":run.state["validationPaths"],"reason":run.state["reason"],"launchLog":run.state["launchLog"]}),
        None,
    )
}

/// Appends the passing preview record: the flow, its digest and scope commit, a sha256 over the
/// step results and frame bytes this runner captured, and this runner's own binary and version.
fn record_pass(
    run: &Run,
    project: &Path,
    digest: &str,
    commit: &str,
) -> std::result::Result<Value, String> {
    use sha2::{Digest, Sha256};
    let mut evidence = Sha256::new();
    let mut results = json!({"screens":run.state["screens"],"edges":run.state["edges"]});
    // #1662: a backend run's command output is part of what the record vouches for.
    if !run.state["outputs"].is_null() {
        results["outputs"] = run.state["outputs"].clone();
    }
    evidence.update(results.to_string().as_bytes());
    let mut screens: Vec<&String> = run.state["screens"]
        .as_object()
        .map(|screens| screens.keys().collect())
        .unwrap_or_default();
    screens.sort();
    for screen in screens {
        if let Some(file) = run.state["screens"][screen.as_str()]["file"].as_str() {
            let bytes = std::fs::read(run.dir.join("frames").join(file))
                .map_err(|_| "a captured frame could not be read".to_string())?;
            evidence.update(Sha256::digest(&bytes));
        }
    }
    let runner = std::env::current_exe()
        .and_then(std::fs::read)
        .map_err(|_| "the runner binary could not be read".to_string())?;
    super::journey_owner::record_previewed(
        project,
        &run.flow_id,
        digest,
        commit,
        &super::journey_owner::PreviewEvidence {
            evidence_sha256: &hex::encode(evidence.finalize()),
            runner_sha256: &hex::encode(Sha256::digest(&runner)),
            version: env!("CARGO_PKG_VERSION"),
        },
    )?;
    // #1672 review: the record may predate this run (an existing record for the same flow,
    // digest and commit is kept, not rewritten), so the observer is the one the STORED record
    // names, read back, never this binary's own values.
    super::journey_owner::previewed_by(project, &run.flow_id, digest, commit)
        .ok_or_else(|| "the passing preview record could not be read back".to_string())
}

fn play(
    project: &Path,
    flow: &Value,
    run: &mut Run,
    deadline: Instant,
    stopper: &std::sync::Arc<std::sync::Mutex<Option<LaunchedStop>>>,
    views: &[View],
) -> Result<Option<Launched>> {
    // #927: a backend flow needs no browser, and an app only when it sends requests.
    let backend = super::journey_backend::is_backend(flow);
    let needs_app = !backend || super::journey_backend::needs_app(flow);
    if !backend {
        let setup = super::observers::ensure_playwright(project, deadline, |message| {
            run.state["observerSetup"] = json!({"state":"installing","message":message});
            run.save();
            eprintln!("{message}");
        });
        match setup {
            Ok(_) => {
                run.state["observerSetup"] =
                    json!({"state":"ready","message":"Browser observer ready"});
                run.save();
            }
            Err(error) => {
                run.state["observerSetup"] = json!({
                    "state":"failed", "message":"Browser observer setup failed",
                    "error":error["message"],
                });
                run.save();
                let code = if error["code"] == "observer.setup_timeout" {
                    "observer.setup_timeout"
                } else {
                    "observer.setup_failed"
                };
                return Err(failure(code, "/observer", 3));
            }
        }
    }
    if needs_app && run.validation && !launcher_isolated(project) {
        return Err(failure(
            "preview.launcher_not_isolated",
            "/launcher/isolated",
            2,
        ));
    }
    let mut base = flow["base"].as_str().unwrap().to_owned();
    // The app under test is started when it is down, and stopped when the preview ends. An
    // isolated launcher always starts its own on free ports (#585), and the preview plays there.
    let mut launched: Option<Launched> =
        if !needs_app || (!launcher_isolated(project) && base_reachable(&base)) {
            None
        } else {
            Some(start_app(project, &base, flow, run, stopper)?)
        };
    let flow_base = base.clone();
    if let Some(own) = launched.as_ref().and_then(|app| app.base.clone()) {
        base = own;
    }
    let secrets_of = |launched: &Option<Launched>| {
        if backend {
            Ok(Default::default())
        } else {
            preflight(
                flow,
                &launched
                    .as_ref()
                    .map(|app| app.secrets.clone())
                    .unwrap_or_default(),
            )
        }
    };
    let mut secrets = secrets_of(&launched)?;
    // #1662: a draft's commands run only when the owner's store confirms this exact content and
    // argv list; the held state names both, so the owner sees what a confirm would allow.
    let argv = super::journey_backend::cli_argv(flow);
    let approved = flow["status"] == "approved";
    let digest = super::journey_flow::approval_digest(flow);
    let cli_confirmed = !approved
        && !argv.is_empty()
        && super::journey_owner::cli_confirmed(project, &run.flow_id, &digest, &argv)
            .unwrap_or(false);
    // #1911: a draft whose flow file is byte-identical to the reviewed blob on origin/main runs
    // its commands without the owner's confirm; any other draft waits for review.
    let cli_reviewed = !approved && !argv.is_empty() && run.reviewed;
    if !approved && !argv.is_empty() {
        run.state["cliConfirm"] =
            json!({"digest": digest, "argv": argv, "confirmed": cli_confirmed});
        run.state["cliReviewed"] = json!(cli_reviewed);
    }
    let guard = Guard {
        approved,
        launched: launched.is_some(),
        confirmed: run.confirmed,
        cli_confirmed,
        cli_reviewed,
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
    // #1777: every viewport plays every path in its own browser, with its own share of the
    // budget. #1896: a launched app is stopped and started afresh before every viewport after the
    // first, so a flow that changes state finds the same starting state at each one.
    each_view(flow, views, run, |run, index, view| {
        if index > 0 && launched.is_some() {
            drop(launched.take());
            let app = start_app(project, &flow_base, flow, run, stopper)?;
            base = app.base.clone().unwrap_or_else(|| flow_base.clone());
            launched = Some(app);
            secrets = secrets_of(&launched)?;
        }
        run.first_view = index == 0;
        run.viewport = view.viewport.clone();
        run.device = view.device;
        run.frames_sub = if index == 0 {
            PathBuf::from("frames")
        } else {
            Path::new("frames").join(view.name)
        };
        run.state["viewport"] = view.name.into();
        run.save();
        let deadline = deadline.min(Instant::now() + PREVIEW_BUDGET);
        play_paths(
            project, &base, &secrets, &screens, &edges, &paths, backend, guard, deadline, run,
        )
    })?;
    // The runner retains ownership until the completed result is stored.
    Ok(launched)
}

/// Starts the project's launcher for `flow`. How to stop it is recorded before `up` returns, since
/// a dead runner cannot clean up itself, even if it dies before up returns.
fn start_app(
    project: &Path,
    base: &str,
    flow: &Value,
    run: &mut Run,
    stopper: &std::sync::Arc<std::sync::Mutex<Option<LaunchedStop>>>,
) -> Result<Launched> {
    launch(
        project,
        base,
        flow["id"].as_str().unwrap_or_default(),
        |stop| {
            run.state["launched"] = stop.record();
            super::atomic::atomic_write(
                &run.dir.join(STATE),
                &serde_json::to_vec_pretty(&run.state).unwrap(),
            )
            .map_err(|_| failure("watch.launch_failed", "/launcher", 1))?;
            if let Ok(mut slot) = stopper.lock() {
                *slot = Some(stop.clone());
            }
            Ok(())
        },
    )
}

/// #1777: plays `body` for each viewport in order and keeps what each recorded under
/// `viewports.<name>`. A viewport whose pass is held for the owner's click (`body` answers `true`)
/// is the last one played, and its results are kept under its own name, never a later one's.
fn each_view(
    flow: &Value,
    views: &[View],
    run: &mut Run,
    mut body: impl FnMut(&mut Run, usize, &View) -> Result<bool>,
) -> Result<()> {
    let mut played: Option<&'static str> = None;
    for (index, view) in views.iter().enumerate() {
        if let Some(previous) = played {
            stash_view(flow, &mut run.state, previous);
            run.state["screens"] = json!({});
            run.state["edges"] = json!({});
        }
        played = Some(view.name);
        let held = body(run, index, view);
        if !matches!(held, Ok(false)) {
            stash_view(flow, &mut run.state, view.name);
            return held.map(|_| ());
        }
    }
    if let Some(last) = played {
        stash_view(flow, &mut run.state, last);
    }
    Ok(())
}

/// #1777: one viewport's pass over every path. `true` when a path is held for the owner's click,
/// so nothing after it is played.
#[allow(clippy::too_many_arguments)]
fn play_paths(
    project: &Path,
    base: &str,
    secrets: &std::collections::BTreeMap<String, String>,
    screens: &std::collections::BTreeMap<&str, &Value>,
    edges: &std::collections::BTreeMap<&str, &Value>,
    paths: &[(&String, &Value)],
    backend: bool,
    guard: Guard,
    deadline: Instant,
    run: &mut Run,
) -> Result<bool> {
    for (name, path_edges) in paths {
        let name = name.as_str();
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
        if backend {
            play_backend_path(project, base, screens, edges, &ids, name, guard, run);
        } else {
            play_path(
                project, base, secrets, screens, edges, &ids, name, guard, run,
            )?;
        }
        if !run.state["held"].is_null() {
            // Waiting for the owner's click: nothing after the held act is played.
            return Ok(true);
        }
    }
    Ok(false)
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
    /// #1662: the owner's store holds a confirm of this draft's exact digest and argv list.
    cli_confirmed: bool,
    /// #1911: the draft's flow file is byte-identical to its blob on origin/main.
    cli_reviewed: bool,
}

impl Guard {
    /// The step reason an act is not sent with, or `None` when it is played.
    ///
    /// #927 backend acts: there is no isolated target for a command, so a `cli` act runs only on
    /// argv the owner has seen. #1662: in a draft it holds (`confirm_needed`) until the owner's
    /// store confirms this exact digest and argv list (approval needs a passing preview, so
    /// waiting for approval could never end). A mutating `http` act (any method but GET, HEAD,
    /// OPTIONS) in a draft is refused even on a launched app, and an approved flow holds it for
    /// the owner's click.
    fn stops_before(self, act: &Value) -> Option<&'static str> {
        let backend = matches!(act["kind"].as_str(), Some("http" | "cli"));
        // #1911: a draft's command runs when its flow is the reviewed one on main (or the owner
        // confirmed this exact argv, #1662); otherwise it waits for review.
        if act["kind"] == "cli" && !self.approved && !self.cli_confirmed && !self.cli_reviewed {
            return Some("review_needed");
        }
        would_destroy(act)?;
        if self.approved {
            (!self.confirmed).then_some("confirm_needed")
        } else {
            (backend || !self.launched).then_some("guard_refused")
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
    path_name: &str,
    guard: Guard,
    run: &mut Run,
) -> Result<()> {
    if run.validation {
        run.state["validationPaths"][path_name] =
            json!({"execution":"failed","collection":"unavailable"});
        run.save();
    }
    let output = TemporaryOutput::create()?;
    let mut driver = Driver::start(project, output.path(), secrets)?;
    // One exit from the live driver: even a held or failed path attempts collection and closes.
    let result = (|| -> Result<&'static str> {
        let first = edges[ids[0]]["from"].as_str().unwrap();
        let entry = format!(
            "{}{}",
            base.trim_end_matches('/'),
            screens[first]["url"].as_str().unwrap()
        );
        let mut request = with_declared(
            json!({"base":entry,"viewport":run.viewport,"allowOrigins":[],"survive":true}),
            run.declared.as_ref(),
        );
        if let Some(device) = run.device {
            request["device"] = device.into();
        }
        if run.validation {
            request["coverage"] = true.into();
        }
        driver.call("open", request, "/entry")?;
        let arrived = |driver: &mut Driver, run: &mut Run, screen: &str| -> Result<bool> {
            let observed = observe(driver, screens[screen], base, &format!("/screens/{screen}"));
            // #1286: each assertion's own seen/closest travels on the screen entry, so the Studio
            // marks them one by one instead of painting the step's status on all of them.
            let checks = driver.take_checks();
            match observed {
                Ok(_) => {
                    let frame = run.capture(driver, output.path(), screen);
                    run.prove(screen, frame.as_ref())?;
                    run.screen(screen, "pass", None, with_checks(frame, checks));
                    Ok(true)
                }
                Err((code, _, _))
                    if SURVIVABLE.contains(&code) || code == "replay.wrong_screen" =>
                {
                    let frame = run.capture(driver, output.path(), screen);
                    run.screen(
                        screen,
                        "drift",
                        Some(step_reason(code)),
                        with_checks(frame, checks),
                    );
                    Ok(false)
                }
                Err(failed) => Err(failed),
            }
        };
        if !arrived(&mut driver, run, first)? {
            return Ok("drift");
        }
        for id in ids {
            let edge = edges[id];
            let to = edge["to"].as_str().unwrap();
            for act in edge["acts"].as_array().unwrap() {
                if let Some(reason) = guard.stops_before(act) {
                    run.edge(id, "skipped", Some(reason));
                    if matches!(reason, "confirm_needed" | "review_needed") {
                        run.state["held"] = json!({"edge":id,"act":act["name"],"base":base});
                        if act["kind"] == "cli" {
                            run.state["held"]["cliConfirm"] = run.state["cliConfirm"].clone();
                        }
                        run.save();
                        return Ok("held");
                    }
                    return Ok("skipped");
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
                        let frame = run.capture(&mut driver, output.path(), to);
                        run.screen(to, "fail", Some(reason), frame);
                        return Ok("failed");
                    }
                    Err(failed) => return Err(failed),
                }
            }
            if arrived(&mut driver, run, to)? {
                run.edge(id, "pass", None);
            } else {
                run.edge(id, "drift", Some("expect_missing"));
                return Ok("drift");
            }
        }
        Ok("completed")
    })();
    if run.validation {
        let execution = result.as_ref().copied().unwrap_or("failed");
        let collection = finalize_coverage(&mut driver, run, output.path(), path_name, execution);
        run.state["validationPaths"][path_name] =
            json!({"execution":execution,"collection":collection});
        run.save();
    }
    let _ = driver.close();
    result.map(|_| ())
}

/// #927: one path of a backend flow, recorded exactly as a browser path is. The start screen is
/// observed by a GET of its URL when the flow sends requests; a `cli`-only flow's start screen is
/// its precondition and is not observed. Each act's observation is checked against the screen it
/// reaches: an act that cannot run fails, an unmet expectation is drift. No coverage is collected.
/// #1662 review: what a command printed, as kept in the run state and its evidence. stdout and
/// stderr are masked like the driver's stderr (every `GRAPHHELM_SECRET_*` value and the user's
/// home) and capped at the tail; the sha256 of the raw bytes stays so the evidence binds them.
fn command_output(argv: &Value, seen: &super::journey_backend::Observation) -> Value {
    let secrets: Vec<String> = std::env::vars_os()
        .filter(|(key, _)| key.to_string_lossy().starts_with("GRAPHHELM_SECRET_"))
        .filter_map(|(_, value)| value.into_string().ok())
        .collect();
    masked_output(argv, seen, &secrets)
}

fn masked_output(
    argv: &Value,
    seen: &super::journey_backend::Observation,
    secrets: &[String],
) -> Value {
    use sha2::{Digest, Sha256};
    // The whole output is kept (no cut window), then its tail is bounded by `masked_tail`.
    let cut = false;
    let keep = |text: &str| super::journey_replay::masked_tail(text.as_bytes(), cut, 0, secrets);
    json!({
        "argv": argv,
        "exit": seen.exit,
        "stdout": keep(&seen.body),
        "stderr": keep(&seen.stderr),
        "stdoutSha256": hex::encode(Sha256::digest(seen.body.as_bytes())),
        "stderrSha256": hex::encode(Sha256::digest(seen.stderr.as_bytes())),
    })
}

#[allow(clippy::too_many_arguments)]
fn play_backend_path(
    project: &Path,
    base: &str,
    screens: &std::collections::BTreeMap<&str, &Value>,
    edges: &std::collections::BTreeMap<&str, &Value>,
    ids: &[&str],
    path_name: &str,
    guard: Guard,
    run: &mut Run,
) {
    use super::journey_backend as backend;
    let mut walk = || -> &'static str {
        let first = edges[ids[0]]["from"].as_str().unwrap();
        if backend::needs_app(&json!({"edges": ids.iter().map(|id| edges[id]).collect::<Vec<_>>()}))
        {
            let url = screens[first]["url"].as_str().unwrap();
            match backend::request(base, "GET", url, None, None) {
                Ok(seen) => match backend::unmet(screens[first], &seen) {
                    None => run.screen(first, "pass", None, None),
                    Some(_) => {
                        run.screen(first, "drift", Some("expect_missing"), None);
                        return "drift";
                    }
                },
                Err(_) => {
                    run.screen(first, "fail", Some("act_failed"), None);
                    return "failed";
                }
            }
        }
        for id in ids {
            let edge = edges[id];
            let to = edge["to"].as_str().unwrap();
            let mut last = None;
            for act in edge["acts"].as_array().unwrap() {
                if let Some(reason) = guard.stops_before(act) {
                    run.edge(id, "skipped", Some(reason));
                    if matches!(reason, "confirm_needed" | "review_needed") {
                        run.state["held"] = json!({"edge":id,"act":act["name"],"base":base});
                        if act["kind"] == "cli" {
                            run.state["held"]["cliConfirm"] = run.state["cliConfirm"].clone();
                        }
                        run.save();
                        return "held";
                    }
                    return "skipped";
                }
                match backend::play(project, base, act) {
                    Ok(seen) => {
                        // #1662: what each command printed is kept with the run's evidence.
                        if act["kind"] == "cli" {
                            let mut outputs = run.state["outputs"][id]
                                .as_array()
                                .cloned()
                                .unwrap_or_default();
                            outputs.push(command_output(&act["name"], &seen));
                            run.state["outputs"][id] = Value::Array(outputs);
                        }
                        last = Some(seen);
                    }
                    Err(_) => {
                        run.edge(id, "fail", Some("act_failed"));
                        run.screen(to, "fail", Some("act_failed"), None);
                        return "failed";
                    }
                }
            }
            if last.is_some_and(|seen| backend::unmet(screens[to], &seen).is_none()) {
                run.screen(to, "pass", None, None);
                run.edge(id, "pass", None);
            } else {
                run.screen(to, "drift", Some("expect_missing"), None);
                run.edge(id, "drift", Some("expect_missing"));
                return "drift";
            }
        }
        "completed"
    };
    let execution = walk();
    if run.validation {
        run.state["validationPaths"][path_name] =
            json!({"execution":execution,"collection":"not_applicable"});
    }
    run.save();
}

fn finalize_coverage(
    driver: &mut Driver,
    run: &Run,
    output: &Path,
    path_name: &str,
    execution: &str,
) -> &'static str {
    let save = || -> Option<&'static str> {
        let result = driver.call("coverage", json!({}), "/coverage").ok()?;
        if result["path"] != "coverage.json" {
            return None;
        }
        let source = output.join("coverage.json");
        if !safe_node(&source) || std::fs::metadata(&source).ok()?.len() > 16 * 1024 * 1024 {
            return None;
        }
        let mut artifact: Value = serde_json::from_slice(&std::fs::read(source).ok()?).ok()?;
        if artifact["schema"] != "graphhelm-js-coverage/1" {
            return None;
        }
        artifact["flow"] = run.flow_id.clone().into();
        artifact["path"] = path_name.into();
        artifact["execution"] = execution.into();
        let dir = run.dir.join("coverage").join(sha_of(path_name));
        std::fs::create_dir_all(&dir).ok()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join("coverage.json"))
            .ok()?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec(&artifact).ok()?).ok()?;
        Some(
            if artifact["collection"] == "captured" && artifact["completeness"]["complete"] == true
            {
                "captured"
            } else {
                "incomplete"
            },
        )
    };
    let mut save = save;
    save().unwrap_or("unavailable")
}

/// `GET /v1/journey-flows/{id}/screens/{screen}/frame`: one screen's frame from the current
/// preview, with the tag it is served under (the flow digest's first twelve hex digits plus the
/// commit's, so a new run of the same flow and commit keeps the tag only when nothing changed).
/// #1777: the recorded entry and file of one screen's frame at `viewport` in a run's `dir`. No
/// viewport, or the run's first one, is the top-level entry in `frames/` (what every run before
/// #1777 recorded); any other viewport is its own entry in `frames/<viewport>/`.
fn viewport_frame(
    dir: &Path,
    state: &Value,
    screen: &str,
    viewport: Option<&str>,
) -> Option<(Value, PathBuf)> {
    let first = state["viewportsPlanned"][0].as_str().unwrap_or("web");
    let (entry, folder) = match viewport {
        None => (state["screens"][screen].clone(), dir.join("frames")),
        Some(name) if name == first => (state["screens"][screen].clone(), dir.join("frames")),
        Some(name @ ("web" | "mobile")) => (
            state["viewports"][name]["screens"][screen].clone(),
            dir.join("frames").join(name),
        ),
        Some(_) => return None,
    };
    if entry["frame"] != true || entry["file"] != format!("{screen}.png").as_str() {
        return None;
    }
    let file = folder.join(format!("{screen}.png"));
    (safe_node(&file) && std::fs::metadata(&file).ok()?.len() <= FRAME_LIMIT)
        .then_some((entry, file))
}

pub(crate) fn screen_frame(
    project: &Path,
    id: &str,
    screen: &str,
    viewport: Option<&str>,
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
    let (entry, file) = viewport_frame(&dir, &state, screen, viewport)?;
    let bytes = std::fs::read(file).ok()?;
    let started = state["startedAt"].as_str().unwrap_or_default();
    let digest_hex = digest.trim_start_matches("sha256:");
    let tag = format!(
        "{}-{}{}",
        &digest_hex[..12.min(digest_hex.len())],
        &sha_of(started)[..8],
        viewport.map_or(String::new(), |name| format!("-{name}"))
    );
    Some((
        bytes,
        tag,
        entry["width"].as_u64().unwrap_or(0),
        entry["height"].as_u64().unwrap_or(0),
    ))
}

/// How many finished runs a journey keeps in `history/` (#history): enough to see when a screen
/// changed, bounded so a project replayed all day does not fill its disk.
const HISTORY_KEEP: usize = 20;

/// A finished run's id in `history/`: its start time in milliseconds, zero-padded so the names
/// sort in time order.
fn history_id(state: &Value) -> Option<String> {
    let started = chrono::DateTime::parse_from_rfc3339(state["startedAt"].as_str()?).ok()?;
    Some(format!("{:020}", started.timestamp_millis().max(0)))
}

/// Before a new run replaces it, the finished run (its state and frames) moves to
/// `history/<id>/`, and only the newest `HISTORY_KEEP` stay. Best effort: a run that cannot be
/// archived is simply not in the history.
fn archive(dir: &Path, previous: &Value) {
    let Some(id) = history_id(previous) else {
        return;
    };
    let history = dir.join("history");
    let target = history.join(&id);
    if std::fs::create_dir_all(&target).is_err() || safe_directory(&target, false).is_err() {
        return;
    }
    let frames = dir.join("frames");
    if safe_directory(&frames, false).is_ok() {
        let _ = std::fs::remove_dir_all(target.join("frames"));
        let _ = std::fs::rename(&frames, target.join("frames"));
    }
    save_state(&target, previous);
    let mut runs = history_ids(&history);
    while runs.len() > HISTORY_KEEP {
        let oldest = runs.remove(0);
        let _ = std::fs::remove_dir_all(history.join(oldest));
    }
}

/// The archived run ids, oldest first.
fn history_ids(history: &Path) -> Vec<String> {
    let mut runs: Vec<String> = std::fs::read_dir(history)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .filter(|name| name.len() == 20 && name.chars().all(|c| c.is_ascii_digit()))
                .collect()
        })
        .unwrap_or_default();
    runs.sort();
    runs
}

/// `GET /v1/journey-flows/{id}/history` (#history): the journey's earlier finished runs, newest
/// first, each with its verdict, time, flow digest, commit and per-screen results. The current
/// run is the preview route's; this is only what came before it. #1956: a finished run kept for
/// an earlier version of the flow (its digest or scope commit no longer the flow's) is no longer
/// the preview route's current run, which reads `none`; it is listed here as the newest entry,
/// under the id it will be archived with, so its verdict and frames stay readable. #2011: the same
/// holds for a finished run of the current version: it is listed too, not only after the next run.
pub(crate) fn history(project: &Path, id: &str) -> Outcome {
    if !plain_id(id) {
        return answer(json!({}), Some(failure("preview.flow_unknown", "/id", 2)));
    }
    if let Err(failed) = flow_and_key(project, id) {
        return answer(json!({}), Some(failed));
    }
    let dir = dir_of(project, id);
    let history = dir.join("history");
    let entry = |run: String, state: Value| {
        let mut entry = json!({"run": run});
        for key in [
            "state",
            "kind",
            "result",
            "reason",
            "digest",
            "commit",
            "startedAt",
            "ranAt",
            "screens",
            "edges",
            "detail",
        ] {
            if !state[key].is_null() {
                entry[key] = state[key].clone();
            }
        }
        entry
    };
    let archived = history_ids(&history);
    let earlier = slot_run(&dir)
        .filter(|(run, _)| !archived.contains(run))
        .map(|(run, state)| entry(run, state));
    let runs: Vec<Value> = earlier
        .into_iter()
        .chain(archived.into_iter().rev().filter_map(|run| {
            let state = read_state(&history.join(&run))?;
            Some(entry(run, state))
        }))
        .collect();
    answer(json!({"id": id, "runs": runs}), None)
}

/// #1795 (R4a): the journey's newest finished run on this Runtime's checkout (the "Before = main"
/// side of a compare), as `{run, current, commit, ranAt, result, screens: [{screen, frameUrl}]}`:
/// the current run when it finished, else the newest run in `history/`. `None` when the journey is
/// unknown or never finished a run. Frame URLs are the existing frame routes, relative to the
/// Runtime: no host path leaves this function.
pub(crate) fn newest_finished(project: &Path, id: &str) -> Option<Value> {
    if !plain_id(id) {
        return None;
    }
    let dir = dir_of(project, id);
    let finished = |state: &Value| matches!(state["state"].as_str(), Some("ready" | "failed"));
    let (state, run, current) = match read_state(&dir).filter(finished) {
        Some(state) => {
            let run = history_id(&state)?;
            (state, run, true)
        }
        None => {
            let history = dir.join("history");
            let run = history_ids(&history).into_iter().next_back()?;
            (
                read_state(&history.join(&run)).filter(finished)?,
                run,
                false,
            )
        }
    };
    let mut screens: Vec<Value> = state["screens"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(screen, entry)| {
            plain_id(screen)
                && entry["frame"] == true
                && entry["file"] == format!("{screen}.png").as_str()
        })
        .map(|(screen, _)| {
            let url = if current {
                format!("/v1/journey-flows/{id}/screens/{screen}/frame")
            } else {
                format!("/v1/journey-flows/{id}/history/{run}/screens/{screen}/frame")
            };
            // #1777: each viewport that kept a frame of this screen, by its own URL.
            let mut viewports = serde_json::Map::new();
            for (name, view) in state["viewports"].as_object().into_iter().flatten() {
                if view["screens"][screen.as_str()]["frame"] == true {
                    viewports.insert(name.clone(), format!("{url}?viewport={name}").into());
                }
            }
            let mut entry = json!({"screen": screen, "frameUrl": url});
            if !viewports.is_empty() {
                entry["viewports"] = viewports.into();
            }
            entry
        })
        .collect();
    screens.sort_by(|a, b| a["screen"].as_str().cmp(&b["screen"].as_str()));
    let text = |key: &str| state[key].as_str().map(str::to_owned);
    Some(json!({
        "run": run,
        "current": current,
        "commit": text("commit"),
        "ranAt": text("ranAt"),
        "result": text("result"),
        "screens": screens,
    }))
}

/// #1795: a journey's title as its flow names it, or the id when the flow cannot be read.
pub(crate) fn journey_title(project: &Path, id: &str) -> String {
    flow_and_key(project, id)
        .ok()
        .and_then(|(flow, _, _)| flow["title"].as_str().map(str::to_owned))
        .unwrap_or_else(|| id.to_owned())
}

/// `GET /v1/journey-flows/{id}/history/{run}/screens/{screen}/frame`: one screen's frame as an
/// earlier run kept it. The run's id never changes, so its bytes never do.
pub(crate) fn history_frame(
    project: &Path,
    id: &str,
    run: &str,
    screen: &str,
    viewport: Option<&str>,
) -> Option<Vec<u8>> {
    if !plain_id(id)
        || !plain_id(screen)
        || run.len() != 20
        || !run.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let flow_dir = dir_of(project, id);
    let dir = flow_dir.join("history").join(run);
    let (dir, state) = match read_state(&dir) {
        Some(state) => (dir, state),
        // #2011: the slot's run the history lists before it is archived.
        None => {
            flow_and_key(project, id).ok()?;
            let (kept, state) = slot_run(&flow_dir)?;
            (kept == run).then_some((flow_dir, state))?
        }
    };
    let (_, file) = viewport_frame(&dir, &state, screen, viewport)?;
    std::fs::read(file).ok()
}

/// #2011: the finished run in the journey's current slot, with the history id it will be archived
/// under, whether it was kept for this version of the flow or an earlier one (#1956). A run only
/// moves to `history/` when the next one starts, so until then it would be missing from the
/// history list although it is the newest run. `None` when the slot is empty or still running.
fn slot_run(dir: &Path) -> Option<(String, Value)> {
    let state = read_state(dir)?;
    if !matches!(state["state"].as_str(), Some("ready" | "failed")) {
        return None;
    }
    Some((history_id(&state)?, state))
}

/// #1835: the journey's last finished run, as `journey flows` lists it. Read only from what the
/// runner recorded: the kept `state.json` when it finished, else (a run in flight replaced it) the
/// newest archived run. At most two state files are read and nothing is written, probed or
/// reaped. No finished run is `not_run`, never a guessed verdict. `status` is `passed` only for
/// a finished run whose result is `pass`; a `fail`, a `drift` or a run that stopped is `failed`.
/// `failingStep` is the first step that did not pass in path order (each path's entry screen,
/// then each edge and the screen it reaches), else in the run's own sorted keys; `null` when a
/// run stopped before any step (its `reason` says why). `flowChanged` says the flow's approval
/// digest moved since that run. #1777: `viewports` is each viewport's status (a run recorded
/// before #1777 is `web` alone, `mobile` `not_run`), `failsOnlyOn` names the one viewport that
/// failed while every other one passed (else `null`), and `failingStep.viewport` the viewport its
/// step failed at when that is one.
pub(crate) fn last_run(project: &Path, id: &str, flow: Option<&Value>) -> Value {
    let not_run = json!({"status":"not_run","ranAt":null,"kind":null,"reason":null,
        "failingStep":null,"viewports":{"web":"not_run","mobile":"not_run"},"failsOnlyOn":null,
        "flowChanged":null});
    if !plain_id(id) {
        return not_run;
    }
    let dir = dir_of(project, id);
    let finished = |state: &Value| matches!(state["state"].as_str(), Some("ready" | "failed"));
    let state = read_state(&dir).filter(finished).or_else(|| {
        let history = dir.join("history");
        let newest = history_ids(&history).pop()?;
        read_state(&history.join(newest)).filter(finished)
    });
    let Some(state) = state else { return not_run };
    let passed = state["state"] == "ready" && state["result"] == "pass";
    let text = |key: &str| state[key].as_str().map_or(Value::Null, Value::from);
    let (viewports, fails_only_on) = viewport_statuses(&state, passed);
    let mut failing = if passed {
        Value::Null
    } else {
        first_failing(&state, flow)
    };
    if failing.is_object() {
        let at = &state[if failing["kind"] == "screen" {
            "screens"
        } else {
            "edges"
        }][failing["id"].as_str().unwrap_or_default()]["failedAt"];
        failing["viewport"] = match at.as_array().map(Vec::as_slice) {
            Some([one]) => one.clone(),
            _ => Value::Null,
        };
    }
    json!({"status": if passed { "passed" } else { "failed" },
    "ranAt": text("ranAt"), "kind": text("kind"), "reason": text("reason"),
    "failingStep": failing, "viewports": viewports, "failsOnlyOn": fails_only_on,
    "flowChanged": flow.map_or(Value::Null, |flow| {
        (state["digest"] != super::journey_flow::approval_digest(flow).as_str()).into()
    })})
}

/// #1953: the journey's run in flight, as `journey flows` lists it next to `lastRun`, so a Studio
/// that reloads, or did not start the run, still shows it running. Read only from the kept
/// `state.json`: nothing is written, probed or reaped. A `running` state past its budget plus the
/// grace has no runner (`within_budget`), so it is `null` rather than running for ever; a runner
/// that died inside the budget reads running until the next preview read reaps it. `current` is
/// the step the runner is on, `null` before its first step.
pub(crate) fn running(project: &Path, id: &str) -> Value {
    if !plain_id(id) {
        return Value::Null;
    }
    match read_state(&dir_of(project, id)) {
        Some(state) if state["state"] == "running" && within_budget(&state) => {
            let text = |key: &str| state[key].as_str().map_or(Value::Null, Value::from);
            json!({"startedAt": text("startedAt"), "current": text("current"), "kind": text("kind")})
        }
        _ => Value::Null,
    }
}

/// #1777: each viewport's status in a finished run, and the one viewport that alone failed. A run
/// recorded before #1777 has no `viewports`: it proved web only, so mobile is `not_run` and no
/// viewport is named as the only one that failed.
fn viewport_statuses(state: &Value, passed: bool) -> (Value, Value) {
    let Some(views) = state["viewports"].as_object() else {
        let web = if passed { "passed" } else { "failed" };
        return (json!({"web": web, "mobile": "not_run"}), Value::Null);
    };
    let finished = state["state"] == "ready";
    let mut statuses = json!({"web": "not_run", "mobile": "not_run"});
    for (name, view) in views {
        if name == "web" || name == "mobile" {
            statuses[name] = if finished && view["result"] == "pass" {
                "passed"
            } else {
                "failed"
            }
            .into();
        }
    }
    let ran: Vec<&str> = ["web", "mobile"]
        .into_iter()
        .filter(|name| statuses[*name] != "not_run")
        .collect();
    let failed: Vec<&str> = ran
        .iter()
        .copied()
        .filter(|name| statuses[*name] == "failed")
        .collect();
    let only = match (finished, ran.len(), failed.as_slice()) {
        (true, 2, [one]) => Value::from(*one),
        _ => Value::Null,
    };
    (statuses, only)
}

/// The first recorded step whose result is `fail` or `drift`: in the flow's path order when the
/// flow is readable, then in the run's own sorted screen and edge keys.
fn first_failing(state: &Value, flow: Option<&Value>) -> Value {
    let mut order: Vec<(&str, String)> = Vec::new();
    if let Some(flow) = flow {
        let edges = flow["edges"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        for path in flow["paths"]
            .as_object()
            .into_iter()
            .flat_map(|p| p.values())
        {
            for (index, step) in path.as_array().into_iter().flatten().enumerate() {
                let Some(edge) = edges.iter().find(|edge| edge["id"] == *step) else {
                    continue;
                };
                if index == 0
                    && let Some(from) = edge["from"].as_str()
                {
                    order.push(("screen", from.to_owned()));
                }
                if let Some(id) = edge["id"].as_str() {
                    order.push(("edge", id.to_owned()));
                }
                if let Some(to) = edge["to"].as_str() {
                    order.push(("screen", to.to_owned()));
                }
            }
        }
    }
    for (kind, key) in [("screen", "screens"), ("edge", "edges")] {
        for id in state[key].as_object().into_iter().flat_map(|m| m.keys()) {
            order.push((kind, id.clone()));
        }
    }
    order
        .into_iter()
        .find_map(|(kind, id)| {
            let entry = &state[if kind == "screen" { "screens" } else { "edges" }][&id];
            matches!(entry["result"].as_str(), Some("fail" | "drift")).then(|| {
                json!({"kind": kind, "id": id, "result": entry["result"],
                    "reason": entry["reason"].as_str().map_or(Value::Null, Value::from)})
            })
        })
        .unwrap_or(Value::Null)
}

fn sha_of(text: &str) -> String {
    graphhelm_protocols::hash::sha256_hex(text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::{
        Guard, HISTORY_KEEP, JourneyPreviewArgs, RUNNER_LOG, archive, body, dir_of, failed_reason,
        history_frame, history_id, history_ids, newest_finished, plain_id, proof_args, read_state,
        reap_lost, reap_lost_from, reap_lost_from_with, runner_liveness, runner_liveness_with,
        save_state, severity, step_reason, unsealed_proof, watch_budget, with_checks,
    };

    /// #1795: the "Before = main" run is the newest FINISHED one: a run still in progress is
    /// skipped for the newest archived run, and a finished current run is served at the current
    /// frame route. Frame URLs are route-relative; a screen without a kept frame is left out.
    /// #1777 (#1863 review): a pass held for the owner's click is the last one played, and its
    /// results stay under its own viewport. Defects named: a run held at web stashed as mobile,
    /// or the viewport after a held one still played. Cost: in memory only.
    #[test]
    fn a_pass_held_at_web_is_kept_as_web_and_mobile_is_not_played() {
        use super::*;
        let flow = json!({"screens": [{"id": "cart"}], "edges": [], "paths": {}});
        let mut run = Run {
            dir: std::path::PathBuf::from("unused"),
            state: json!({"screens": {}, "edges": {}}),
            frames: 0,
            proof: None,
            contract: String::new(),
            last: None,
            confirmed: false,
            viewport: Value::Null,
            declared: None,
            device: None,
            frames_sub: std::path::PathBuf::from("frames"),
            first_view: true,
            validation: false,
            flow_id: String::new(),
            reviewed: false,
        };
        let mut played = Vec::new();
        each_view(&flow, &views_of(&json!({})), &mut run, |run, _, view| {
            played.push(view.name);
            run.state["screens"]["cart"] = json!({"frame": false, "result": "pass"});
            Ok(view.name == "web")
        })
        .unwrap();
        assert_eq!(played, ["web"], "nothing after a held pass is played");
        let kept: Vec<_> = run.state["viewports"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(kept, ["web"], "{}", run.state);
        assert_eq!(
            run.state["viewports"]["web"]["screens"]["cart"]["result"],
            "pass"
        );
        // Unheld, both are played and kept under their own names.
        let mut played = Vec::new();
        run.state = json!({"screens": {}, "edges": {}});
        each_view(&flow, &views_of(&json!({})), &mut run, |_, _, view| {
            played.push(view.name);
            Ok(false)
        })
        .unwrap();
        assert_eq!(played, ["web", "mobile"]);
        assert_eq!(run.state["viewports"].as_object().unwrap().len(), 2);
    }

    /// #1777: which viewports a Run plays. Defects named: a flow with no viewport field proves only
    /// web, `viewports` is ignored, a fixed `viewport` (#585) gains a second size, or mobile is
    /// played without the driver's mobile device.
    #[test]
    fn a_run_plays_web_and_mobile_unless_the_flow_keeps_fewer() {
        use super::*;
        let names = |flow: Value| {
            views_of(&flow)
                .into_iter()
                .map(|v| v.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(json!({})), ["web", "mobile"]);
        assert_eq!(names(json!({"viewports": ["mobile"]})), ["mobile"]);
        assert_eq!(
            names(json!({"viewports": ["mobile", "web"]})),
            ["web", "mobile"]
        );
        let both = views_of(&json!({}));
        assert_eq!(both[0].viewport, json!({"width": 1280, "height": 800}));
        assert_eq!(both[0].device, None);
        assert_eq!(both[1].viewport, json!({"width": 390, "height": 844}));
        assert_eq!(both[1].device, Some("mobile"));
        // A fixed size is that size alone, named by its width, with no device emulation added.
        let fixed = views_of(&json!({"viewport": {"width": 390, "height": 844}}));
        assert_eq!(fixed.len(), 1);
        assert_eq!((fixed[0].name, fixed[0].device), ("mobile", None));
        assert_eq!(
            views_of(&json!({"viewport": {"width": 1440, "height": 900}}))[0].name,
            "web"
        );
        // Each viewport gets the whole preview budget; an older state is one viewport.
        assert_eq!(budget_of(&json!({})), PREVIEW_BUDGET);
        assert_eq!(
            budget_of(&json!({"viewportsPlanned": ["web", "mobile"]})),
            PREVIEW_BUDGET * 2
        );
    }

    /// #1777: a step that passes at web and fails at mobile fails the run, and the run says it
    /// fails only on mobile, on which step; a run recorded before #1777 reads as web only with
    /// mobile not run and names no viewport. Defects named: the viewports combined with OR, the
    /// top-level shape changed for older readers, or an old run read as passing on mobile.
    #[test]
    fn a_break_seen_only_at_mobile_fails_the_run_and_is_named() {
        use super::*;
        let flow = json!({"screens": [{"id": "cart"}, {"id": "pay"}], "edges": [{"id": "cart.pay", "from": "cart", "to": "pay"}],
            "paths": {"main": ["cart.pay"]}});
        let pass = json!({"frame": true, "file": "cart.png", "result": "pass", "width": 1280, "height": 800});
        let mut state = json!({"state": "running", "viewportsPlanned": ["web", "mobile"],
            "screens": {"cart": pass, "pay": {"frame": true, "file": "pay.png", "result": "pass"}},
            "edges": {"cart.pay": {"result": "pass"}}});
        stash_view(&flow, &mut state, "web");
        state["screens"] = json!({"cart": {"frame": true, "file": "cart.png", "result": "pass"},
            "pay": {"frame": true, "file": "pay.png", "result": "fail", "reason": "expectation_failed"}});
        state["edges"] = json!({"cart.pay": {"result": "pass"}});
        stash_view(&flow, &mut state, "mobile");
        fill_not_reached(&flow, &mut state);
        combine_views(&mut state);
        // The top level keeps the first viewport's frame, with the worst result at any viewport.
        assert_eq!(state["screens"]["cart"]["width"], 1280);
        assert_eq!(state["screens"]["cart"]["result"], "pass");
        assert_eq!(state["screens"]["pay"]["result"], "fail");
        assert_eq!(state["screens"]["pay"]["failedAt"], json!(["mobile"]));
        assert_eq!(
            run_verdict(&flow, &state["screens"], &state["edges"]).0,
            "fail"
        );
        assert_eq!(state["viewports"]["web"]["result"], "pass");
        assert_eq!(state["viewports"]["mobile"]["result"], "fail");

        let project = tempfile::tempdir().unwrap();
        let dir = dir_of(project.path(), "checkout");
        std::fs::create_dir_all(&dir).unwrap();
        state["state"] = "ready".into();
        state["result"] = "fail".into();
        save_state(&dir, &state);
        let last = last_run(project.path(), "checkout", Some(&flow));
        assert_eq!(last["status"], "failed");
        assert_eq!(
            last["viewports"],
            json!({"web": "passed", "mobile": "failed"})
        );
        assert_eq!(last["failsOnlyOn"], "mobile");
        assert_eq!(last["failingStep"]["id"], "pay");
        assert_eq!(last["failingStep"]["viewport"], "mobile");

        // A state written before #1777: web alone, mobile not run, no viewport named.
        save_state(
            &dir,
            &json!({"state": "ready", "result": "fail",
            "screens": {"pay": {"frame": false, "result": "fail"}}, "edges": {}}),
        );
        let old = last_run(project.path(), "checkout", Some(&flow));
        assert_eq!(
            old["viewports"],
            json!({"web": "failed", "mobile": "not_run"})
        );
        assert_eq!(old["failsOnlyOn"], Value::Null);
        assert_eq!(old["failingStep"]["viewport"], Value::Null);
        save_state(
            &dir,
            &json!({"state": "ready", "result": "pass", "screens": {}, "edges": {}}),
        );
        let old = last_run(project.path(), "checkout", Some(&flow));
        assert_eq!(
            old["viewports"],
            json!({"web": "passed", "mobile": "not_run"})
        );
    }

    /// #1777: each viewport's frame is served from its own file; no viewport is the first one's,
    /// exactly the file every run before #1777 kept. Defects named: mobile served the web frame,
    /// an old run answers a mobile frame it never took, or a crafted viewport reads another file.
    #[test]
    fn each_viewport_serves_its_own_frame_and_an_old_run_has_only_web() {
        use super::*;
        let dir = tempfile::tempdir().unwrap();
        let frames = dir.path().join("frames");
        std::fs::create_dir_all(frames.join("mobile")).unwrap();
        std::fs::write(frames.join("cart.png"), b"web").unwrap();
        std::fs::write(frames.join("mobile").join("cart.png"), b"mobile").unwrap();
        let entry = json!({"frame": true, "file": "cart.png"});
        let state = json!({"viewportsPlanned": ["web", "mobile"], "screens": {"cart": entry},
            "viewports": {"web": {"screens": {"cart": entry}}, "mobile": {"screens": {"cart": entry}}}});
        let read = |viewport| {
            viewport_frame(dir.path(), &state, "cart", viewport)
                .map(|(_, file)| std::fs::read(file).unwrap())
        };
        assert_eq!(read(None).unwrap(), b"web");
        assert_eq!(read(Some("web")).unwrap(), b"web");
        assert_eq!(read(Some("mobile")).unwrap(), b"mobile");
        assert!(read(Some("tablet")).is_none());
        let old = json!({"screens": {"cart": entry}});
        assert!(viewport_frame(dir.path(), &old, "cart", Some("mobile")).is_none());
        assert!(viewport_frame(dir.path(), &old, "cart", None).is_some());
    }

    #[test]
    fn the_before_run_is_the_newest_finished_one() {
        let project = tempfile::tempdir().unwrap();
        let dir = dir_of(project.path(), "checkout");
        let older = dir.join("history").join("00000001760000000000");
        std::fs::create_dir_all(&older).unwrap();
        save_state(
            &older,
            &serde_json::json!({"state":"ready","result":"pass","commit":"c0ffee",
            "startedAt":"2025-10-09T08:53:20Z","ranAt":"2025-10-09T08:54:00Z",
            "screens":{"cart":{"frame":true,"file":"cart.png","result":"pass"},"pay":{"result":"pass"}}}),
        );
        save_state(
            &dir,
            &serde_json::json!({"state":"running","startedAt":"2026-10-22T00:00:00Z"}),
        );
        let before = newest_finished(project.path(), "checkout").unwrap();
        assert_eq!(before["run"], "00000001760000000000");
        assert_eq!(before["current"], false);
        assert_eq!(before["commit"], "c0ffee");
        assert_eq!(
            before["screens"],
            serde_json::json!([{"screen":"cart","frameUrl":"/v1/journey-flows/checkout/history/00000001760000000000/screens/cart/frame"}])
        );
        save_state(
            &dir,
            &serde_json::json!({"state":"failed","result":"fail","commit":"beef01",
            "startedAt":"2026-10-22T00:00:00Z","ranAt":"2026-10-22T00:01:00Z",
            "screens":{"cart":{"frame":true,"file":"cart.png","result":"fail"}}}),
        );
        let before = newest_finished(project.path(), "checkout").unwrap();
        assert_eq!(
            (before["current"].clone(), before["result"].clone()),
            (serde_json::json!(true), serde_json::json!("fail"))
        );
        assert_eq!(
            before["screens"][0]["frameUrl"],
            "/v1/journey-flows/checkout/screens/cart/frame"
        );
        assert!(newest_finished(project.path(), "never-run").is_none());
        assert!(newest_finished(project.path(), "../escape").is_none());
    }

    /// #1286: a screen keeps its frame fields and gains `checks`; without checks nothing changes.
    /// Cost: microseconds, in-memory JSON.
    #[test]
    fn a_screen_entry_carries_the_drivers_checks() {
        let checks = json!([{"role":"heading","name":"A","seen":false,"closest":null}]);
        assert_eq!(with_checks(None, None), None);
        assert_eq!(
            with_checks(None, Some(checks.clone())),
            Some(json!({"frame":false,"checks":checks}))
        );
        let frame = json!({"frame":true,"path":"f.png"});
        assert_eq!(with_checks(Some(frame.clone()), None), Some(frame.clone()));
        let mut expected = frame.clone();
        expected["checks"] = checks.clone();
        assert_eq!(with_checks(Some(frame), Some(checks)), Some(expected));
    }
    use serde_json::{Value, json};
    use std::path::Path;

    /// #676: the real preview must publish its result before the launcher's down runs.
    /// Existing reaper cells start with supplied states and never observe this boundary.
    /// Cost: one isolated Studio preview; opt-in Node, Playwright/Chromium and built CLI.
    /// No production seam: the declared launcher reads the public persisted preview state.
    #[test]
    #[ignore = "requires an isolated Studio fixture and browser toolchain"]
    fn a_finished_preview_is_ready_before_launcher_down() {
        let root = std::env::var_os("GRAPHHELM_PREVIEW_OBSERVER_ROOT")
            .expect("OBSERVER_MISSING: browser toolchain directory");
        let project = tempfile::tempdir_in(root).unwrap();
        let project = project.path();
        // #1808: a studio2 flow of the checkout under test, so the copied scope is the live app's.
        let id = "studio2-open";
        std::fs::create_dir_all(project.join(".graphhelm/journeys")).unwrap();
        std::fs::create_dir_all(project.join(".graphhelm/observers")).unwrap();
        std::fs::create_dir_all(super::dir_of(project, id)).unwrap();
        let repo = std::path::PathBuf::from(
            std::env::var_os("GRAPHHELM_PREVIEW_OBSERVER_REPO")
                .expect("OBSERVER_MISSING: Studio checkout"),
        );
        let source =
            std::fs::read(repo.join(".graphhelm/journeys/studio2-open.journey.yaml")).unwrap();
        std::fs::write(
            project.join(".graphhelm/journeys/studio2-open.journey.yaml"),
            &source,
        )
        .unwrap();
        let flow: Value = serde_yaml_ng::from_slice(&source).unwrap();
        for screen in flow["screens"].as_array().unwrap() {
            for scope in screen["scope"].as_array().unwrap() {
                let scope = scope.as_str().unwrap();
                let target = project.join(scope);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::copy(repo.join(scope), target).unwrap();
            }
        }
        std::fs::write(
            project.join(".graphhelm/observers/journey_driver.mjs"),
            include_bytes!("../../../../tools/journey-driver/driver.mjs"),
        )
        .unwrap();
        std::fs::write(
            project.join(".graphhelm/observers/judge.mjs"),
            include_bytes!("../../../../tools/journey-driver/judge.mjs"),
        )
        .unwrap();
        std::fs::write(project.join("package.json"), "{}").unwrap();
        std::fs::write(
            project.join(".graphhelm/journey-fixture.json"),
            r#"{"schema":"graphhelm-journey-fixture/1","script":"observe.sh","isolated":true}"#,
        )
        .unwrap();
        std::fs::write(
            project.join("observe.sh"),
            r#"#!/usr/bin/env bash
set -eu
if [ "$1" = down ]; then
  cp .graphhelm/journey-previews/studio2-open/state.json before-down.json
fi
export GRAPHHELM_BIN="$GRAPHHELM_PREVIEW_OBSERVER_BIN"
exec bash "$GRAPHHELM_PREVIEW_OBSERVER_REPO/tools/studio-journey-fixture/fixture.sh" "$@"
"#,
        )
        .unwrap();
        let outcome = super::run(&JourneyPreviewArgs {
            id: id.into(),
            project: Some(project.into()),
            force: true,
            read: false,
            run: true,
            confirm: false,
            events: None,
            execution: None,
            keyring: None,
            key_id: None,
            validation_dir: None,
            confirm_digest: None,
            token_file: None,
            owner: false,
        });
        assert_eq!(outcome.exit_code, 0);
        let during: Value = serde_json::from_slice(
            &std::fs::read(project.join("before-down.json")).expect("launcher reached down"),
        )
        .unwrap();
        assert_eq!(during["screens"].as_object().unwrap().len(), 4, "{during}");
        assert!(
            during["screens"]
                .as_object()
                .unwrap()
                .values()
                .all(|s| s["result"] == "pass"),
            "{during}"
        );
        assert_eq!(during["state"], "ready", "{during}");
        assert_eq!(during["result"], "pass", "{during}");
        assert!(during["ranAt"].is_string(), "{during}");
        assert_eq!(
            reap_lost(project, &super::dir_of(project, id)).unwrap()["state"],
            "ready"
        );
    }

    /// #1309 card 1c: the key is the latest commit touching the flow file or a scope path, so an
    /// unrelated commit keeps it, and a scope path since deleted still names the commit that
    /// deleted it rather than reading as `unknown`. Cost: one temp git repository.
    #[test]
    fn the_scope_commit_ignores_unrelated_commits_and_survives_a_deleted_path() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            let out = graphhelm_process::command("git")
                .current_dir(dir.path())
                .args(["-c", "user.name=T", "-c", "user.email=t@example.test"])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        let head = || git(&["rev-parse", "HEAD"]).trim().to_owned();
        git(&["init", "-q", "--object-format=sha1"]);
        std::fs::create_dir_all(dir.path().join("app")).unwrap();
        std::fs::write(dir.path().join("app/page.tsx"), "a").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "scope"]);
        let flow = json!({"screens":[{"id":"cart","scope":["app/page.tsx"]}]});
        let scoped = head();
        assert_eq!(
            super::scope_commit_of(dir.path(), "checkout", &flow),
            scoped
        );
        std::fs::write(dir.path().join("other.md"), "b").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "unrelated"]);
        assert_eq!(
            super::scope_commit_of(dir.path(), "checkout", &flow),
            scoped
        );
        git(&["rm", "-q", "app/page.tsx"]);
        git(&["commit", "-q", "-m", "deleted"]);
        assert_eq!(
            super::scope_commit_of(dir.path(), "checkout", &flow),
            head()
        );
    }

    /// #1911 (BLOCK on #2002): a draft's commands run unasked only when the PARSED bytes equal the
    /// blob on the `main` the Runtime's own remote serves now. Defects named, each must answer
    /// "not reviewed": no remote configured, tampered bytes, a stale review (the remote's main
    /// moved on), an id reaching outside the journeys directory, an option-shaped or unreachable
    /// URL, and a project whose local `refs/remotes/origin/main` and `.git/config` remote were
    /// rewritten to vouch for the draft (neither is consulted). Cost: a few local git processes.
    #[test]
    fn reviewed_on_main_trusts_only_the_runtime_remotes_main_and_the_parsed_bytes() {
        let remote = tempfile::tempdir().unwrap();
        let git_in = |dir: &Path, args: &[&str]| {
            let out = graphhelm_process::command("git")
                .current_dir(dir)
                .args(["-c", "user.name=T", "-c", "user.email=t@example.test"])
                .args(["-c", "core.autocrlf=false"])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap()
        };
        let commit_flow = |dir: &Path, text: &str, message: &str| {
            let flow = dir.join(".graphhelm").join("journeys");
            std::fs::create_dir_all(&flow).unwrap();
            std::fs::write(flow.join("greet.journey.yaml"), text).unwrap();
            git_in(dir, &["add", "-A"]);
            git_in(dir, &["commit", "-q", "-m", message]);
        };
        let v1 = b"id: greet\nv: 1\n".as_slice();
        let v2 = b"id: greet\nv: 2\n".as_slice();
        git_in(
            remote.path(),
            &["init", "-q", "-b", "main", "--object-format=sha1"],
        );
        commit_flow(remote.path(), "id: greet\nv: 1\n", "v1");
        let url = remote.path().to_str().unwrap().to_owned();
        let url = Some(url.as_str());

        // Merged on the remote's main: these exact parsed bytes are reviewed.
        assert!(super::reviewed_on_main(url, "greet", v1));
        // No remote from the Runtime: nothing is known to be reviewed.
        assert!(!super::reviewed_on_main(None, "greet", v1));
        // Tampered or edited bytes are not the reviewed ones.
        assert!(!super::reviewed_on_main(url, "greet", v2));
        assert!(!super::reviewed_on_main(url, "greet", b""));
        // A path outside the journeys directory is never looked up.
        for bad in ["../greet", "a/b", "", ".greet"] {
            assert!(!super::reviewed_on_main(url, bad, v1), "{bad}");
        }
        // An option-shaped or unreachable URL fails closed.
        assert!(!super::reviewed_on_main(
            Some("--upload-pack=x"),
            "greet",
            v1
        ));
        let gone = remote.path().join("missing-repo");
        assert!(!super::reviewed_on_main(gone.to_str(), "greet", v1));

        // A lane's project with forged local anchors: its own commit holds v2, its
        // refs/remotes/origin/main points at that commit and its `origin` URL points at itself.
        // The Runtime's remote still serves v1, so v2 waits for review.
        let project = tempfile::tempdir().unwrap();
        git_in(
            project.path(),
            &["init", "-q", "-b", "main", "--object-format=sha1"],
        );
        commit_flow(project.path(), "id: greet\nv: 2\n", "forged");
        git_in(
            project.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        let own = project.path().to_str().unwrap().to_owned();
        git_in(project.path(), &["remote", "add", "origin", &own]);
        assert!(!super::reviewed_on_main(url, "greet", v2));

        // Stale review: the remote's main moved to v2, so v1 is no longer the reviewed flow.
        commit_flow(remote.path(), "id: greet\nv: 2\n", "v2");
        assert!(super::reviewed_on_main(url, "greet", v2));
        assert!(!super::reviewed_on_main(url, "greet", v1));
    }

    /// #1911 (BLOCK on #2002): the reviewed check sees the bytes the run PARSED, never a second
    /// read. Defect named: a malicious flow parsed, then the reviewed bytes swapped back into the
    /// file before the check, so the check passes while the malicious argv runs. Cost: a few
    /// local git processes.
    #[test]
    fn a_flow_swapped_after_parse_is_checked_as_parsed() {
        let reviewed =
            include_str!("../../tests/fixtures/journey_backend/backend.journey.yaml").to_owned();
        let remote = tempfile::tempdir().unwrap();
        let git_in = |dir: &Path, args: &[&str]| {
            let out = graphhelm_process::command("git")
                .current_dir(dir)
                .args(["-c", "user.name=T", "-c", "user.email=t@example.test"])
                .args(["-c", "core.autocrlf=false"])
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        let journeys = remote.path().join(".graphhelm").join("journeys");
        std::fs::create_dir_all(&journeys).unwrap();
        std::fs::write(journeys.join("backend.journey.yaml"), &reviewed).unwrap();
        git_in(
            remote.path(),
            &["init", "-q", "-b", "main", "--object-format=sha1"],
        );
        git_in(remote.path(), &["add", "-A"]);
        git_in(remote.path(), &["commit", "-q", "-m", "reviewed"]);
        let url = remote.path().to_str().unwrap().to_owned();

        let project = tempfile::tempdir().unwrap();
        let file = project
            .path()
            .join(".graphhelm")
            .join("journeys")
            .join("backend.journey.yaml");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        let malicious = reviewed.replace("sh greet.sh", "sh evil.sh");
        assert_ne!(malicious, reviewed, "the fixture must name the command");
        std::fs::write(&file, &malicious).unwrap();
        let (flow, _, _, source) = super::flow_key_and_source(project.path(), "backend").unwrap();
        assert_eq!(
            super::super::journey_backend::cli_argv(&flow),
            vec!["sh evil.sh".to_owned()]
        );
        // The swap: the reviewed bytes are back in the file before the check runs.
        std::fs::write(&file, &reviewed).unwrap();
        assert!(!super::reviewed_on_main(
            Some(&url),
            "backend",
            source.as_bytes()
        ));
        // Control: the reviewed bytes, parsed, are reviewed.
        let (_, _, _, source) = super::flow_key_and_source(project.path(), "backend").unwrap();
        assert!(super::reviewed_on_main(
            Some(&url),
            "backend",
            source.as_bytes()
        ));
    }

    /// #1633 follow-up (gh-claude-18): only a path's ENTRY screen is excused by a passing edge
    /// leaving it. A screen in the middle of a path that was never observed is not, even when the
    /// next edge passed. Cost: microseconds.
    #[test]
    fn only_a_paths_entry_screen_is_excused_by_the_edge_leaving_it() {
        let flow = json!({"edges":[{"id":"a.b","from":"a","to":"b"},{"id":"b.c","from":"b","to":"c"}],
            "paths":{"main":["a.b","b.c"]}});
        let edges = json!({"a.b":{"result":"pass"},"b.c":{"result":"pass"}});
        let middle = json!({"a":{"frame":true,"result":"pass"},"b":{"frame":false,"reason":"not_reached"},
            "c":{"frame":true,"result":"pass"}});
        assert_ne!(super::run_verdict(&flow, &middle, &edges).0, "pass");
        let entry = json!({"a":{"frame":false,"reason":"not_reached"},"b":{"frame":true,"result":"pass"},
            "c":{"frame":true,"result":"pass"}});
        assert_eq!(super::run_verdict(&flow, &entry, &edges).0, "pass");
    }

    /// #1633: a run passes only when every step passed. Seen on gh-team: six keel flows whose
    /// screens were all `not_reached` and one edge `skipped/cli_needs_approval` read `pass` and
    /// recorded the owner's approve proof. Cost: microseconds.
    #[test]
    fn a_run_that_did_not_reach_or_skipped_a_step_is_not_a_pass() {
        // keel-red-first-proof, as the runner stored it on gh-team.
        let flow = json!({"edges":[{"id":"prove-regression","from":"defective-parent","to":"proof-report"}]});
        let screens = json!({"defective-parent":{"frame":false,"reason":"not_reached"},
            "proof-report":{"frame":false,"reason":"not_reached"}});
        let edges = json!({"prove-regression":{"reason":"cli_needs_approval","result":"skipped"}});
        assert_ne!(super::run_verdict(&flow, &screens, &edges).0, "pass");
        let ab = json!({"edges":[{"id":"a.b","from":"a","to":"b"}],"paths":{"main":["a.b"]}});
        let passed = json!({"a":{"frame":true,"result":"pass"},"b":{"frame":true,"result":"pass"}});
        let skipped = json!({"a.b":{"result":"skipped","reason":"confirm_needed"}});
        assert_ne!(super::run_verdict(&ab, &passed, &skipped).0, "pass");
        assert_eq!(
            super::run_verdict(&ab, &passed, &json!({"a.b":{"result":"pass"}})).0,
            "pass"
        );
        // A backend flow's start screen has no result of its own; the edge leaving it passed.
        let started =
            json!({"a":{"frame":false,"reason":"not_reached"},"b":{"frame":false,"result":"pass"}});
        assert_eq!(
            super::run_verdict(&ab, &started, &json!({"a.b":{"result":"pass"}})).0,
            "pass"
        );
        // A destination never reached is not excused by the edge into it.
        let unreached_end =
            json!({"a":{"frame":true,"result":"pass"},"b":{"frame":false,"reason":"not_reached"}});
        assert_ne!(
            super::run_verdict(&ab, &unreached_end, &json!({"a.b":{"result":"pass"}})).0,
            "pass"
        );
        let drifted =
            json!({"a":{"frame":true,"result":"drift"},"b":{"frame":false,"reason":"not_reached"}});
        assert_eq!(super::run_verdict(&ab, &drifted, &json!({})).0, "drift");
    }

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

    /// #1298: a runner that dies before it launches any app (Run again on an isolated fixture:
    /// "stopped before step 1", no launcher lines, and the next run erased it) is stored as
    /// `failed / preview.runner_lost` with the tail of its own stderr, so Proof says why and the
    /// next run archives it into the history.
    #[test]
    fn a_dead_runner_without_an_app_is_stored_failed_with_its_log_and_archived() {
        let project = tempfile::tempdir().unwrap();
        let dir = dir_of(project.path(), "checkout");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(RUNNER_LOG),
            "early line\nthread 'main' panicked: boom\n",
        )
        .unwrap();
        save_state(
            &dir,
            &json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"running",
                "pid":u32::MAX,"startedAt":chrono::Utc::now().to_rfc3339(),
                "screens":{},"edges":{}}),
        );
        let reaped = reap_lost(project.path(), &dir).unwrap();
        assert_eq!(reaped["state"], "failed", "{reaped}");
        assert_eq!(reaped["reason"], "preview.runner_lost", "{reaped}");
        let tail = reaped["launchLog"]["tail"].as_str().unwrap_or_default();
        assert!(tail.contains("panicked: boom"), "{reaped}");
        assert_eq!(
            read_state(&dir).unwrap()["state"],
            "failed",
            "stored, not only shown"
        );
        archive(&dir, &reaped);
        assert_eq!(history_ids(&dir.join("history")).len(), 1);
    }

    /// #history: a finished run that a new one replaces moves to `history/` with its frames, its
    /// frame stays readable by run id, and only the newest `HISTORY_KEEP` runs are kept.
    #[test]
    fn a_replaced_run_is_kept_in_history_with_its_frames() {
        let project = tempfile::tempdir().unwrap();
        let dir = dir_of(project.path(), "checkout");
        std::fs::create_dir_all(dir.join("frames")).unwrap();
        std::fs::write(dir.join("frames/cart.png"), b"old cart").unwrap();
        let finished = |at: &str| {
            json!({"state":"ready","result":"pass","startedAt":at,
            "screens":{"cart":{"frame":true,"file":"cart.png","result":"pass"}},"edges":{}})
        };
        archive(&dir, &finished("2026-10-14T10:00:00Z"));
        assert!(
            !dir.join("frames").exists(),
            "the frames moved with the run"
        );
        let runs = history_ids(&dir.join("history"));
        assert_eq!(runs.len(), 1);
        assert_eq!(
            history_frame(project.path(), "checkout", &runs[0], "cart", None).unwrap(),
            b"old cart"
        );
        assert!(history_frame(project.path(), "checkout", "../../x", "cart", None).is_none());
        for minute in 0..25 {
            archive(&dir, &finished(&format!("2026-10-14T11:{minute:02}:00Z")));
        }
        let runs = history_ids(&dir.join("history"));
        assert_eq!(runs.len(), HISTORY_KEEP);
        assert_eq!(
            runs.last(),
            history_id(&finished("2026-10-14T11:24:00Z")).as_ref()
        );
    }

    /// #1956: a flow edited after its run reads `none` on the preview route, and that run is then
    /// the newest history entry with its frame readable by run id, so Proof shows the earlier
    /// version's result instead of "not run". Defects named: the run vanishes from every route,
    /// the current version's finished run is missing from history, or a running slot is offered.
    #[test]
    fn an_earlier_versions_run_is_the_newest_history_entry() {
        let project = tempfile::tempdir().unwrap();
        let journeys = project.path().join(".graphhelm/journeys");
        std::fs::create_dir_all(&journeys).unwrap();
        std::fs::write(
            journeys.join("studio-add-project.journey.yaml"),
            include_bytes!("testdata/studio-add-project.journey.yaml"),
        )
        .unwrap();
        for path in [
            "apps/studio/src/components/addproject.tsx",
            "apps/studio/src/components/rail.tsx",
        ] {
            let target = project.path().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, "fixture").unwrap();
        }
        let id = "studio-add-project";
        let dir = super::dir_of(project.path(), id);
        std::fs::create_dir_all(dir.join("frames")).unwrap();
        std::fs::write(dir.join("frames/run.png"), b"old run").unwrap();
        let (_, digest, commit) = super::flow_and_key(project.path(), id).unwrap();
        let kept = |digest: &str, state: &str| {
            json!({"preview":true,"state":state,"result":"pass","digest":digest,"commit":commit,
                "startedAt":"2026-10-22T21:53:00Z","ranAt":"2026-10-22T21:53:09Z",
                "screens":{"run":{"frame":true,"file":"run.png","result":"pass"}},"edges":{}})
        };
        let runs = |project: &std::path::Path| {
            super::history(project, id).output.data.unwrap()["runs"].clone()
        };
        save_state(&dir, &kept("sha256:old", "ready"));
        let run = history_id(&kept("sha256:old", "ready")).unwrap();
        let listed = runs(project.path());
        assert_eq!(listed[0]["run"], run.as_str(), "{listed}");
        assert_eq!(listed[0]["digest"], "sha256:old", "{listed}");
        assert_eq!(listed[0]["result"], "pass", "{listed}");
        assert_eq!(
            history_frame(project.path(), id, &run, "run", None).unwrap(),
            b"old run"
        );
        // #2011: the current version's finished run is the newest history entry too.
        save_state(&dir, &kept(&digest, "ready"));
        let listed = runs(project.path());
        assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
        assert_eq!(listed[0]["run"], run.as_str(), "{listed}");
        assert_eq!(listed[0]["digest"], digest.as_str(), "{listed}");
        assert_eq!(
            history_frame(project.path(), id, &run, "run", None).unwrap(),
            b"old run"
        );
        // A slot still running is no earlier result.
        save_state(&dir, &kept("sha256:old", "running"));
        assert_eq!(runs(project.path()), json!([]));
    }

    /// #560 review: a `running` state past the budget reads as a dead runner even when its pid is
    /// alive (here: this test's own process, as after pid reuse), so the next start runs anew; a
    /// fresh one with a live pid is still running. Cost: one process listing.
    #[test]
    fn a_running_preview_older_than_its_budget_has_no_runner() {
        let pid = std::process::id();
        let fresh =
            json!({"state":"running","pid":pid,"startedAt":chrono::Utc::now().to_rfc3339()});
        assert_eq!(runner_liveness(&fresh), super::Liveness::Alive);
        let old = json!({"state":"running","pid":pid,
            "startedAt":(chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339()});
        assert_eq!(runner_liveness(&old), super::Liveness::Dead);
        let flow = json!({"status":"draft"});
        let mut stored = old.clone();
        stored["digest"] = "sha256:aa".into();
        stored["commit"] = "c1".into();
        let answer = body(Some(stored), &flow, "sha256:aa", "c1");
        assert_eq!(answer["state"], "failed");
        // #560 review (gh-claude-3's real run): past its budget the run did not finish in time.
        assert_eq!(answer["reason"], "preview.budget_exceeded");
    }

    /// #1610: a status read reaps first (one probe: the runner is alive, so it hands back the
    /// `running` copy), then builds the answer. If the answer probed again, a runner that wrote
    /// `ready` and exited in between read dead, and the GET answered the stale copy as
    /// `failed / internal` with no step run: the Studio showed "It stopped before step 1" and
    /// stopped polling, while the stored run was `ready / pass` (seen on the fixture after
    /// Run again). The answer trusts the reaper's verdict. Cost: no subprocess.
    #[test]
    fn an_answer_never_reprobes_a_runner_the_reaper_found_alive() {
        let flow = json!({"status":"approved"});
        // No process has this pid: the runner finished after the reaper's probe.
        let stored = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"running",
            "pid":u32::MAX,"startedAt":chrono::Utc::now().to_rfc3339(),"screens":{},"edges":{}});
        let answer = body(Some(stored), &flow, "sha256:aa", "c1");
        assert_eq!(answer["state"], "running", "{answer}");
        assert!(answer.get("reason").is_none(), "{answer}");
    }

    /// #1610 follow-up (gh-claude-21's review of #1616): start reaped with one probe (the runner
    /// is alive, so it handed back the `running` copy), then probed again to decide whether a run
    /// is going. A runner that wrote `ready` and exited in between read dead, so a forced start
    /// wiped its frames, stored a new `running` over the finished result and spawned a second
    /// run. Start trusts the reaper: it answers the run as still going and touches nothing.
    /// Cost: temp files and an injected probe; no subprocess.
    #[test]
    fn a_forced_start_never_replaces_a_run_that_finishes_between_two_probes() {
        let project = tempfile::tempdir().unwrap();
        let journeys = project.path().join(".graphhelm/journeys");
        std::fs::create_dir_all(&journeys).unwrap();
        std::fs::write(
            journeys.join("studio-add-project.journey.yaml"),
            include_bytes!("testdata/studio-add-project.journey.yaml"),
        )
        .unwrap();
        for path in [
            "apps/studio/src/components/addproject.tsx",
            "apps/studio/src/components/rail.tsx",
        ] {
            let target = project.path().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, "fixture").unwrap();
        }
        let id = "studio-add-project";
        let dir = super::dir_of(project.path(), id);
        let frames = dir.join("frames");
        std::fs::create_dir_all(&frames).unwrap();
        std::fs::write(frames.join("entry.png"), b"frame").unwrap();
        let (_, digest, commit) = super::flow_and_key(project.path(), id).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let running = json!({"preview":true,"kind":"preview","digest":digest,"commit":commit,
            "state":"running","pid":123,"startedAt":now,"current":null,"screens":{},"edges":{}});
        let mut ready = running.clone();
        ready["state"] = "ready".into();
        ready["result"] = "pass".into();
        ready["ranAt"] = now.clone().into();
        save_state(&dir, &running);
        let probes = std::cell::Cell::new(0);

        let answer = super::start_with(
            &JourneyPreviewArgs {
                id: id.into(),
                project: Some(project.path().to_path_buf()),
                force: true,
                read: false,
                run: false,
                confirm: false,
                events: None,
                execution: None,
                keyring: None,
                key_id: None,
                validation_dir: None,
                confirm_digest: None,
                token_file: None,
                owner: false,
            },
            |_| {
                probes.set(probes.get() + 1);
                if probes.get() == 1 {
                    // The runner finishes while the reaper's probe still sees it.
                    save_state(&dir, &ready);
                    super::Liveness::Alive
                } else {
                    super::Liveness::Dead
                }
            },
        );

        let answer = answer.output.data.unwrap();
        assert_eq!(answer["state"], "running", "{answer}");
        assert_eq!(
            read_state(&dir).unwrap(),
            ready,
            "the finished run is not overwritten"
        );
        assert_eq!(std::fs::read(frames.join("entry.png")).unwrap(), b"frame");
    }

    /// #676: a failed process probe is not evidence that a running preview is dead, so its
    /// launched app must not receive `down` and its state must remain running. Cost: one temp
    /// fixture and one injected probe; no subprocess, browser or network.
    #[test]
    fn an_unavailable_liveness_probe_never_reaps_a_running_preview() {
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
        let app = project.path().join("fixture-app");
        std::fs::create_dir(&app).unwrap();
        let running = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"running",
            "pid":123,"startedAt":chrono::Utc::now().to_rfc3339(),"screens":{},"edges":{},
            "launched":{"script":"fake.sh","dir":app.to_string_lossy()}});
        save_state(previews.path(), &running);

        let result = reap_lost_from_with(
            project.path(),
            previews.path(),
            || read_state(previews.path()),
            |state| runner_liveness_with(state, |_| super::Liveness::Unknown),
        )
        .unwrap();

        assert_eq!(result["state"], "running", "{result}");
        assert!(result["launched"].is_object(), "{result}");
        assert!(!project.path().join("down.log").exists());
        assert!(app.exists());
        assert_eq!(read_state(previews.path()).unwrap()["state"], "running");
    }

    /// #676 review: start must not replace an in-budget preview when its runner probe is unknown.
    /// A malformed pid is an unavailable probe on every platform; the saved state and frames
    /// belong to the existing runner. Cost: one temp project and files; no subprocess or browser.
    #[test]
    fn start_does_not_replace_an_in_budget_preview_with_unknown_liveness() {
        let project = tempfile::tempdir().unwrap();
        let journeys = project.path().join(".graphhelm/journeys");
        std::fs::create_dir_all(&journeys).unwrap();
        std::fs::write(
            journeys.join("studio-add-project.journey.yaml"),
            include_bytes!("testdata/studio-add-project.journey.yaml"),
        )
        .unwrap();
        for path in [
            "apps/studio/src/components/addproject.tsx",
            "apps/studio/src/components/rail.tsx",
        ] {
            let target = project.path().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, "fixture").unwrap();
        }
        let id = "studio-add-project";
        let dir = super::dir_of(project.path(), id);
        let frames = dir.join("frames");
        std::fs::create_dir_all(&frames).unwrap();
        std::fs::write(frames.join("existing.png"), b"frame").unwrap();
        let (_, digest, commit) = super::flow_and_key(project.path(), id).unwrap();
        let running = json!({"preview":true,"kind":"preview","digest":digest,"commit":commit,
            "state":"running","pid":"unavailable",
            "startedAt":(chrono::Utc::now() - chrono::Duration::seconds(90)).to_rfc3339(),
            "current":null,"screens":{},"edges":{},"launched":{"script":"fixture.sh"}});
        save_state(&dir, &running);

        let answer = super::start(&JourneyPreviewArgs {
            id: id.into(),
            project: Some(project.path().to_path_buf()),
            force: true,
            read: false,
            run: false,
            confirm: false,
            events: None,
            execution: None,
            keyring: None,
            key_id: None,
            validation_dir: None,
            confirm_digest: None,
            token_file: None,
            owner: false,
        });

        let answer = answer.output.data.unwrap();
        assert_eq!(answer["state"], "running");
        assert_eq!(read_state(&dir).unwrap(), running);
        assert_eq!(
            std::fs::read(frames.join("existing.png")).unwrap(),
            b"frame"
        );
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
                cli_confirmed: false,
                cli_reviewed: false,
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
            cli_confirmed: false,
            cli_reviewed: false,
        };
        assert_eq!(draft(false).stops_before(&pay), Some("guard_refused"));
        assert_eq!(draft(true).stops_before(&pay), None);
    }

    /// #927 review: backend acts must not bypass the guard. Defects named: a DELETE or POST read
    /// as harmless because its path holds no deny-list word, and a draft running a command.
    #[test]
    fn backend_mutations_and_draft_commands_are_guarded() {
        let http = |method: &str| json!({"kind":"http","role":method,"name":"/items/3"});
        let cli = json!({"kind":"cli","role":"exec","name":"sh greet.sh"});
        let wipe = json!({"kind":"cli","role":"exec","name":"git push --force"});
        for launched in [false, true] {
            let draft = Guard {
                approved: false,
                launched,
                confirmed: false,
                cli_confirmed: false,
                cli_reviewed: false,
            };
            for method in ["DELETE", "POST", "put", "BREW"] {
                assert_eq!(
                    draft.stops_before(&http(method)),
                    Some("guard_refused"),
                    "{method}"
                );
            }
            for method in ["GET", "HEAD", "options"] {
                assert_eq!(draft.stops_before(&http(method)), None, "{method}");
            }
            assert_eq!(draft.stops_before(&cli), Some("review_needed"));
            // #1911: a draft reviewed on main runs its command; a destructive one stays refused.
            let reviewed = Guard {
                cli_reviewed: true,
                ..draft
            };
            assert_eq!(reviewed.stops_before(&cli), None);
            assert_eq!(reviewed.stops_before(&wipe), Some("guard_refused"));
            // #1662: the owner's confirm of this argv lets it play; a destructive one stays refused.
            let confirmed = Guard {
                cli_confirmed: true,
                ..draft
            };
            assert_eq!(confirmed.stops_before(&cli), None);
            assert_eq!(confirmed.stops_before(&wipe), Some("guard_refused"));
        }
        let approved = Guard {
            approved: true,
            launched: true,
            confirmed: false,
            cli_confirmed: false,
            cli_reviewed: false,
        };
        for method in ["DELETE", "POST"] {
            assert_eq!(approved.stops_before(&http(method)), Some("confirm_needed"));
        }
        assert_eq!(approved.stops_before(&http("GET")), None);
        assert_eq!(approved.stops_before(&cli), None);
        assert_eq!(approved.stops_before(&wipe), Some("confirm_needed"));
        let clicked = Guard {
            confirmed: true,
            ..approved
        };
        assert_eq!(clicked.stops_before(&http("DELETE")), None);
    }

    /// #1662 review: a command's output never reaches the run state or its evidence with a secret
    /// in it, and it is bounded; the digests still bind the raw bytes.
    #[test]
    fn a_commands_output_is_masked_bounded_and_digested() {
        let secret = "s3cr3t-token-value".to_owned();
        let seen = super::super::journey_backend::Observation {
            status: None,
            exit: Some(0),
            body: format!(
                "token={secret}
{}",
                "x".repeat(10_000)
            ),
            stderr: format!("warn: {secret}"),
        };
        let kept =
            super::masked_output(&json!("sh greet.sh"), &seen, std::slice::from_ref(&secret));
        let text = kept.to_string();
        assert!(!text.contains(&secret), "{text}");
        assert!(kept["stderr"].as_str().unwrap().contains("***"), "{kept}");
        assert!(
            kept["stdout"].as_str().unwrap().len() <= 4096,
            "unbounded stdout"
        );
        assert_eq!(kept["stdoutSha256"].as_str().unwrap().len(), 64);
        assert_eq!(kept["exit"], 0);
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
            validation_dir: None,
            confirm_digest: None,
            token_file: None,
            owner: false,
        };
        let approved = json!({"status":"approved"});
        let proof = proof_args(&full, &approved, project).expect("approved + execution records");
        assert_eq!(proof.id, "checkout");
        assert_eq!(proof.record.execution.as_deref(), Some("x"));
        assert!(proof_args(&full, &json!({"status":"draft"}), project).is_none());
        let partial = JourneyPreviewArgs {
            key_id: None,
            validation_dir: None,
            ..full
        };
        assert!(proof_args(&partial, &approved, project).is_none());
        // A Runtime with no keyring: an approved flow is refused, a draft plays as a preview.
        assert!(unsealed_proof(&partial, &approved));
        assert!(!unsealed_proof(&partial, &json!({"status":"draft"})));
        let no_execution = JourneyPreviewArgs {
            execution: None,
            validation_dir: None,
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
            let probe = |state: &Value| {
                if state["pid"] == std::process::id() {
                    super::Liveness::Alive
                } else {
                    super::Liveness::Dead
                }
            };
            let stored = super::read_settled_with(
                || {
                    count += 1;
                    reads.next()
                },
                &probe,
            );
            (
                super::body_with(stored, &flow, "sha256:aa", "c1", &probe),
                count,
            )
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

    /// #1672: the observer a passing preview reports is the one the STORED record names, read
    /// back. A record for the same flow, digest and commit is kept, not rewritten, so a later run
    /// by another binary must still report the first runner. Credible regression (#1689 review):
    /// reporting this binary's own sha and version beside an older record. Cost: one temporary
    /// project store; no browser.
    #[test]
    fn a_later_runner_reports_the_runner_the_stored_record_names() {
        let project = tempfile::tempdir().unwrap();
        // The owner record store a project gets from `graphhelm init`.
        let store = super::super::journey_owner::store(project.path());
        std::fs::create_dir_all(&store).unwrap();
        drop(super::super::event_store(&store).unwrap());
        let digest = format!("sha256:{}", "a".repeat(64));
        let commit = "c".repeat(40);
        // Runner A recorded this flow first, with an older version.
        let runner_a = "a1".repeat(32);
        super::super::journey_owner::record_previewed(
            project.path(),
            "checkout",
            &digest,
            &commit,
            &super::super::journey_owner::PreviewEvidence {
                evidence_sha256: &"e".repeat(64),
                runner_sha256: &runner_a,
                version: "0.0.9",
            },
        )
        .unwrap();
        // This binary (runner B) passes the same flow at the same digest and commit.
        let run = super::Run {
            dir: project.path().join("preview"),
            state: json!({"state":"ready","result":"pass","screens":{},"edges":{}}),
            frames: 0,
            proof: None,
            contract: String::new(),
            last: None,
            confirmed: false,
            viewport: Value::Null,
            declared: None,
            device: None,
            frames_sub: std::path::PathBuf::from("frames"),
            first_view: true,
            validation: false,
            flow_id: "checkout".to_owned(),
            reviewed: false,
        };
        let observer = super::record_pass(&run, project.path(), &digest, &commit).unwrap();
        assert_eq!(
            observer,
            json!({
                "actor": super::super::journey_owner::PREVIEW_RUNNER,
                "runnerSha256": runner_a,
                "version": "0.0.9",
            }),
            "the stored record's runner, not this binary's"
        );
    }

    /// #593: a runner killed inside up must leave a record that a reader can reap. Existing
    /// reaper cells supply a completed launch record and miss this window. Cost: a child test
    /// process, a shell and temp files; no browser/network, at most 15 seconds waiting for up.
    #[test]
    fn a_preview_records_its_fixture_before_up_returns() {
        use std::process::Stdio;
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};
        const CHILD: &str = "GRAPHHELM_TEST_PREVIEW_DURING_UP";
        if let Some(project) = std::env::var_os(CHILD) {
            let project = std::path::PathBuf::from(project);
            let mut run = super::Run {
                dir: project.join("preview"),
                state: json!({"state":"running","pid":std::process::id(),
                    "startedAt":chrono::Utc::now().to_rfc3339()}),
                frames: 0,
                proof: None,
                contract: String::new(),
                last: None,
                confirmed: false,
                viewport: Value::Null,
                declared: None,
                device: None,
                frames_sub: std::path::PathBuf::from("frames"),
                first_view: true,
                validation: false,
                flow_id: String::new(),
                reviewed: false,
            };
            run.save();
            let _ = super::play(
                &project,
                &json!({"base":"http://127.0.0.1:1","paths":{},"edges":[],"screens":[]}),
                &mut run,
                Instant::now() + Duration::from_secs(30),
                &Arc::new(Mutex::new(None)),
                &super::views_of(&json!({})),
            );
            return;
        }
        let project = tempfile::tempdir().unwrap();
        let observers = project.path().join(".graphhelm/observers");
        let previews = project.path().join("preview");
        std::fs::create_dir_all(&observers).unwrap();
        std::fs::create_dir(&previews).unwrap();
        std::fs::write(
            observers.join("journey_driver.mjs"),
            include_bytes!("../../../../tools/journey-driver/driver.mjs"),
        )
        .unwrap();
        std::fs::write(
            observers.join("judge.mjs"),
            include_bytes!("../../../../tools/journey-driver/judge.mjs"),
        )
        .unwrap();
        std::fs::write(
            project.path().join(".graphhelm/journey-fixture.json"),
            r#"{"schema":"graphhelm-journey-fixture/1","script":"fake.sh","isolated":true}"#,
        )
        .unwrap();
        std::fs::write(
            project.path().join("fake.sh"),
            r#"#!/usr/bin/env bash
if [ "$1" = up ]; then
  mkdir -p "$2"
  echo "$$ $(cat /proc/$$/winpid 2>/dev/null)" > "$2/up.pid"
  printf '%s' "$2" > up.marker
  sleep 30
else
  read -r shellpid winpid < "$2/up.pid"
  if [ -n "$winpid" ]; then
    taskkill //PID "$winpid" //T //F >/dev/null 2>&1 || true
  else
    kill -STOP "$shellpid" 2>/dev/null || true
    pkill -TERM -P "$shellpid" 2>/dev/null || true
    kill -KILL "$shellpid" 2>/dev/null || true
  fi
  echo down >> down.log
fi
"#,
        )
        .unwrap();
        let mut runner = graphhelm_process::command(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::journey_preview::tests::a_preview_records_its_fixture_before_up_returns",
            ])
            .env(CHILD, project.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let until = Instant::now() + Duration::from_secs(15);
        let fixture = loop {
            let marker = std::fs::read_to_string(project.path().join("up.marker"));
            if let Some(marker) = marker.ok().filter(|text| !text.is_empty()) {
                break Some(marker);
            }
            if Instant::now() >= until {
                break None;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let recorded = read_state(&previews);
        runner.kill().unwrap();
        runner.wait().unwrap();
        let reaped = reap_lost(project.path(), &previews);
        let downs = std::fs::read_to_string(project.path().join("down.log"))
            .map_or(0, |log| log.lines().count());
        let fixture = fixture.expect("the real up process reached its sleeping marker");
        let removed = !Path::new(&fixture).exists();
        // Keep the RED run clean too: the old code has no persisted handle for the reader.
        if !removed {
            super::LaunchedStop::from_record(
                project.path(),
                &json!({"script":"fake.sh","dir":fixture}),
            )
            .unwrap()
            .stop();
        }
        let recorded = recorded.unwrap();
        assert_eq!(recorded["launched"]["script"], "fake.sh", "{recorded}");
        assert_eq!(recorded["launched"]["dir"], fixture);
        let reaped = reaped.unwrap();
        assert_eq!(reaped["state"], "failed", "{reaped}");
        assert_eq!(reaped["reason"], "preview.runner_lost", "{reaped}");
        assert_eq!(downs, 1, "the reader ran down once");
        assert!(removed, "the reader removed the fixture directory");
        reap_lost(project.path(), &previews);
        assert_eq!(
            std::fs::read_to_string(project.path().join("down.log"))
                .unwrap()
                .lines()
                .count(),
            1
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

    /// #1448: the runner can write `ready` and exit between two liveness probes. A reaper that
    /// read `running`, probed the runner alive, then probed again and saw it dead, stored its
    /// stale `running` copy as `preview.runner_lost` over the finished run (seen on the fixture:
    /// `ready` at 17:51:18.76, `runner_lost` at 17:51:19.38, same pid). One probe per read.
    /// Cost: temp files and an injected probe; no subprocess.
    #[test]
    fn a_runner_that_finishes_between_two_probes_keeps_its_result() {
        let project = tempfile::tempdir().unwrap();
        let previews = tempfile::tempdir().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let running = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"running",
            "pid":123,"startedAt":now,"screens":{},"edges":{}});
        let ready = json!({"preview":true,"digest":"sha256:aa","commit":"c1","state":"ready",
            "result":"pass","pid":123,"startedAt":now,"ranAt":now,"screens":{},"edges":{}});
        save_state(previews.path(), &running);
        let probes = std::cell::Cell::new(0);
        let answered = reap_lost_from_with(
            project.path(),
            previews.path(),
            || read_state(previews.path()),
            |_| {
                probes.set(probes.get() + 1);
                if probes.get() == 1 {
                    // The runner finishes while the first probe still sees it.
                    save_state(previews.path(), &ready);
                    super::Liveness::Alive
                } else {
                    super::Liveness::Dead
                }
            },
        )
        .unwrap();
        assert_ne!(answered["reason"], "preview.runner_lost", "{answered}");
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
