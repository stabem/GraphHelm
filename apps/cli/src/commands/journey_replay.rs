//! Approved-flow replay. Browser I/O and blocking filesystem/Git/recording stages run
//! inside a contained worker. No route, gateway lease, model executor or healing path.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use fs2::FileExt;
use graphhelm_protocols::Diagnostic;
use graphhelm_schema::OfflineSchemaSet;
use serde_json::{Value, json};

use crate::args::JourneyReplayArgs;
use crate::output::{CommandOutput, Outcome};

const COMMAND: &str = "journey.replay";
const PROTOCOL: &str = "graphhelm-journey-driver/1";
const CACHE_SCHEMA: &str = "https://p50.dev/schemas/journey-replay-cache.schema.json";
const FRAME: usize = 64 * 1024;
/// The driver's ARIA snapshot caps (`tools/journey-driver/driver.mjs` `SNAPSHOT` /
/// `SNAPSHOT_DISCOVER`, #434): a replay snapshot may carry a real page, a discover snapshot feeds a
/// model and keeps the explore design's budget.
const SNAPSHOT: usize = 32 * 1024;
const SNAPSHOT_DISCOVER: usize = 6144;
const CACHE_LIMIT: u64 = 2 * 1024 * 1024;
pub(super) const OP_BUDGET: Duration = Duration::from_secs(30);
// How long a caller waits for OwnedChild::cleanup to report. Cleanup runs the process-tree
// terminate first and only then its own one-second reap window, so this wait must outlast
// both; an equal one-second wait raced the terminate and reported cleanup_uncertain.
const CLEANUP_OBSERVE: Duration = Duration::from_secs(5);
// The independent supervisor bounds waiting, including startup and the worker's
// blocking storage/Git/record calls. Kill/reap gets a separate one-second observer.
// OS scheduler failure and an inconclusive cleanup are reported uncertain, not success.
pub(super) const RUN_BUDGET: Duration = Duration::from_secs(180);
/// Driver failures a headed session survives (`tools/journey-driver/driver.mjs` `SURVIVABLE`).
pub(super) const SURVIVABLE: [&str; 5] = [
    "driver.locator_missing",
    "driver.locator_ambiguous",
    "driver.expectation_failed",
    "driver.action_failed",
    "driver.timeout",
];
pub(super) type Failure = (&'static str, String, i32);
pub(super) type Result<T> = std::result::Result<T, Failure>;

pub(super) fn failure(code: &'static str, path: impl Into<String>, exit: i32) -> Failure {
    (code, path.into(), exit)
}

fn report(data: Value, failed: Option<Failure>) -> Outcome {
    let Some((code, path, exit_code)) = failed else {
        return Outcome::success(COMMAND, data);
    };
    let message = match code {
        "replay.observer_missing" | "driver.observer_missing" => {
            "OBSERVER_MISSING: run setup --install-observer playwright in this project, then retry with a runnable Node/Playwright/Chromium observer"
        }
        "replay.timeout" => {
            "the replay budget ended; committed records and masked temporary inputs may remain; reconcile before retrying a mutation"
        }
        "replay.cleanup_uncertain" => {
            "owned process cleanup was not observed; partial effects remain uncertain"
        }
        "replay.recording_incomplete" => {
            "supply all of --events, --execution, --keyring and --key-id, or none"
        }
        "replay.worker_invalid" => {
            "the internal replay worker handshake or bounded response was invalid"
        }
        _ => "replay refused or could not observe this obligation; earlier observations remain",
    };
    Outcome {
        output: CommandOutput {
            ok: false,
            command: COMMAND,
            data: Some(data),
            diagnostics: vec![Diagnostic::error(code, message, path, "graphhelm")],
        },
        exit_code,
    }
}

pub(super) fn safe_environment(command: &mut Command, recording: bool) {
    command.env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "SYSTEMROOT",
        "WINDIR",
        "COMSPEC",
        "TEMP",
        "TMP",
        "HOME",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PLAYWRIGHT_BROWSERS_PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    if recording && let Some(value) = std::env::var_os("GRAPHHELM_EVENTS_KEY") {
        command.env("GRAPHHELM_EVENTS_KEY", value);
    }
    command.env("GIT_OPTIONAL_LOCKS", "0");
}

/// One owner closes each job/group. Cleanup never joins an inherited-pipe reader.
struct OwnedChild {
    child: Option<Child>,
    group: Option<graphhelm_process_tree::ProcessGroup>,
}

impl OwnedChild {
    fn cleanup(mut self) -> Receiver<std::result::Result<i32, ()>> {
        let child = self.child.take().unwrap();
        let group = self.group.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let mut child = child;
            let mut group = group;
            // The leader exiting does not prove its pipe-owning descendants exited.
            // Use the adapter's termination observer before closing the group in both cases.
            let terminated = graphhelm_process_tree::terminate(child.id(), group);
            graphhelm_process_tree::close(&mut group);
            let deadline = Instant::now() + Duration::from_secs(1);
            let status = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Ok(status.code().unwrap_or(1)),
                    _ if Instant::now() >= deadline => break Err(()),
                    _ => std::thread::sleep(Duration::from_millis(5)),
                }
            };
            let complete = matches!(
                terminated,
                graphhelm_process_tree::TerminationOutcome::Complete
            );
            let _ = tx.send(if complete { status } else { Err(()) });
        });
        rx
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let (Some(mut child), Some(mut group)) = (self.child.take(), self.group.take()) {
            std::thread::spawn(move || {
                let _ = graphhelm_process_tree::terminate(child.id(), group);
                graphhelm_process_tree::close(&mut group);
                let _ = child.wait();
            });
        }
    }
}

fn spawn_owned(mut command: Command, deadline: Instant) -> Result<OwnedChild> {
    let (tx, rx) = mpsc::sync_channel(1);
    // A delayed spawn owns its eventual child too. Neither worker nor driver performs
    // effects before the caller sends its start/request frame after containment.
    std::thread::spawn(move || {
        graphhelm_process_tree::configure(&mut command);
        let answer = command
            .spawn()
            .map_err(|_| failure("replay.observer_missing", "/observer", 3))
            .and_then(|mut child| {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(failure("replay.timeout", "/startup", 1));
                }
                match graphhelm_process_tree::create(&child) {
                    Ok(group) => Ok(OwnedChild {
                        child: Some(child),
                        group: Some(group),
                    }),
                    Err(_) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        Err(failure("replay.cleanup_uncertain", "/startup", 1))
                    }
                }
            });
        let _ = tx.send(answer);
    });
    rx.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| failure("replay.timeout", "/startup", 1))?
}

/// Reads a child's stderr to the end and discards it. A receiver nobody reads lets the
/// pipe either fill (the child blocks) or close early (the child's next write fails), so
/// diagnostics output must be drained for the whole life of the child, never echoed.
fn drain(pipe: impl Read + Send + 'static) {
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut BufReader::new(pipe), &mut std::io::sink());
    });
}

fn frames(pipe: impl Read + Send + 'static, limit: usize) -> Receiver<Result<Option<Vec<u8>>>> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(pipe);
        loop {
            let mut bytes = Vec::new();
            let read = Read::by_ref(&mut reader)
                .take(limit as u64 + 2)
                .read_until(b'\n', &mut bytes);
            if matches!(read, Ok(0)) {
                let _ = tx.send(Ok(None));
                return;
            }
            if read.is_err() || bytes.len() > limit + 1 || bytes.last() != Some(&b'\n') {
                let _ = tx.send(Err(failure("replay.driver_frame_invalid", "/observer", 1)));
                return;
            }
            bytes.pop();
            if tx.send(Ok(Some(bytes))).is_err() {
                return;
            }
        }
    });
    rx
}

fn write_frame(
    mut pipe: impl Write + Send + 'static,
    bytes: Vec<u8>,
    deadline: Instant,
) -> Result<()> {
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = pipe.write_all(&bytes).and_then(|()| pipe.flush());
        let _ = tx.send(result.is_ok());
    });
    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
        Ok(true) => Ok(()),
        Ok(false) => Err(failure("replay.driver_frame_invalid", "/observer", 1)),
        Err(_) => Err(failure("replay.timeout", "/observer/write", 1)),
    }
}

pub(super) fn child_output(
    command: Command,
    input: Vec<u8>,
    budget: Duration,
    limit: usize,
) -> Result<(i32, Vec<u8>)> {
    let deadline = Instant::now() + budget;
    let mut owned = spawn_owned(command, deadline)?;
    let child = owned.child.as_mut().unwrap();
    let replies = frames(child.stdout.take().unwrap(), limit);
    drain(child.stderr.take().unwrap());
    supervise_owned(owned, input, deadline, replies)
}

fn supervise_owned(
    mut owned: OwnedChild,
    input: Vec<u8>,
    deadline: Instant,
    rx: Receiver<Result<Option<Vec<u8>>>>,
) -> Result<(i32, Vec<u8>)> {
    let result = (|| {
        let child = owned.child.as_mut().unwrap();
        write_frame(child.stdin.take().unwrap(), input, deadline)?;
        let bytes = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| failure("replay.timeout", "/worker", 1))??
            .ok_or_else(|| failure("replay.worker_invalid", "/worker", 1))?;
        if bytes.is_empty() {
            return Err(failure("replay.worker_invalid", "/worker", 1));
        }
        let trailing = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| failure("replay.timeout", "/worker", 1))??;
        if trailing.is_some() {
            return Err(failure("replay.worker_invalid", "/worker", 1));
        }
        while !graphhelm_process_tree::leader_exited(owned.child.as_mut().unwrap())
            .map_err(|_| failure("replay.cleanup_uncertain", "/worker", 1))?
        {
            if Instant::now() >= deadline {
                return Err(failure("replay.timeout", "/worker", 1));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(bytes)
    })();
    // Even a write/read/protocol failure waits for the independent kill/reap
    // observer. No error path silently drops a live pipe-owning group.
    let status = owned
        .cleanup()
        .recv_timeout(CLEANUP_OBSERVE)
        .map_err(|_| failure("replay.cleanup_uncertain", "/worker", 1))?
        .map_err(|()| failure("replay.cleanup_uncertain", "/worker", 1))?;
    result.map(|bytes| (status, bytes))
}

pub(super) struct Driver {
    owned: Option<OwnedChild>,
    writer: mpsc::SyncSender<(Vec<u8>, mpsc::SyncSender<bool>)>,
    replies: Receiver<Result<Option<Vec<u8>>>>,
    sequence: u64,
    secrets: Vec<String>,
    headed: bool,
}

pub(super) struct TemporaryOutput(PathBuf);

impl TemporaryOutput {
    pub(super) fn create() -> Result<Self> {
        let root = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| failure("replay.output_unavailable", "/observer/output", 3))?;
        let path = root.join(format!("graphhelm-replay-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path)
            .map_err(|_| failure("replay.output_unavailable", "/observer/output", 3))?;
        Ok(Self(path))
    }
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        // Only this invocation's created absolute directory; a replaced symlink/
        // junction is never followed into somebody else's output.
        if safe_node(&self.0) && self.0.canonicalize().ok().as_ref() == Some(&self.0) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

impl Driver {
    pub(super) fn start(
        project: &Path,
        output: &Path,
        secrets: &BTreeMap<String, String>,
    ) -> Result<Self> {
        // Node's main-module resolver rejects Windows canonical drive prefixes.
        // Change only the external spelling, and verify it still identifies the
        // same directory before sending it to an observer.
        let plain = |path: &Path| -> Result<PathBuf> {
            let candidate = path
                .to_str()
                .and_then(|p| p.strip_prefix(r"\\?\"))
                .filter(|p| p.as_bytes().get(1) == Some(&b':'))
                .map_or_else(|| path.to_path_buf(), PathBuf::from);
            if candidate.canonicalize().ok() != path.canonicalize().ok() {
                return Err(failure("replay.observer_missing", "/observer/path", 3));
            }
            Ok(candidate)
        };
        let project = plain(project)?;
        let output = plain(output)?;
        let mut command = Command::new("node");
        command
            .arg(project.join(".graphhelm/observers/journey_driver.mjs"))
            .args(["--project"])
            .arg(&project)
            .arg("--output-dir")
            .arg(&output)
            .current_dir(&project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        safe_environment(&mut command, false);
        for (name, value) in secrets {
            command.env(name, value);
        }
        let mut owned = spawn_owned(command, Instant::now() + OP_BUDGET)?;
        let child = owned.child.as_mut().unwrap();
        let replies = frames(child.stdout.take().unwrap(), FRAME);
        drain(child.stderr.take().unwrap());
        let mut stdin = child.stdin.take().unwrap();
        let (tx, rx) = mpsc::sync_channel::<(Vec<u8>, mpsc::SyncSender<bool>)>(1);
        std::thread::spawn(move || {
            while let Ok((bytes, ack)) = rx.recv() {
                let ok = stdin.write_all(&bytes).and_then(|()| stdin.flush()).is_ok();
                let _ = ack.send(ok);
                if !ok {
                    break;
                }
            }
        });
        Ok(Self {
            owned: Some(owned),
            writer: tx,
            replies,
            sequence: 0,
            secrets: secrets.values().cloned().collect(),
            headed: false,
        })
    }

    pub(super) fn call(&mut self, op: &str, request: Value, path: &str) -> Result<Value> {
        if op == "open" && request["headed"] == true {
            self.headed = true;
        }
        let result = self.call_inner(op, request, path);
        // A headed (live, #398) session survives an observation failure; the driver keeps
        // its browser open too (`SURVIVABLE` in driver.mjs).
        let survived =
            self.headed && matches!(&result, Err((code, _, _)) if SURVIVABLE.contains(code));
        if result.is_err()
            && !survived
            && let Some(owned) = self.owned.take()
        {
            owned
                .cleanup()
                .recv_timeout(CLEANUP_OBSERVE)
                .map_err(|_| failure("replay.cleanup_uncertain", path, 1))?
                .map_err(|()| failure("replay.cleanup_uncertain", path, 1))?;
        }
        result
    }

    fn call_inner(&mut self, op: &str, mut request: Value, path: &str) -> Result<Value> {
        let deadline = Instant::now() + OP_BUDGET;
        self.sequence += 1;
        request["protocol"] = PROTOCOL.into();
        request["requestId"] = self.sequence.into();
        request["op"] = op.into();
        let mut bytes = serde_json::to_vec(&request)
            .map_err(|_| failure("replay.driver_frame_invalid", path, 1))?;
        if bytes.len() > FRAME {
            return Err(failure("replay.driver_frame_invalid", path, 1));
        }
        bytes.push(b'\n');
        let (tx, rx) = mpsc::sync_channel(1);
        self.writer
            .try_send((bytes, tx))
            .map_err(|_| failure("replay.driver_frame_invalid", path, 1))?;
        if !rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| failure("replay.timeout", format!("{path}/write"), 1))?
        {
            return Err(failure("replay.driver_frame_invalid", path, 1));
        }
        let bytes = self
            .replies
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| failure("replay.timeout", format!("{path}/read"), 1))??
            .ok_or_else(|| failure("replay.driver_frame_invalid", format!("{path}/eof"), 1))?;
        if self.secrets.iter().any(|secret| {
            !secret.is_empty() && {
                let encoded = serde_json::to_vec(secret).unwrap();
                let escaped = &encoded[1..encoded.len() - 1];
                bytes.windows(escaped.len()).any(|slice| slice == escaped)
            }
        }) {
            return Err(failure("driver.redaction_failed", path, 1));
        }
        // Each malformed-frame check names itself under the call's pointer, so a refused
        // frame says which check refused it without echoing driver-controlled text.
        let reply: Value = serde_json::from_slice(&bytes)
            .map_err(|_| failure("replay.driver_frame_invalid", format!("{path}/json"), 1))?;
        let success = reply["ok"]
            .as_bool()
            .ok_or_else(|| failure("replay.driver_frame_invalid", format!("{path}/ok"), 1))?;
        let keys = if success {
            &["protocol", "requestId", "ok", "result"][..]
        } else {
            &["protocol", "requestId", "ok", "code", "path"][..]
        };
        if reply["protocol"] != PROTOCOL
            || reply["requestId"] != self.sequence
            || !reply.as_object().is_some_and(|v| {
                v.len() == keys.len() && v.keys().all(|k| keys.contains(&k.as_str()))
            })
        {
            return Err(failure(
                "replay.driver_frame_invalid",
                format!("{path}/envelope"),
                1,
            ));
        }
        if !success {
            let code = match reply["code"].as_str().unwrap_or("") {
                "driver.locator_missing" => "driver.locator_missing",
                "driver.locator_ambiguous" => "driver.locator_ambiguous",
                "driver.expectation_failed" => "driver.expectation_failed",
                "driver.host_refused" => "driver.host_refused",
                "driver.timeout" => "driver.timeout",
                "driver.observer_missing" => "driver.observer_missing",
                "driver.secret_missing" => "driver.secret_missing",
                "driver.secret_literal" => "driver.secret_literal",
                "driver.unsupported_act" => "driver.unsupported_act",
                "driver.snapshot_too_large" => "driver.snapshot_too_large",
                "driver.redaction_failed" => "driver.redaction_failed",
                "driver.capture_refused" => "driver.capture_refused",
                "driver.action_failed" => "driver.action_failed",
                _ => {
                    return Err(failure(
                        "replay.driver_frame_invalid",
                        format!("{path}/code"),
                        1,
                    ));
                }
            };
            return Err(failure(
                code,
                path,
                if code == "driver.observer_missing" || code == "driver.secret_missing" {
                    3
                } else {
                    1
                },
            ));
        }
        let result = &reply["result"];
        let fields: &[&str] = match op {
            "open" => &["url"],
            "snapshot" if request["discover"] == true => {
                &["url", "ariaYaml", "controls", "fingerprint", "expectations"]
            }
            "snapshot" => &["url", "ariaYaml", "controls", "fingerprint"],
            "act" => &["url", "locator"],
            "capture" => &["path", "width", "height", "masked"],
            "close" => &["closed"],
            _ => return Err(failure("replay.driver_frame_invalid", path, 1)),
        };
        let closed = |value: &Value, keys: &[&str]| {
            value.as_object().is_some_and(|object| {
                object.len() == keys.len() && object.keys().all(|key| keys.contains(&key.as_str()))
            })
        };
        let text = |value: &Value, max: usize| value.as_str().is_some_and(|text| text.len() <= max);
        let valid = closed(result, fields)
            && match op {
                "open" => text(&result["url"], 4096),
                "snapshot" => {
                    (request["discover"] != true
                        || result["expectations"].as_array().is_some_and(|pairs| {
                            pairs.len() <= 8
                                && pairs.iter().all(|pair| {
                                    closed(pair, &["role", "name"])
                                        && text(&pair["role"], 64)
                                        && text(&pair["name"], 256)
                                })
                        }))
                        && text(&result["url"], 4096)
                        && text(
                            &result["ariaYaml"],
                            if request["discover"] == true {
                                SNAPSHOT_DISCOVER
                            } else {
                                SNAPSHOT
                            },
                        )
                        && result["fingerprint"].as_str().is_some_and(|value| {
                            value.strip_prefix("sha256:").is_some_and(|hash| {
                                hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())
                            })
                        })
                        && result["controls"].as_array().is_some_and(|controls| {
                            controls.len() <= 128
                                && controls.iter().all(|control| {
                                    closed(control, &["role", "name"])
                                        && text(&control["role"], 64)
                                        && text(&control["name"], 256)
                                })
                        })
                }
                "act" => {
                    text(&result["url"], 4096)
                        && closed(
                            &result["locator"],
                            &["role", "name", "exact", "testId", "context", "nth"],
                        )
                        && result["locator"]["role"] == request["role"]
                        && result["locator"]["name"] == request["name"]
                        && result["locator"]["exact"] == true
                        && result["locator"]["nth"].is_null()
                        && (result["locator"]["testId"].is_null()
                            || text(&result["locator"]["testId"], 256))
                        && (result["locator"]["context"].is_null()
                            || text(&result["locator"]["context"], 512))
                }
                "capture" => {
                    result["path"] == request["path"]
                        && result["masked"] == true
                        && result["width"]
                            .as_u64()
                            .is_some_and(|n| (1..=16384).contains(&n))
                        && result["height"]
                            .as_u64()
                            .is_some_and(|n| (1..=16384).contains(&n))
                }
                "close" => result["closed"] == true,
                _ => false,
            };
        if !valid {
            return Err(failure(
                "replay.driver_frame_invalid",
                format!("{path}/result"),
                1,
            ));
        }
        Ok(result.clone())
    }

    pub(super) fn close(mut self) -> Result<()> {
        self.call("close", json!({}), "/observer/close")?;
        // Each step after the close reply names itself, so a failed close says which one.
        let trailing = self
            .replies
            .recv_timeout(OP_BUDGET)
            .map_err(|_| failure("replay.timeout", "/observer/close/eof", 1))??;
        if trailing.is_some() {
            return Err(failure(
                "replay.driver_frame_invalid",
                "/observer/close/trailing",
                1,
            ));
        }
        let owned = self.owned.take().unwrap();
        owned
            .cleanup()
            .recv_timeout(CLEANUP_OBSERVE)
            .map_err(|_| failure("replay.cleanup_uncertain", "/observer/close/cleanup", 1))?
            .map_err(|()| failure("replay.cleanup_uncertain", "/observer/close/cleanup", 1))?;
        Ok(())
    }
}

pub(super) fn safe_node(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| {
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return false;
            }
        }
        !metadata.file_type().is_symlink()
    })
}

pub(super) fn safe_directory(path: &Path, create: bool) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && safe_node(path) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
            std::fs::create_dir(path).map_err(|_| failure("replay.cache_invalid", "/cache", 2))
        }
        _ => Err(failure("replay.cache_invalid", "/cache", 2)),
    }
}

pub(super) fn cache_valid(cache: &Value, flow: &Value) -> bool {
    let schema = serde_json::from_str(include_str!(
        "../../../../schemas/journey-replay-cache.schema.json"
    ))
    .unwrap();
    let schemas =
        OfflineSchemaSet::compile(BTreeMap::from([(CACHE_SCHEMA.to_owned(), schema)])).unwrap();
    if !schemas.validate(CACHE_SCHEMA, cache, "/cache").is_empty()
        || cache["id"] != flow["id"]
        || cache["viewport"]["width"].as_f64().unwrap()
            * cache["viewport"]["height"].as_f64().unwrap()
            > 16_777_216.0
    {
        return false;
    }
    // A valid old document has custody even when approval changed. It may not
    // drive a locator; semantic binding is checked only for the current digest.
    if cache["flowDigest"] != super::journey_flow::approval_digest(flow) {
        return true;
    }
    let selected: BTreeSet<_> = flow["paths"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|path| path.as_array().unwrap())
        .map(|id| id.as_str().unwrap())
        .collect();
    let edges: Vec<_> = flow["edges"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|edge| selected.contains(edge["id"].as_str().unwrap()))
        .collect();
    let screens: BTreeSet<_> = edges
        .iter()
        .flat_map(|edge| [edge["from"].as_str().unwrap(), edge["to"].as_str().unwrap()])
        .collect();
    if cache["screens"].as_object().unwrap().len() != screens.len()
        || cache["edges"].as_object().unwrap().len() != edges.len()
    {
        return false;
    }
    if screens
        .iter()
        .any(|screen| cache["screens"].get(*screen).is_none())
    {
        return false;
    }
    if edges.iter().any(|edge| {
        let acts = edge["acts"].as_array().unwrap();
        let Some(locators) = cache["edges"]
            .get(edge["id"].as_str().unwrap())
            .and_then(Value::as_array)
        else {
            return true;
        };
        locators.len() != acts.len()
            || locators.iter().zip(acts).any(|(locator, act)| {
                locator["role"] != act["role"] || locator["name"] != act["name"]
            })
    }) {
        return false;
    }
    true
}

pub(super) fn load_cache(path: &Path, flow: &Value) -> Result<Option<Value>> {
    let metadata = match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(metadata) => metadata,
        _ => return Err(failure("replay.cache_invalid", "/cache", 2)),
    };
    if !metadata.is_file() || !safe_node(path) || metadata.len() > CACHE_LIMIT {
        return Err(failure("replay.cache_invalid", "/cache", 2));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(CACHE_LIMIT + 1).read_to_end(&mut bytes))
        .map_err(|_| failure("replay.cache_invalid", "/cache", 2))?;
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| failure("replay.cache_invalid", "/cache", 2))?;
    if bytes.len() as u64 > CACHE_LIMIT || !cache_valid(&value, flow) {
        return Err(failure("replay.cache_invalid", "/cache", 2));
    }
    Ok(Some(value))
}

pub(super) fn preflight(flow: &Value) -> Result<BTreeMap<String, String>> {
    let supported = [
        "activate",
        "submit",
        "enter_text",
        "navigate",
        "wait_for",
        "inspect",
    ];
    let mut secrets = BTreeMap::new();
    let selected: BTreeSet<_> = flow["paths"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|path| path.as_array().unwrap())
        .map(|id| id.as_str().unwrap())
        .collect();
    for (index, edge) in flow["edges"].as_array().unwrap().iter().enumerate() {
        if !selected.contains(edge["id"].as_str().unwrap()) {
            continue;
        }
        for (act_index, act) in edge["acts"].as_array().unwrap().iter().enumerate() {
            let path = format!("/edges/{index}/acts/{act_index}");
            if !supported.contains(&act["kind"].as_str().unwrap()) {
                return Err(failure("driver.unsupported_act", path, 3));
            }
            if act["kind"] == "enter_text"
                && act.get("text").is_none()
                && act.get("secret").is_none()
            {
                return Err(failure("replay.act_value_missing", path, 3));
            }
            if let Some(secret) = act["secret"].as_str() {
                let name = format!("GRAPHHELM_SECRET_{secret}");
                let value = std::env::var(&name)
                    .ok()
                    .filter(|v| !v.is_empty() && v.len() <= 4096)
                    .ok_or_else(|| failure("driver.secret_missing", path.clone(), 3))?;
                secrets.insert(name, value);
            }
        }
    }
    let bytes = serde_json::to_vec(flow).unwrap();
    if secrets.values().any(|v| {
        let encoded = serde_json::to_vec(v).unwrap();
        let escaped = &encoded[1..encoded.len() - 1];
        bytes.windows(escaped.len()).any(|b| b == escaped)
    }) {
        return Err(failure("driver.secret_literal", "/secrets", 3));
    }
    let edges: BTreeMap<_, _> = flow["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| (edge["id"].as_str().unwrap(), edge))
        .collect();
    let screens: BTreeMap<_, _> = flow["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|screen| (screen["id"].as_str().unwrap(), screen))
        .collect();
    for (name, path) in flow["paths"].as_object().unwrap() {
        let first = &edges[path[0].as_str().unwrap()];
        let entry = screens[first["from"].as_str().unwrap()]["url"]
            .as_str()
            .unwrap();
        if entry
            .split(['/', '?', '&', '='])
            .any(|part| part.starts_with(':'))
        {
            return Err(failure("replay.entry_missing", format!("/paths/{name}"), 3));
        }
    }
    Ok(secrets)
}

pub(super) fn url_matches(base: &str, pattern: &str, observed: &str) -> bool {
    let Ok(expected_url) =
        format!("{}{}", base.trim_end_matches('/'), pattern).parse::<axum::http::Uri>()
    else {
        return false;
    };
    let Ok(actual_url) = observed
        .split('#')
        .next()
        .unwrap()
        .parse::<axum::http::Uri>()
    else {
        return false;
    };
    let port = |uri: &axum::http::Uri| {
        uri.port_u16()
            .unwrap_or(if uri.scheme_str() == Some("https") {
                443
            } else {
                80
            })
    };
    if expected_url.scheme_str() != actual_url.scheme_str()
        || !expected_url
            .host()
            .zip(actual_url.host())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        || port(&expected_url) != port(&actual_url)
    {
        return false;
    }
    let pattern = expected_url
        .path_and_query()
        .map_or("/", |value| value.as_str());
    let actual = actual_url
        .path_and_query()
        .map_or("/", |value| value.as_str());
    let parts = |s: &str| {
        s.split(['/', '?', '&', '='])
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    let expected = parts(pattern);
    let actual = parts(actual);
    expected.len() == actual.len()
        && expected.iter().zip(actual).all(|(want, have)| {
            if want.starts_with(':') {
                !have.is_empty()
            } else {
                *want == have
            }
        })
}

/// The installed observer must be byte-identical to this binary's bundled driver.
pub(super) fn observer_ready(project: &Path) -> Result<()> {
    let installed = project.join(".graphhelm/observers/journey_driver.mjs");
    let expected = include_bytes!("../../../../tools/journey-driver/driver.mjs");
    let mut installed_bytes = Vec::new();
    if !safe_node(&installed)
        || std::fs::File::open(&installed)
            .and_then(|file| {
                file.take(expected.len() as u64 + 1)
                    .read_to_end(&mut installed_bytes)
            })
            .is_err()
        || installed_bytes != expected
    {
        return Err(failure("replay.observer_missing", "/observer", 3));
    }
    if !safe_node(&project.join(".graphhelm/observers")) {
        return Err(failure("replay.observer_missing", "/observer", 3));
    }
    Ok(())
}

pub(super) fn observe(
    driver: &mut Driver,
    screen: &Value,
    base: &str,
    path: &str,
) -> Result<Value> {
    let snapshot = driver.call("snapshot", json!({"expect":screen["expect"]}), path)?;
    let url = snapshot["url"]
        .as_str()
        .ok_or_else(|| failure("replay.driver_frame_invalid", path, 1))?;
    if !url_matches(base, screen["url"].as_str().unwrap(), url) {
        return Err(failure("replay.wrong_screen", path, 1));
    }
    let aria = snapshot["ariaYaml"]
        .as_str()
        .ok_or_else(|| failure("replay.driver_frame_invalid", path, 1))?;
    if aria.len() > SNAPSHOT
        || !snapshot["controls"].is_array()
        || !snapshot["fingerprint"].is_string()
    {
        return Err(failure("replay.driver_frame_invalid", path, 1));
    }
    Ok(json!({"fingerprint":snapshot["fingerprint"],"controls":snapshot["controls"]}))
}

fn record_command(args: &JourneyReplayArgs, kind: &str, contract: &str) -> Result<Command> {
    let mut command = Command::new(
        std::env::current_exe().map_err(|_| failure("replay.record_failed", "/recording", 1))?,
    );
    command
        .args(["--json", "journey", kind, "--contract", contract])
        .arg("--events")
        .arg(args.events.as_ref().unwrap())
        .args(["--execution", args.execution.as_ref().unwrap()])
        .arg("--keyring")
        .arg(args.keyring.as_ref().unwrap())
        .args(["--key-id", args.key_id.as_ref().unwrap()])
        .arg("--project")
        .arg(args.project.as_deref().unwrap_or(Path::new(".")));
    safe_environment(&mut command, true);
    command.stdin(Stdio::null()).stderr(Stdio::null());
    Ok(command)
}

pub(super) fn record(
    args: &JourneyReplayArgs,
    contract: &str,
    step: &str,
    image: &Path,
    phase: Option<&str>,
) -> Result<String> {
    let mut capture = record_command(args, "capture", contract)?;
    capture.args(["--step", step, "--image"]).arg(image);
    if let Some(phase) = phase {
        capture.args(["--phase", phase]);
    }
    // Blocking store/record calls are inside the independently supervised worker.
    let output = capture
        .output()
        .map_err(|_| failure("replay.record_failed", "/recording/capture", 1))?;
    let reply: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| failure("replay.record_uncertain", "/recording/capture", 1))?;
    if !output.status.success() || reply["data"]["outcome"] != "recorded" {
        return Err(failure("replay.record_failed", "/recording/capture", 1));
    }
    let signal = reply["data"]["signalId"]
        .as_str()
        .ok_or_else(|| failure("replay.record_uncertain", "/recording/capture", 1))?
        .to_owned();
    Ok(signal)
}

pub(super) fn walked(
    args: &JourneyReplayArgs,
    contract: &str,
    from: &str,
    step: &str,
    from_capture: &str,
    signal: &str,
) -> Result<String> {
    let mut walked = record_command(args, "walked", contract)?;
    walked.args([
        "--from",
        from,
        "--to",
        step,
        "--from-capture",
        from_capture,
        "--to-capture",
        signal,
    ]);
    let output = walked
        .output()
        .map_err(|_| failure("replay.record_failed", "/recording/walked", 1))?;
    let reply: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| failure("replay.record_uncertain", "/recording/walked", 1))?;
    if !output.status.success() || reply["data"]["outcome"] != "recorded" {
        return Err(failure("replay.record_failed", "/recording/walked", 1));
    }
    reply["data"]["signalId"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| failure("replay.record_uncertain", "/recording/walked", 1))
}

fn replay(args: &JourneyReplayArgs, data: &mut Value) -> Result<()> {
    let project = args.project.clone().unwrap_or_else(|| PathBuf::from("."));
    let project = project
        .canonicalize()
        .map_err(|_| failure("replay.project_invalid", "/project", 3))?;
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
    let digest = super::journey_flow::approval_digest(&flow);
    let directory = project.join(".graphhelm/journey-cache");
    if std::fs::symlink_metadata(&directory).is_ok() {
        safe_directory(&directory, false)?;
    }
    let target = directory.join(format!("{}.json", args.id));
    let previous = load_cache(&target, &flow)?;
    let secrets = preflight(&flow)?;
    if let Some(cache) = &previous {
        let bytes = serde_json::to_vec(cache).unwrap();
        if secrets.values().any(|secret| {
            let encoded = serde_json::to_vec(secret).unwrap();
            let escaped = &encoded[1..encoded.len() - 1];
            bytes.windows(escaped.len()).any(|slice| slice == escaped)
        }) {
            return Err(failure("replay.cache_invalid", "/cache", 2));
        }
    }
    data["flowId"] = args.id.clone().into();
    observer_ready(&project)?;
    let reused = previous
        .as_ref()
        .filter(|cache| cache["flowDigest"] == digest);
    let viewport = previous
        .as_ref()
        .map(|cache| cache["viewport"].clone())
        .unwrap_or(json!({"width":1280,"height":720}));
    data["flowDigest"] = digest.clone().into();
    data["viewport"] = viewport.clone();
    data["cacheReused"] = reused.is_some().into();
    // Lock acquisition is nonblocking, and every writer uses the same per-flow lock.
    safe_directory(&directory, true)?;
    let lockpath = directory.join(format!("{}.lock", args.id));
    if std::fs::symlink_metadata(&lockpath).is_ok() && !safe_node(&lockpath) {
        return Err(failure("replay.cache_invalid", "/cache/lock", 2));
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lockpath)
        .map_err(|_| failure("replay.cache_invalid", "/cache/lock", 2))?;
    lock.try_lock_exclusive()
        .map_err(|_| failure("replay.cache_busy", "/cache/lock", 3))?;
    if load_cache(&target, &flow)? != previous {
        return Err(failure("replay.cache_changed", "/cache", 2));
    }
    let snapshot = super::journey_flow::read_for_replay(&file, &project)
        .map_err(|_| failure("replay.source_changed", "/flow", 2))?;
    if snapshot != flow {
        return Err(failure("replay.source_changed", "/flow", 2));
    }
    let temporary = TemporaryOutput::create()?;
    let mut cache = json!({"schema":"graphhelm.journey-replay-cache/1","id":args.id,"flowDigest":digest,"viewport":viewport,"screens":{},"edges":{}});
    let screens: BTreeMap<_, _> = flow["screens"]
        .as_array()
        .unwrap()
        .iter()
        .map(|screen| (screen["id"].as_str().unwrap(), screen))
        .collect();
    let edges: BTreeMap<_, _> = flow["edges"]
        .as_array()
        .unwrap()
        .iter()
        .map(|edge| (edge["id"].as_str().unwrap(), edge))
        .collect();
    let mut paths: Vec<_> = flow["paths"].as_object().unwrap().iter().collect();
    paths.sort_by(|(a, _), (b, _)| (a.as_str() != "main", a).cmp(&(b.as_str() != "main", b)));
    data["paths"]=paths.iter().map(|(name,path_edges)| {
        let first=edges[path_edges[0].as_str().unwrap()]["from"].as_str().unwrap();
        let contract=if name.as_str()=="main" {args.id.clone()} else {format!("{}.{}",args.id,name)};
        let visited:Vec<_>=std::iter::once(first).chain(path_edges.as_array().unwrap().iter().map(|id|edges[id.as_str().unwrap()]["to"].as_str().unwrap())).collect();
        json!({"name":name,"contractId":contract,"outcome":"unobserved","observedScreens":[],"capturedSignalIds":[],"walkedPairs":[],"unresolvedSteps":visited})
    }).collect::<Vec<_>>().into();
    for (index, (name, path_edges)) in paths.into_iter().enumerate() {
        let first = edges[path_edges[0].as_str().unwrap()]["from"]
            .as_str()
            .unwrap();
        let contract = if name == "main" {
            args.id.clone()
        } else {
            format!("{}.{}", args.id, name)
        };
        let visited: Vec<_> = std::iter::once(first)
            .chain(
                path_edges
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| edges[id.as_str().unwrap()]["to"].as_str().unwrap()),
            )
            .collect();
        let mut path_data = data["paths"][index].clone();
        path_data["outcome"] = "running".into();
        data["paths"][index] = path_data.clone();
        let mut driver = Driver::start(&project, temporary.path(), &secrets)?;
        let base = flow["base"].as_str().unwrap();
        driver.call("open",json!({"base":format!("{}{}",base.trim_end_matches('/'),screens[first]["url"].as_str().unwrap()),"viewport":viewport,"allowOrigins":args.allow_origin}),&format!("/paths/{name}/entry"))?;
        let mut last_capture: Option<String> = None;
        for (step_index, screen_id) in visited.iter().enumerate() {
            if step_index > 0 {
                let edge_id = path_edges[step_index - 1].as_str().unwrap();
                let edge = edges[edge_id];
                let mut locators = Vec::new();
                for (act_index, act) in edge["acts"].as_array().unwrap().iter().enumerate() {
                    let pointer = format!("/paths/{name}/edges/{edge_id}/acts/{act_index}");
                    let mut request =
                        json!({"kind":act["kind"],"role":act["role"],"name":act["name"]});
                    if let Some(text) = act.get("text") {
                        request["text"] = text.clone();
                    }
                    if let Some(secret) = act["secret"].as_str() {
                        request["secretEnv"] = format!("GRAPHHELM_SECRET_{secret}").into();
                    }
                    if let Some(cached) = reused {
                        request["locator"] = cached["edges"][edge_id][act_index].clone();
                    }
                    let result = driver.call("act", request, &pointer)?;
                    locators.push(result["locator"].clone());
                    // Observe the actual post-act state even before a multi-act edge is complete.
                    driver.call("snapshot", json!({"expect":[]}), &pointer)?;
                }
                cache["edges"][edge_id] = locators.into();
            }
            let screen_path = format!("/paths/{name}/screens/{screen_id}");
            cache["screens"][*screen_id] =
                observe(&mut driver, screens[screen_id], base, &screen_path)?;
            path_data["observedScreens"]
                .as_array_mut()
                .unwrap()
                .push((*screen_id).into());
            path_data["unresolvedSteps"] = visited[step_index + 1..]
                .iter()
                .map(|id| Value::from(*id))
                .collect::<Vec<_>>()
                .into();
            data["paths"][index] = path_data.clone();
            if args.events.is_some() {
                let image = format!("{name}-{screen_id}.png");
                let capture = driver.call(
                    "capture",
                    json!({"path":image,"maskSecrets":true}),
                    &screen_path,
                )?;
                if capture["path"] != image || capture["masked"] != true {
                    return Err(failure("replay.driver_frame_invalid", screen_path, 1));
                }
                let previous = last_capture
                    .as_deref()
                    .map(|capture| (visited[step_index - 1], capture));
                let signal = record(
                    args,
                    &contract,
                    screen_id,
                    &temporary.path().join(&image),
                    None,
                )?;
                path_data["capturedSignalIds"]
                    .as_array_mut()
                    .unwrap()
                    .push(signal.clone().into());
                // Capture committed before walked: retain its id even if the
                // following mutation refuses or has an uncertain reply.
                data["paths"][index] = path_data.clone();
                if let Some((from, from_capture)) = previous {
                    let transition =
                        walked(args, &contract, from, screen_id, from_capture, &signal)?;
                    path_data["walkedPairs"].as_array_mut().unwrap().push(json!({"from":from,"to":screen_id,"fromCaptureId":from_capture,"toCaptureId":signal,"transitionSignalId":transition}));
                }
                last_capture = Some(signal);
                data["paths"][index] = path_data.clone();
            }
        }
        driver.close()?;
        path_data["outcome"] = "passed".into();
        data["paths"][index] = path_data;
    }
    // Unreachable screens/edges are not invented cache observations.
    if !cache_valid(&cache, &flow) {
        return Err(failure("replay.cache_incomplete", "/cache", 2));
    }
    let current = super::journey_flow::read_for_replay(&file, &project)
        .map_err(|_| failure("replay.source_changed", "/flow", 2))?;
    if current != flow || super::journey_flow::approval_digest(&current) != digest {
        return Err(failure("replay.source_changed", "/flow", 2));
    }
    let mut bytes = serde_json::to_vec_pretty(&cache).unwrap();
    bytes.push(b'\n');
    if bytes.len() as u64 > CACHE_LIMIT {
        return Err(failure("replay.cache_invalid", "/cache", 2));
    }
    if secrets.values().any(|secret| {
        let encoded = serde_json::to_vec(secret).unwrap();
        let escaped = &encoded[1..encoded.len() - 1];
        bytes.windows(escaped.len()).any(|slice| slice == escaped)
    }) {
        return Err(failure("driver.redaction_failed", "/cache", 1));
    }
    // Read back custody immediately before replacement; unsafe or changed targets refuse.
    if load_cache(&target, &flow)? != previous {
        return Err(failure("replay.cache_changed", "/cache", 2));
    }
    safe_directory(&directory, false)?;
    super::journey_flow::atomic_write(&target, &bytes)
        .map_err(|_| failure("replay.cache_write_refused", "/cache", 1))?;
    if load_cache(&target, &flow)? != Some(cache) {
        return Err(failure("replay.cache_write_refused", "/cache", 1));
    }
    data["cachePublished"] = true.into();
    Ok(())
}

pub(crate) fn run(args: &JourneyReplayArgs) -> Outcome {
    let mut data = json!({"flowId":null,"modelCalls":0,"recording":if args.events.is_some() {"required"} else {"not_requested"},"cachePublished":false,"paths":[],"partialEffects":"retained"});
    if !graphhelm_execution::valid_journey_id(&args.id) {
        return report(data, Some(failure("replay.id_invalid", "/id", 3)));
    }
    let bundle = [
        args.events.is_some(),
        args.execution.is_some(),
        args.keyring.is_some(),
        args.key_id.is_some(),
    ];
    if bundle.iter().any(|v| *v) && !bundle.iter().all(|v| *v) {
        return report(
            data,
            Some(failure("replay.recording_incomplete", "/recording", 3)),
        );
    }
    if args.replay_worker {
        if std::env::var_os("GRAPHHELM_REPLAY_WORKER").is_none() {
            return report(data, Some(failure("replay.worker_invalid", "/worker", 3)));
        }
        let mut start = Vec::new();
        let read = std::io::stdin()
            .lock()
            .take(129)
            .read_until(b'\n', &mut start);
        if read.is_err()
            || start != b"{\"protocol\":\"graphhelm-replay-worker/1\",\"start\":true}\n"
        {
            return report(data, Some(failure("replay.worker_invalid", "/worker", 3)));
        }
        let result = replay(args, &mut data);
        if result.is_err() {
            for path in data["paths"].as_array_mut().unwrap() {
                if path["outcome"] == "running" {
                    path["outcome"] = "failed".into();
                }
            }
        }
        return report(data, result.err());
    }
    // Path lookup/environment access and command construction are startup too.
    // Keep even those calls off the bounded caller, not just browser/storage I/O.
    let deadline = Instant::now() + RUN_BUDGET;
    let args = args.clone();
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = tx.send(supervise(&args, deadline));
    });
    rx.recv_timeout(RUN_BUDGET + Duration::from_secs(1))
        .unwrap_or_else(|_| {
            report(
                data,
                Some(failure("replay.timeout", "/startup-or-cleanup", 1)),
            )
        })
}

fn supervise(args: &JourneyReplayArgs, deadline: Instant) -> Outcome {
    let data = json!({"flowId":null,"modelCalls":0,"recording":if args.events.is_some() {"required"} else {"not_requested"},"cachePublished":false,"paths":[],"partialEffects":"uncertain"});
    let bundle = [
        args.events.is_some(),
        args.execution.is_some(),
        args.keyring.is_some(),
        args.key_id.is_some(),
    ];
    let executable = match std::env::current_exe() {
        Ok(path) => path,
        Err(_) => return report(data, Some(failure("replay.worker_invalid", "/worker", 3))),
    };
    let mut command = Command::new(executable);
    command
        .args(["--json", "journey", "replay", &args.id, "--replay-worker"])
        .arg("--project")
        .arg(args.project.as_deref().unwrap_or(Path::new(".")))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    safe_environment(&mut command, true);
    command.env("GRAPHHELM_REPLAY_WORKER", "1");
    let mut secrets = Vec::new();
    for (key, value) in std::env::vars_os()
        .filter(|(key, _)| key.to_string_lossy().starts_with("GRAPHHELM_SECRET_"))
    {
        command.env(key, &value);
        if let Some(value) = value.to_str().filter(|v| !v.is_empty()) {
            secrets.push(value.to_owned());
        }
    }
    for origin in &args.allow_origin {
        command.args(["--allow-origin", origin]);
    }
    if bundle[0] {
        command
            .arg("--events")
            .arg(args.events.as_ref().unwrap())
            .args(["--execution", args.execution.as_ref().unwrap()])
            .arg("--keyring")
            .arg(args.keyring.as_ref().unwrap())
            .args(["--key-id", args.key_id.as_ref().unwrap()]);
    }
    match child_output(
        command,
        b"{\"protocol\":\"graphhelm-replay-worker/1\",\"start\":true}\n".to_vec(),
        deadline.saturating_duration_since(Instant::now()),
        CACHE_LIMIT as usize,
    ) {
        Ok((status, bytes)) => {
            if secrets.iter().any(|secret| {
                let encoded = serde_json::to_vec(secret).unwrap();
                let escaped = &encoded[1..encoded.len() - 1];
                bytes.windows(escaped.len()).any(|slice| slice == escaped)
            }) {
                return report(data, Some(failure("driver.redaction_failed", "/worker", 1)));
            }
            let output: Value = match serde_json::from_slice(&bytes) {
                Ok(output) => output,
                Err(_) => {
                    return report(data, Some(failure("replay.worker_invalid", "/worker", 1)));
                }
            };
            if output["command"] != COMMAND
                || output["ok"].as_bool() != Some(status == 0)
                || !matches!(status, 0..=3)
            {
                return report(data, Some(failure("replay.worker_invalid", "/worker", 1)));
            }
            let diagnostics: Vec<Diagnostic> =
                match serde_json::from_value(output["diagnostics"].clone()) {
                    Ok(value) => value,
                    Err(_) => {
                        return report(data, Some(failure("replay.worker_invalid", "/worker", 1)));
                    }
                };
            Outcome {
                output: CommandOutput {
                    ok: status == 0,
                    command: COMMAND,
                    data: Some(output["data"].clone()),
                    diagnostics,
                },
                exit_code: status,
            }
        }
        Err(error) => report(data, Some(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contract: successful replies are closed typed frames, not trusted arbitrary JSON.
    /// Regression: unknown nested fields, a non-exact locator or false masking silently passes.
    /// Gap: Node request validation does not validate Rust's incoming peer boundary.
    /// Cost: milliseconds, fixed independent peer frames and pipe-I/O channels; no public seam.
    #[test]
    fn incoming_success_frames_reject_unknown_fields_and_unsafe_locator_facts() {
        for bytes in [b"{}".to_vec(), vec![b'x'; 1025]] {
            let received = super::frames(std::io::Cursor::new(bytes), 1024)
                .recv_timeout(Duration::from_secs(1))
                .unwrap();
            assert_eq!(received.unwrap_err().0, "replay.driver_frame_invalid");
        }
        assert_eq!(
            super::frames(std::io::Cursor::new(Vec::<u8>::new()), 1024)
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            None
        );
        let frames = [
            (
                "open",
                json!({}),
                json!({"url":"http://localhost/", "secret":"unexpected"}),
            ),
            (
                "act",
                json!({"role":"button","name":"Save"}),
                json!({"url":"http://localhost/", "locator":{"role":"button","name":"Save","exact":false,"testId":null,"context":null,"nth":null}}),
            ),
            (
                "capture",
                json!({"path":"masked.png"}),
                json!({"path":"masked.png","width":1280,"height":720,"masked":false}),
            ),
        ];
        for (op, request, result) in frames {
            let (writer, requests) = mpsc::sync_channel::<(Vec<u8>, mpsc::SyncSender<bool>)>(1);
            std::thread::spawn(move || {
                let (_, ack) = requests.recv().unwrap();
                ack.send(true).unwrap();
            });
            let (peer, replies) = mpsc::sync_channel(1);
            peer.send(Ok(Some(
                serde_json::to_vec(
                    &json!({"protocol":PROTOCOL,"requestId":1,"ok":true,"result":result}),
                )
                .unwrap(),
            )))
            .unwrap();
            let mut driver = Driver {
                owned: None,
                writer,
                replies,
                sequence: 0,
                secrets: vec![],
                headed: false,
            };
            assert_eq!(
                driver.call(op, request, "/peer").unwrap_err().0,
                "replay.driver_frame_invalid"
            );
        }
    }

    /// #434: a replay snapshot may carry a real page (a Studio Journey tab listing 21 flows is
    /// 13.8 KiB), so its ARIA is accepted up to `SNAPSHOT`; a `discover` snapshot feeds a model
    /// and keeps `SNAPSHOT_DISCOVER`. Regression: one cap for both, and every flow that opens a
    /// busy page stops replaying. Cost: milliseconds, two fixed peer frames.
    #[test]
    fn a_replay_snapshot_accepts_a_real_page_and_a_discover_snapshot_keeps_the_model_budget() {
        let aria = "- heading \"Journey flows\"
".repeat(400);
        assert!(aria.len() > SNAPSHOT_DISCOVER && aria.len() <= SNAPSHOT);
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        for (request, accepted) in [
            (json!({"expect":[]}), true),
            (json!({"expect":[],"discover":true}), false),
        ] {
            let mut result = json!({"url":"http://localhost/","ariaYaml":aria,"controls":[],"fingerprint":fingerprint});
            if request["discover"] == true {
                result["expectations"] = json!([]);
            }
            let (writer, requests) = mpsc::sync_channel::<(Vec<u8>, mpsc::SyncSender<bool>)>(1);
            std::thread::spawn(move || {
                let (_, ack) = requests.recv().unwrap();
                ack.send(true).unwrap();
            });
            let (peer, replies) = mpsc::sync_channel(1);
            peer.send(Ok(Some(
                serde_json::to_vec(
                    &json!({"protocol":PROTOCOL,"requestId":1,"ok":true,"result":result}),
                )
                .unwrap(),
            )))
            .unwrap();
            let mut driver = Driver {
                owned: None,
                writer,
                replies,
                sequence: 0,
                secrets: vec![],
                headed: false,
            };
            let reply = driver.call("snapshot", request.clone(), "/peer");
            assert_eq!(reply.is_ok(), accepted, "{request}: {reply:?}");
            if !accepted {
                assert_eq!(reply.unwrap_err().0, "replay.driver_frame_invalid");
            }
        }
    }

    /// Contract: browser-canonical local origins and declared dynamic segments match exactly.
    /// Regression: literal base-prefix comparison rejects default ports or host casing.
    /// Gap: fixture origins use lowercase IPs and random non-default ports. No seam; milliseconds.
    #[test]
    fn url_patterns_accept_canonical_origins_and_refuse_wrong_destinations() {
        assert!(url_matches(
            "http://APP.test:80",
            "/orders/:id?tab=:v",
            "http://app.test/orders/42?tab=paid#section"
        ));
        assert!(url_matches(
            "https://localhost:443",
            "/orders/:id",
            "https://localhost/orders/42"
        ));
        assert!(!url_matches(
            "http://localhost",
            "/orders/:id",
            "http://localhost:3000/orders/42"
        ));
        assert!(!url_matches(
            "http://localhost",
            "/orders/:id",
            "http://localhost/orders/"
        ));
        assert!(!url_matches(
            "http://localhost",
            "/orders/:id",
            "http://localhost/other/42"
        ));
    }

    fn fault_command(mode: &str, marker: &Path, lock: Option<&Path>) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "commands::journey_replay::tests::fault_process",
                "--ignored",
                "--nocapture",
            ])
            .env("GH_REPLAY_FAULT", mode)
            .env("GH_REPLAY_MARKER", marker)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(lock) = lock {
            command.env("GH_REPLAY_LOCK", lock);
        }
        command
    }

    // Helper subprocess, not a production flag or a test-only public interface.
    #[test]
    #[ignore = "subprocess fault helper"]
    #[allow(
        clippy::zombie_processes,
        reason = "fixture deliberately exits before its pipe-owning descendant; the outer owned job/group terminates and reaps that boundary"
    )]
    fn fault_process() {
        let mode = std::env::var("GH_REPLAY_FAULT").unwrap();
        let marker = PathBuf::from(std::env::var_os("GH_REPLAY_MARKER").unwrap());
        if mode == "descendant" {
            std::fs::write(&marker, b"entered").unwrap();
            while !marker.with_extension("effect").exists() {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::write(marker.with_extension("late"), b"descendant effect").unwrap();
            std::thread::sleep(Duration::from_secs(10));
            return;
        }
        std::fs::write(&marker, b"entered").unwrap();
        let ready = Instant::now() + Duration::from_secs(5);
        while !marker.with_extension("go").exists() {
            assert!(Instant::now() < ready, "fault gate was not released");
            std::thread::sleep(Duration::from_millis(5));
        }
        if mode == "inherited" {
            let _child = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "commands::journey_replay::tests::fault_process",
                    "--ignored",
                    "--nocapture",
                ])
                .env("GH_REPLAY_FAULT", "descendant")
                .env("GH_REPLAY_MARKER", marker.with_extension("descendant"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            while !marker.with_extension("descendant").exists() {
                assert!(Instant::now() < ready);
                std::thread::sleep(Duration::from_millis(5));
            }
            std::fs::write(&marker, b"blocking").unwrap();
            std::io::stdin().read_line(&mut String::new()).unwrap();
            return;
        }
        std::fs::write(&marker, b"blocking").unwrap();
        if mode == "record" {
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(std::env::var_os("GH_REPLAY_LOCK").unwrap())
                .unwrap();
            FileExt::lock_exclusive(&lock).unwrap();
        }
        while !marker.with_extension("effect").exists() {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(marker.with_extension("late"), b"effect").unwrap();
        std::thread::sleep(Duration::from_secs(10));
    }

    fn start_fault(
        mode: &str,
        marker: &Path,
        lock: Option<&Path>,
    ) -> (OwnedChild, Receiver<Result<Option<Vec<u8>>>>) {
        let ready = Instant::now() + Duration::from_secs(5);
        let mut owned = spawn_owned(fault_command(mode, marker, lock), ready).unwrap();
        let child = owned.child.as_mut().unwrap();
        // libtest owns stdout; the fault's reply boundary is its otherwise silent
        // stderr. Drain bounded harness output rather than mistake a prelude for
        // an observer response. Production uses this same supervisor with stdout.
        let harness = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = harness.take(8192).read_to_end(&mut bytes);
        });
        let replies = frames(child.stderr.take().unwrap(), 1024);
        while !marker.exists() {
            assert!(Instant::now() < ready, "fault startup did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::fs::write(marker.with_extension("go"), b"start").unwrap();
        while std::fs::read(marker).ok().as_deref() != Some(b"blocking") {
            assert!(Instant::now() < ready);
            std::thread::sleep(Duration::from_millis(5));
        }
        (owned, replies)
    }

    /// Contract: neither a blocked write nor a blocked response can outlive the supervisor.
    /// Defect/gap: a deadline read before write_all/read does not bound that call;
    /// no prior replay consumer exists. Real pipe-owning child, no production seam.
    /// Cost: ~1.5s, offline, portable test executable; Windows containment observed here.
    #[test]
    fn blocked_input_and_output_are_killed_before_later_effects() {
        for mode in ["write", "read", "inherited"] {
            let dir = tempfile::tempdir().unwrap();
            let marker = dir.path().join("entered");
            let input = if mode == "write" {
                vec![b'x'; 2 * 1024 * 1024]
            } else {
                b"start\n".to_vec()
            };
            let (owned, replies) = start_fault(mode, &marker, None);
            let start = Instant::now();
            let failure = supervise_owned(
                owned,
                input,
                Instant::now() + Duration::from_millis(200),
                replies,
            )
            .unwrap_err();
            assert_eq!(failure.0, "replay.timeout");
            assert!(start.elapsed() < Duration::from_secs(5));
            assert!(
                marker.exists(),
                "fault child never reached the blocking boundary"
            );
            std::fs::write(marker.with_extension("effect"), b"release after cleanup").unwrap();
            std::thread::sleep(Duration::from_millis(300));
            assert!(
                !marker.with_extension("late").exists(),
                "child survived cleanup"
            );
            if mode == "inherited" {
                assert!(
                    marker.with_extension("descendant").exists(),
                    "descendant never inherited the reply pipe"
                );
                assert!(
                    !marker.with_extension("late").exists(),
                    "descendant survived group close"
                );
            }
        }
    }

    /// Contract: a blocked filesystem stage is contained by the same worker supervisor.
    /// Credible regression: reading cache/Git/record synchronously in the ordinary caller.
    /// Existing pipe tests do not observe an OS file lock. Real exclusive lock, offline,
    /// no production seam; ~0.7s. This is blocking-worker proof, not a sealed record receipt.
    #[test]
    fn a_blocked_storage_worker_is_killed_and_a_subsequent_invocation_runs() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("entered");
        let path = dir.path().join("store.lock");
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        FileExt::lock_exclusive(&lock).unwrap();
        let (owned, replies) = start_fault("record", &marker, Some(&path));
        let start = Instant::now();
        let failure = supervise_owned(
            owned,
            b"start\n".to_vec(),
            Instant::now() + Duration::from_millis(200),
            replies,
        )
        .unwrap_err();
        assert_eq!(failure.0, "replay.timeout");
        assert!(start.elapsed() < Duration::from_secs(5));
        assert!(marker.exists());
        FileExt::unlock(&lock).unwrap();
        std::fs::write(marker.with_extension("effect"), b"release after cleanup").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!marker.with_extension("late").exists());
        let mut healthy = Command::new("git");
        healthy
            .arg("--version")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let (status, reply) =
            child_output(healthy, Vec::new(), Duration::from_secs(5), 1024).unwrap();
        assert_eq!(status, 0);
        assert!(reply.starts_with(b"git version "));
        let command = Command::new(dir.path().join("missing-executable"));
        assert_eq!(
            spawn_owned(command, Instant::now() + Duration::from_secs(1))
                .err()
                .unwrap()
                .0,
            "replay.observer_missing"
        );
    }
}
