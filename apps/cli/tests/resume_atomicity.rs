//! Issue #83: `resume` is not atomic against `drive()`'s own setup failure.
//!
//! `execution::resume::execute_prepared` (`apps/cli/src/commands/execution/resume.rs`) commits the
//! resume decision — the `ExecutionResumed` append and the paused-node redispatches — BEFORE the
//! caller's `drive()` (`apps/cli/src/commands/serve/routes.rs:685`) runs its own setup. That setup
//! can fail: `build_sealer` alone rejects a missing or malformed `GRAPHHELM_EVENTS_KEY`. When it
//! did, the operator got a 500 while the store had already recorded the resume and dropped their
//! pause hold. "The call failed" and "your hold still holds" stop being the same fact, silently, at
//! exactly the moment someone is leaning on the hold.
//!
//! **The code these guards assert changed in #96 and the change is deliberate.** #83 fixed the
//! ordering, so a setup refusal commits nothing — but the RESPONSE still said
//! `GHCLI016_DRIVER_FAILURE` for both that and a genuine mid-drive failure, which are opposite
//! hold-states. #96 splits them: a setup refusal now answers `GHCLI019_DRIVER_SETUP` (nothing
//! committed, your hold is intact), and `GHCLI016` is left meaning only "the decision committed and
//! the work then failed". These tests assert the setup class, so they assert the new code; that
//! they had to change is the point, not an accident.
//!
//! The graph is `docs/acceptance/m09-judge-run-2026-08-19/release.yaml` — deliberately not a fresh
//! fixture. It is the graph the M09 paid run started, paused and resumed over this same API, i.e.
//! the one that produced the issue's observation 1, so the guard reproduces the defect on the
//! artefact that discovered it. Its three nodes are all `type: tool`; note the node NAMED `deploy`
//! is a Tool, not a `NodeType::Deploy` — `classify.rs` reads the type, and only an all-classifiable
//! graph makes `drive_is_viable_for` (`routes.rs:765`) answer true with no runtime configured,
//! which is what puts the request on the async drive path where the defect lives at all.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

mod support;
use support::{RawResponse, parse_response, raw_request, split_url};
struct ServerGuard {
    child: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn token_path(events: &Path) -> PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".token");
    events.with_file_name(name)
}

fn read_token(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && !contents.is_empty()
        {
            return contents;
        }
        if Instant::now() >= deadline {
            panic!("the server never wrote a readable token file at {path:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn wait_for_health(base: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(response) = raw_request(&format!("{base}/health"), None)
            && response.status == 200
        {
            return;
        }
        if Instant::now() >= deadline {
            panic!("the server never answered /health at {base}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn get_json(url: &str, token: Option<&str>) -> Value {
    let response =
        raw_request(url, token).unwrap_or_else(|error| panic!("request to {url} failed: {error}"));
    serde_json::from_str(&response.body).unwrap_or_else(|error| {
        panic!(
            "response body from {url} was not JSON ({error}): {:?}",
            response.body
        )
    })
}

/// The repository root, for naming fixtures that live in-tree.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_json(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

/// Runs `graphhelm` with `args` and returns its parsed stdout envelope, panicking with the raw
/// output on a non-zero exit so a bad fixture fails loudly at the call site rather than at a later,
/// more confusing assertion.
fn cli(args: &[&str]) -> Value {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "graphhelm {args:?} failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not valid JSON ({error}): {:?}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// A minimal HTTP/1.1 POST carrying the mutation headers every command route requires.
fn post_request(
    url: &str,
    token: &str,
    extra_headers: &[(&str, &str)],
    body: &Value,
) -> RawResponse {
    let (host, port, path) = split_url(url);
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();

    let payload = serde_json::to_vec(body).unwrap();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        payload.len()
    );
    for (name, value) in extra_headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    parse_response(&String::from_utf8_lossy(&raw)).unwrap()
}

fn post_json(url: &str, token: &str, extra_headers: &[(&str, &str)], body: &Value) -> (u16, Value) {
    let response = post_request(url, token, extra_headers, body);
    let parsed = serde_json::from_str(&response.body).unwrap_or_else(|error| {
        panic!(
            "response body from {url} was not JSON ({error}): {:?}",
            response.body
        )
    });
    (response.status, parsed)
}

fn head_sequence(base: &str, token: &str, execution: &str) -> u64 {
    get_json(&format!("{base}/v1/executions/{execution}"), Some(token))["data"]["headSequence"]
        .as_u64()
        .unwrap_or_else(|| panic!("status carried no numeric headSequence for {execution}"))
}

/// `serve` with explicit control of `GRAPHHELM_EVENTS_KEY`.
///
/// ANY TEST ASSERTING ON SEALER BEHAVIOUR OWNS ITS ENVIRONMENT OR OWNS NOTHING. The plain
/// `serve_with` helpers elsewhere in this suite spawn the child with no env call at all, so it
/// inherits the parent's — and `build_sealer` reads `GRAPHHELM_EVENTS_KEY` straight out of it. A
/// developer with that variable exported would silently get the opposite arrangement from CI. So
/// `key: None` REMOVES the variable rather than merely declining to set it.
fn serve_with_env(
    events: &Path,
    extra: &[&str],
    key: Option<&str>,
) -> (ServerGuard, String, String) {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .args(extra);
    match key {
        Some(value) => command.env("GRAPHHELM_EVENTS_KEY", value),
        None => command.env_remove("GRAPHHELM_EVENTS_KEY"),
    };
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    let read = stdout.read_line(&mut line).unwrap();
    if read == 0 {
        let mut stderr_text = String::new();
        let _ = child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr_text);
        let status = child.wait().unwrap();
        panic!(
            "`graphhelm serve` produced no stdout before exiting (status: {status}); stderr:\n{stderr_text}"
        );
    }
    let started: Value = serde_json::from_str(line.trim())
        .unwrap_or_else(|error| panic!("the startup line was not valid JSON ({error}): {line:?}"));
    assert_eq!(
        started["ok"], true,
        "expected a successful startup: {started}"
    );
    let address = started["data"]["address"]
        .as_str()
        .unwrap_or_else(|| panic!("the startup envelope must carry data.address: {started}"))
        .to_owned();
    let base = format!("http://{address}");
    let token = read_token(&token_path(events));
    wait_for_health(&base);
    (ServerGuard { child }, base, token)
}

/// Every event strictly after `after`, read through the paged tail.
fn events_after(base: &str, token: &str, execution: &str, after: u64) -> Vec<Value> {
    let page = get_json(
        &format!("{base}/v1/executions/{execution}/events?after={after}&limit=1000"),
        Some(token),
    );
    page["data"]["events"]
        .as_array()
        .unwrap_or_else(|| panic!("the events tail carried no array: {page}"))
        .clone()
}

/// The M09 paid run's own release graph — see this file's header for why it, and not a fresh one.
fn release_graph() -> PathBuf {
    root().join("docs/acceptance/m09-judge-run-2026-08-19/release.yaml")
}

/// Arranges a paused execution on the async-drive path, with sealing configured and
/// `GRAPHHELM_EVENTS_KEY` absent so `build_sealer` — and therefore `drive()`'s setup — fails.
/// Returns the server guard, base, token and the watermark taken AFTER the pause.
fn paused_execution_with_failing_setup(
    directory: &Path,
    execution: &str,
) -> (ServerGuard, String, String, u64) {
    let events = directory.join("events");
    let keyring = directory.join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let fixtures = write_json(
        directory,
        "fixtures.json",
        &serde_json::json!({ "nodeOutcomes": { "preflight": "success", "deploy": "failure" } }),
    );

    cli(&[
        "execution",
        "start",
        "--file",
        release_graph().to_str().unwrap(),
        "--events",
        events.to_str().unwrap(),
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--mode",
        "supervised",
        "--execution",
        execution,
    ]);

    let (guard, base, token) = serve_with_env(
        &events,
        &[
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "issue83-key",
        ],
        None,
    );

    let (status, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/pause"),
        &token,
        &[
            ("Idempotency-Key", "issue83-pause"),
            ("X-GraphHelm-Actor", "agent-operator"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &serde_json::json!({}),
    );
    assert_eq!(status, 200, "ARRANGEMENT: pause must succeed: {reply}");
    assert_eq!(
        reply["data"]["status"], "paused",
        "ARRANGEMENT: the execution must reach paused: {reply}"
    );

    let watermark = head_sequence(&base, &token, execution);
    (guard, base, token, watermark)
}

/// Posts the resume whose `drive()` setup fails, and returns `(status, reply)`.
fn resume_with_failing_setup(base: &str, token: &str, execution: &str, key: &str) -> (u16, Value) {
    post_json(
        &format!("{base}/v1/executions/{execution}/resume"),
        token,
        &[
            ("Idempotency-Key", key),
            ("X-GraphHelm-Actor", "agent-operator"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &serde_json::json!({ "file": release_graph().to_str().unwrap() }),
    )
}

/// #83's guard. A resume the operator sees REFUSED must leave their hold exactly as it was.
///
/// The journal assertion is watermark-anchored and landmark-proving, and both halves are load
/// bearing. WATERMARK, not "the last event": the `Started` redispatches append AFTER
/// `ExecutionResumed` (`resume.rs:230-243`), so a last-event formulation is green TODAY, before any
/// fix. LANDMARK: absence is evidence only when the same read proves it is looking at the right
/// stream — a wrong execution id would make "no `execution_resumed`" pass vacuously forever, so the
/// read must first show the arrangement's own pause at or below the watermark.
#[test]
fn a_resume_whose_drive_setup_fails_leaves_the_operator_hold_intact() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-83-hold-intact";
    let (_guard, base, token, watermark) =
        paused_execution_with_failing_setup(directory.path(), execution);

    let (status, reply) = resume_with_failing_setup(&base, &token, execution, "issue83-resume-1");

    // (1) POSITIVE CONTROL, before the guard may speak: this proves the injection fired AND that
    // the request took the async drive path. `GHCLI005_EXECUTION_STATE` here would mean a
    // precondition refusal that never reached the commit — the guard would then pass for a reason
    // that has nothing to do with the defect, which is a FALSE GREEN, not a result.
    assert_eq!(
        status, 500,
        "ARRANGEMENT: the drive setup must fail, giving a driver failure: {reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["code"], "GHCLI019_DRIVER_SETUP",
        "ARRANGEMENT: the failure must be the driver's setup, not a precondition refusal: {reply}"
    );

    // Landmark: this read is provably looking at the arrangement's own stream.
    let whole = events_after(&base, &token, execution, 0);
    assert!(
        whole.iter().any(|event| {
            event["kind"]["type"] == "execution_paused"
                && event["sequence"]
                    .as_u64()
                    .is_some_and(|seq| seq <= watermark)
        }),
        "ARRANGEMENT: the journal read must contain this test's own pause at or below the \
         watermark {watermark}, otherwise an absence below proves nothing: {whole:?}"
    );

    // (2) THE GUARD.
    let tail = events_after(&base, &token, execution, watermark);
    let resumed: Vec<&Value> = tail
        .iter()
        .filter(|event| event["kind"]["type"] == "execution_resumed")
        .collect();
    assert!(
        resumed.is_empty(),
        "a resume refused with GHCLI019 committed ExecutionResumed anyway, so the operator's hold \
         was dropped by a call that answered failure (watermark {watermark}): {resumed:?}"
    );
}

/// The operator-visible half of the same invariant, and the judge's own recorded symptom: after a
/// resume that answered failure, the hold is still theirs, so a SECOND resume must be ACCEPTED.
/// Today the first call silently consumed the pause, so the retry is refused `not_paused` — the
/// operator is told their hold never existed by the same system that just told them the call failed.
///
/// Its own test rather than a later assertion in the guard above, so that it EXECUTES and reports
/// even when the guard fails first.
#[test]
fn a_second_resume_after_a_failed_one_is_still_accepted() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-83-second-resume";
    let (_guard, base, token, _watermark) =
        paused_execution_with_failing_setup(directory.path(), execution);

    let (first_status, first_reply) =
        resume_with_failing_setup(&base, &token, execution, "issue83-resume-a");
    assert_eq!(
        first_status, 500,
        "ARRANGEMENT: the first resume must fail in drive setup: {first_reply}"
    );

    let (second_status, second_reply) =
        resume_with_failing_setup(&base, &token, execution, "issue83-resume-b");
    assert_ne!(
        second_reply["diagnostics"][0]["code"], "GHCLI005_EXECUTION_STATE",
        "the retry was refused as not-paused: the first call answered failure yet took the hold \
         with it, so the operator cannot retry the thing they were told did not happen: \
         {second_reply}"
    );
    assert_eq!(
        second_status, 500,
        "the retry must reach the same driver failure, not a state refusal: {second_reply}"
    );
}

/// 64 lowercase hex characters whose decoded bytes are the material [`sealed_keyring`] builds the
/// keyring from. The two MUST agree: `build_sealer` uses the decoded key as the passphrase that
/// opens the sealed keyring, so a well-formed key over a keyring built from different material
/// fails with "the sealed keyring could not be opened" — a broken ARRANGEMENT, not a product
/// refusal. The first draft of this control passed `"a".repeat(64)` over an empty directory and hit
/// exactly that, which is the control catching its own arrangement before it could certify anything.
const SEALING_KEY_HEX: &str = "0101010101010101010101010101010101010101010101010101010101010101";

/// A real sealed keyring holding `issue83-key`, so `build_sealer` can actually succeed.
fn sealed_keyring(directory: &Path) -> PathBuf {
    let keyring = directory.join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        "issue83-key",
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    keyring
}

/// POSITIVE CONTROL. Without this the guards above pass trivially the moment `resume` stops working
/// at all — "no `ExecutionResumed` after the watermark" is exactly what a totally broken resume
/// produces. So: same arrangement, same graph, same watermark grain, but with a VALID
/// `GRAPHHELM_EVENTS_KEY`, the resume must SUCCEED and the decision must land.
///
/// It is a test rather than a one-off sabotage because the property it protects is permanent: it
/// fails if anyone ever "fixes" #83 by making the resume path refuse more.
#[test]
fn a_resume_whose_drive_setup_succeeds_still_commits_the_decision() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-83-positive-control";
    let events = directory.path().join("events");
    let keyring = sealed_keyring(directory.path());
    let fixtures = write_json(
        directory.path(),
        "fixtures.json",
        &serde_json::json!({ "nodeOutcomes": { "preflight": "success", "deploy": "failure" } }),
    );

    cli(&[
        "execution",
        "start",
        "--file",
        release_graph().to_str().unwrap(),
        "--events",
        events.to_str().unwrap(),
        "--fixtures",
        fixtures.to_str().unwrap(),
        "--mode",
        "supervised",
        "--execution",
        execution,
    ]);

    let (_guard, base, token) = serve_with_env(
        &events,
        &[
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "issue83-key",
        ],
        Some(SEALING_KEY_HEX),
    );

    let (status, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/pause"),
        &token,
        &[
            ("Idempotency-Key", "issue83-pc-pause"),
            ("X-GraphHelm-Actor", "agent-operator"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &serde_json::json!({}),
    );
    assert_eq!(status, 200, "ARRANGEMENT: pause must succeed: {reply}");

    let watermark = head_sequence(&base, &token, execution);
    let (resume_status, resume_reply) =
        resume_with_failing_setup(&base, &token, execution, "issue83-pc-resume");
    assert_eq!(
        resume_status, 200,
        "with a valid sealing key the resume must succeed, not refuse: {resume_reply}"
    );

    let resumed: Vec<Value> = events_after(&base, &token, execution, watermark)
        .into_iter()
        .filter(|event| event["kind"]["type"] == "execution_resumed")
        .collect();
    assert_eq!(
        resumed.len(),
        1,
        "a resume that SUCCEEDED must commit exactly one ExecutionResumed after the watermark \
         {watermark} — if this is 0 the guards above are passing because resume is broken, not \
         because it is atomic"
    );
}

/// `start`'s thin guard. The fix lands in the shared route/`drive()` shape, so `start`
/// (`routes.rs:277`) is repaired by the same hoist that repairs `resume` — for free, and therefore
/// silently. A route fixed for free but left unguarded is how the defect grows back, so this is
/// deliberately the cheapest possible test at the `ExecutionStarted` grain rather than a mirror of
/// the resume suite.
///
/// The landmark here is the graph publish rather than a pause: `load_and_publish` runs before the
/// viability branch, so it is present even on a refused start — which is exactly what proves this
/// read is looking at the right stream before an absence below it is believed.
#[test]
fn a_start_whose_drive_setup_fails_commits_no_execution() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-83-start-atomicity";
    let events = directory.path().join("events");
    let keyring = sealed_keyring(directory.path());
    let fixtures = write_json(
        directory.path(),
        "fixtures.json",
        &serde_json::json!({ "nodeOutcomes": { "preflight": "success", "deploy": "failure" } }),
    );

    let (_guard, base, token) = serve_with_env(
        &events,
        &[
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "issue83-key",
        ],
        None,
    );

    let (status, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "issue83-start"),
            ("X-GraphHelm-Actor", "agent-operator"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &serde_json::json!({
            "file": release_graph().to_str().unwrap(),
            "fixtures": fixtures.to_str().unwrap(),
            "mode": "supervised",
        }),
    );
    assert_eq!(
        status, 500,
        "ARRANGEMENT: the drive setup must fail: {reply}"
    );
    assert_eq!(
        reply["diagnostics"][0]["code"], "GHCLI019_DRIVER_SETUP",
        "ARRANGEMENT: must be a driver-setup failure, not a precondition refusal: {reply}"
    );

    // THE GUARD. Note what this absence cannot prove on its own: a refused start legitimately
    // leaves the execution stream EMPTY, and an empty read is exactly what a wrong stream id or a
    // broken read helper also produces. There is no landmark available here the way the resume
    // guard has its own pause — the graph publish lands on the graph stream, not this one. So the
    // absence is made self-evident below instead of asserted on trust.
    let refused = events_after(&base, &token, execution, 0);
    let started: Vec<&Value> = refused
        .iter()
        .filter(|event| event["kind"]["type"] == "execution_started")
        .collect();
    assert!(
        started.is_empty(),
        "a start refused with GHCLI019 committed ExecutionStarted anyway, leaving an execution \
         that is started but will never be driven: {started:?}"
    );

    // PAIRED CONTROL, and the reason the assertion above means anything: the SAME read against the
    // SAME execution id must go from empty to non-empty once the setup succeeds. A wrong-stream or
    // dead read cannot do that. This also shows the refused start consumed nothing — the operator
    // can simply try again, which is the whole point of not committing.
    drop(_guard);
    let (_second_guard, base, token) = serve_with_env(
        &events,
        &[
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "issue83-key",
        ],
        Some(SEALING_KEY_HEX),
    );
    let (retry_status, retry_reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "issue83-start-retry"),
            ("X-GraphHelm-Actor", "agent-operator"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &serde_json::json!({
            "file": release_graph().to_str().unwrap(),
            "fixtures": fixtures.to_str().unwrap(),
            "mode": "supervised",
        }),
    );
    assert_eq!(
        retry_status, 200,
        "the retry after a refused start must be accepted: {retry_reply}"
    );
    assert!(
        events_after(&base, &token, execution, 0)
            .iter()
            .any(|event| event["kind"]["type"] == "execution_started"),
        "CONTROL: the same read that reported an empty stream must see ExecutionStarted once the \
         start succeeds — otherwise the emptiness above was the read, not the store"
    );
}

/// B's S4, and a blade the fresh-key test above does NOT carry. `a_second_resume_after_a_failed_one`
/// retries with a DIFFERENT Idempotency-Key, which asks "did the hold survive". This asks something
/// else: the SAME key must execute FRESH.
///
/// The distinction is the operator's actual retry loop. A client that gets a 500 retries the
/// identical request — same key, same body — because that is what an idempotency key is FOR. Before
/// the fix the refused call still committed, and the committed event carried that key (visible in
/// the red's own evidence: idempotencyKey "issue83-resume-1-resumed-..."), so the retry met a key
/// the store had already seen and was answered from the idempotency path instead of being executed.
/// After the fix nothing was committed, so the key was never spent and the retry runs for real.
///
/// It becomes correct silently under the fix, which is exactly why it needs a guard: nothing else
/// here would notice it regressing.
#[test]
fn the_same_idempotency_key_after_a_failed_setup_still_executes() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-83-same-key";
    let (_guard, base, token, watermark) =
        paused_execution_with_failing_setup(directory.path(), execution);

    let (first_status, first_reply) =
        resume_with_failing_setup(&base, &token, execution, "issue83-same-key");
    assert_eq!(
        first_status, 500,
        "ARRANGEMENT: the first resume must fail in drive setup: {first_reply}"
    );

    let (retry_status, retry_reply) =
        resume_with_failing_setup(&base, &token, execution, "issue83-same-key");
    assert_eq!(
        retry_reply["diagnostics"][0]["code"], "GHCLI019_DRIVER_SETUP",
        "the same-key retry must reach the driver setup again — a replayed or conflicted answer \
         means the refused call spent the key it never should have taken: {retry_reply}"
    );
    assert_eq!(
        retry_status, 500,
        "the same-key retry must execute fresh, not replay a stored outcome: {retry_reply}"
    );

    // And the retry must not have committed either — the whole property, not just the response.
    let resumed: Vec<Value> = events_after(&base, &token, execution, watermark)
        .into_iter()
        .filter(|event| event["kind"]["type"] == "execution_resumed")
        .collect();
    assert!(
        resumed.is_empty(),
        "neither the call nor its same-key retry may commit the decision: {resumed:?}"
    );
}
