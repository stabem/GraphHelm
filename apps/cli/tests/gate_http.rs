//! M06 Task 8: the gate flow through the REAL serve surface — the one wiring no other
//! suite covers: `routes.rs` hands the driver the pathogen-suite digest this binary
//! carries, so certified-or-not-at-all holds over HTTP exactly as it holds in the driver
//! tests. The model route configured for these stories HANGS FOREVER: a gate that touched
//! the model port would wedge the test — completing fast IS the no-model-transport proof
//! at the HTTP level.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

mod support;
use support::{parse_response, raw_request, split_url};

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

struct ServeExtra {
    args: Vec<String>,
    env: Vec<(String, String)>,
}

fn serve_with(events: &Path, extra: &ServeExtra) -> (ServerGuard, String, String) {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .args(&extra.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in &extra.env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
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
        panic!("serve exited before startup (status: {status}); stderr:\n{stderr_text}");
    }
    let started: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(started["ok"], true, "startup: {started}");
    let address = started["data"]["address"].as_str().unwrap().to_owned();
    let token = read_token(&token_path(events));
    let base = format!("http://{address}");
    wait_for_health(&base);
    (ServerGuard { child }, base, token)
}

fn read_token(path: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(contents) = std::fs::read_to_string(path)
            && !contents.is_empty()
        {
            return contents;
        }
        assert!(Instant::now() < deadline, "no token file at {path:?}");
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
        assert!(Instant::now() < deadline, "no /health at {base}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn post_json(url: &str, token: &str, key: &str, actor_type: &str, body: &Value) -> (u16, Value) {
    let (host, port, path) = split_url(url);
    let mut stream = TcpStream::connect((host.as_str(), port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let payload = serde_json::to_vec(body).unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nIdempotency-Key: {key}\r\nX-GraphHelm-Actor: owner-gate-test\r\nX-GraphHelm-Actor-Type: {actor_type}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let response = parse_response(&String::from_utf8_lossy(&raw)).unwrap();
    let value = serde_json::from_str(&response.body).unwrap_or(Value::Null);
    (response.status, value)
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gateway_key() -> String {
    "ab".repeat(32)
}

/// A model endpoint that never answers: a gate that touched the model port would wedge
/// here, so a fast completion IS the deterministic-transport proof over HTTP.
fn hang_forever_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buffer = [0_u8; 4096];
            let _ = stream.read(&mut buffer);
            std::thread::sleep(Duration::from_secs(600));
        }
    });
    format!("http://{addr}")
}

fn credential_set(broker: &Path, keyring: &Path, key_id: &str, reference: &str, route: &str) {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "gateway",
            "credential",
            "set",
            "--broker",
            broker.to_str().unwrap(),
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            key_id,
            "--ref",
            reference,
            "--provider",
            "anthropic",
            "--usable-by",
            route,
        ])
        .env("GRAPHHELM_GATEWAY_KEY", gateway_key())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"sk-ant-gate-http-test-value")?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(
        output.status.success(),
        "credential set failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The repository's own positive retry-lineage document, embedded so a moved path breaks the
/// BUILD rather than silently leaving the retry-lineage node with evidence it does not judge.
const RECOVERED_RETRY_CHAIN: &str = include_str!(
    "../../../extensions/builtin/graphhelm-jpd/fixtures/positive/recovered-retry-chain.json"
);

/// The evidence a node carries for `gate_id` (#668).
///
/// Before per-gate dispatch every gate node carried geometry's evidence, because geometry's
/// evaluator was the only one a running graph could reach. Now the node carries what the gate it
/// names actually judges: a lineage document for `gate-retry-lineage`, the delivered surface for
/// geometry.
///
/// `gate-journey-contract` deliberately keeps the geometry-shaped surface. A journey contract
/// that passes all eighteen of that gate's checks is a fixture in its own right and is not built
/// here; what this file proves about that gate is that ITS OWN evaluator answered — which its
/// wrong-evidence refusal states in a vocabulary geometry has no way to produce.
fn gate_check_block(gate_id: &str) -> serde_json::Value {
    if gate_id == "gate-geometry-misspelled" {
        // The same coherent surface, with ONE key misspelled: `manifst`. Before the evidence was
        // carried unparsed, `deny_unknown_fields` on the work struct refused this in the driver;
        // the geometry evaluator is where it is caught now, and the cell below pins that the
        // OUTCOME is still a refusal rather than a permanent verdict (#771, found by L).
        let mut block = gate_check_block("gate-geometry");
        let object = block
            .as_object_mut()
            .expect("the geometry block is an object");
        let manifest = object
            .remove("manifest")
            .expect("the geometry block carries a manifest");
        object.insert("manifst".to_owned(), manifest);
        object.insert("gateId".to_owned(), serde_json::json!("gate-geometry"));
        return block;
    }
    if gate_id == "gate-retry-lineage" {
        let document: serde_json::Value = serde_json::from_str(RECOVERED_RETRY_CHAIN)
            .expect("the shipped retry-lineage fixture parses");
        return serde_json::json!({"gateId": gate_id, "document": document});
    }
    serde_json::json!({
        "gateId": gate_id,
        "delivered": {
            "claims": [{"feature": "node table", "elementId": "nodes-table", "artifact": "monitor-snapshot.html"}],
            "html": "<a href=\"#nodes-table\">nodes</a><section id=\"nodes-table\">node table<table><tr><th>a</th></tr><tr><td>1</td></tr></table></section>",
            "reachableIds": ["nodes-table"],
            "journey": [
                {"action": "open monitor", "assertion": "node table renders", "exercisesErrorPath": false},
                {"action": "break token", "assertion": "node table refuses", "exercisesErrorPath": true}
            ],
            "tests": [{"name": "renders", "passed": true, "assertions": 3}],
            "diff": {"filesTouched": 2, "behaviorLines": 14}
        },
        "manifest": {"required": [{"marker": "id=\"nodes-table\"", "label": "node table"}]}
    })
}

/// A single-gate graph whose contract carries the evidence the named gate judges — the same
/// passing subject shape the Task 7 dogfood ran, for geometry.
fn gate_graph(directory: &Path, execution: &str, gate_id: &str) -> PathBuf {
    let gate_block = serde_json::json!({"check": gate_check_block(gate_id)});
    let yaml = format!(
        r#"apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_gate_http_v1
  name: gate over http
  executionId: {execution}
  version: 1
spec:
  entrypoints:
    - quality_gate
  nodes:
    quality_gate:
      type: gate
      name: Geometry gate
      objective: Refuse a useless delivery.
      optionality: required
      gate: {gate_block}
      completion:
        requires:
          - expression: output.executed > 0
  edges: []
  budgets:
    maxParallelModelCalls: 1
  completion:
    terminalNodes:
      - quality_gate
"#
    );
    let path = directory.join("gate-graph.yaml");
    std::fs::write(&path, yaml).unwrap();
    path
}

struct GateServe {
    _guard: ServerGuard,
    base: String,
    token: String,
    events: PathBuf,
    graph: PathBuf,
}

fn gate_serve(directory: &Path, execution: &str) -> GateServe {
    gate_serve_for(directory, execution, "gate-geometry")
}

fn gate_serve_for(directory: &Path, execution: &str, gate_id: &str) -> GateServe {
    let events = directory.join("events");
    let broker = directory.join("broker");
    let keyring = directory.join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let staging = directory.join("staging");
    let key_id = "gate-http-key";
    let route_id = "hang_route";
    let base_url = hang_forever_server();
    credential_set(&broker, &keyring, key_id, "cred_gate", route_id);
    let manifest = directory.join("manifest.json");
    std::fs::write(
        &manifest,
        serde_json::to_vec(&serde_json::json!({
            "manifestVersion": 1,
            "routes": [{
                "id": route_id,
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": base_url,
                "model": "claude-sonnet-5",
                "credentialRef": "cred_gate",
                "profiles": ["critical_reasoning"],
                "enabled": true,
                "timeoutSeconds": 55
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    let extra = ServeExtra {
        args: vec![
            "--manifest".into(),
            manifest.to_str().unwrap().into(),
            "--broker".into(),
            broker.to_str().unwrap().into(),
            "--keyring".into(),
            keyring.to_str().unwrap().into(),
            "--key-id".into(),
            key_id.into(),
            "--route".into(),
            route_id.into(),
            "--staging".into(),
            staging.to_str().unwrap().into(),
            "--allow-program".into(),
            "git".into(),
            "--allow-program".into(),
            "cargo".into(),
        ],
        env: vec![
            ("GRAPHHELM_GATEWAY_KEY".to_owned(), gateway_key()),
            ("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key()),
        ],
    };
    let (guard, base, token) = serve_with(&events, &extra);
    let graph = gate_graph(directory, execution, gate_id);
    GateServe {
        _guard: guard,
        base,
        token,
        events,
        graph,
    }
}

fn start(serve: &GateServe, execution: &str, key: &str) -> (u16, Value) {
    post_json(
        &format!("{}/v1/executions/{execution}/start", serve.base),
        &serve.token,
        key,
        "owner",
        &serde_json::json!({
            "file": serve.graph.to_str().unwrap(),
            "mode": "autopilot",
            "project": root().to_str().unwrap(),
        }),
    )
}

fn certify(serve: &GateServe, gate: &str) -> std::process::Output {
    Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "quality",
            "certify",
            "--events",
            serve.events.to_str().unwrap(),
            "--gate",
            gate,
        ])
        .output()
        .unwrap()
}

/// The collapse this file's instrument used to perform, demonstrated rather than described.
///
/// The events endpoint answers these two situations DIFFERENTLY, and the difference is the whole
/// diagnosis in #237:
///
///   * a malformed execution id  -> a failure status, with no `data.events` at all
///   * a well-formed unknown id  -> 200, with `data.events` present and empty
///
/// The old helper turned BOTH into `[]`, and turned a genuinely empty stream into `[]` as well.
/// Three different facts about the server, one observable — which is why re-running the flaky test
/// could never separate its five candidate causes.
///
/// This guard fails if that distinction is ever lost again: if the malformed id starts answering
/// 200, or the unknown id stops carrying an `events` array.
#[test]
fn the_events_endpoint_distinguishes_a_refusal_from_a_genuinely_empty_stream() {
    let directory = tempfile::tempdir().unwrap();
    let serve = gate_serve(directory.path(), "exec-gate-http-distinguish");

    // Over `OpaqueId`'s documented 128-character cap (see serve/mod.rs), so it fails id parsing
    // for a reason that does not depend on guessing the allowed character set - and it stays a
    // legal HTTP path, which a space would not: a space breaks the request LINE, and the test would
    // then be measuring the HTTP parser rather than the endpoint.
    let too_long = "e".repeat(200);
    let malformed = raw_request(
        &format!("{}/v1/executions/{too_long}/events?after=0", serve.base),
        Some(&serve.token),
    )
    .expect("the malformed-id request completes");
    assert_ne!(
        malformed.status, 200,
        "a malformed execution id must NOT answer 200 - answering 200 here is what let a refusal read back as an empty event list: {}",
        malformed.body
    );

    let unknown = raw_request(
        &format!(
            "{}/v1/executions/exec-gate-http-nobody/events?after=0",
            serve.base
        ),
        Some(&serve.token),
    )
    .expect("the unknown-id request completes");
    assert_eq!(
        unknown.status, 200,
        "a well-formed but unknown execution is not an error, it is an empty stream: {}",
        unknown.body
    );
    let parsed: Value =
        serde_json::from_str(&unknown.body).expect("the unknown-id response is JSON");
    assert!(
        parsed["data"]["events"].is_array(),
        "an empty stream must still carry an `events` ARRAY - absence and emptiness are different facts and the helper now refuses to conflate them: {parsed}"
    );
    assert_eq!(
        parsed["data"]["events"].as_array().map_or(1, Vec::len),
        0,
        "nobody has written to this stream: {parsed}"
    );
}

/// The events read is a SECOND request, issued after `resume` has already answered. Every way it
/// could fail used to collapse into the same empty vector: a non-200, an error envelope, a body
/// that is not JSON, and a genuinely empty page were indistinguishable — `get_json` never looked at
/// the status and `unwrap_or_default()` turned all of them into `[]`.
///
/// That is why #237 could not be diagnosed THROUGH this helper: five different causes produced one
/// observable, so no number of re-runs could separate them. Each failure mode now panics naming
/// which one it was, and "absent" is distinguished from "empty" because they are different facts
/// about the server.
fn event_kinds(serve: &GateServe, execution: &str) -> Vec<String> {
    let url = format!("{}/v1/executions/{execution}/events?after=0", serve.base);
    let response = raw_request(&url, Some(&serve.token))
        .unwrap_or_else(|error| panic!("the events request did not complete: {error} ({url})"));
    assert_eq!(
        response.status, 200,
        "the events endpoint answered {} rather than 200; an error here used to read back as an empty event list: {}",
        response.status, response.body
    );
    let parsed: Value = serde_json::from_str(&response.body)
        .unwrap_or_else(|error| panic!("the events body is not JSON ({error}): {}", response.body));
    let events = parsed["data"]["events"].as_array().unwrap_or_else(|| {
        panic!(
            "the events response carries no `data.events` ARRAY; absent is not the same as empty, and reading one as the other is what made this failure undiagnosable: {parsed}"
        )
    });
    events
        .iter()
        .map(|event| {
            let kind = event["kind"]["type"].as_str().unwrap_or("?").to_owned();
            if kind == "gate_verdict" {
                format!(
                    "gate_verdict:{}:{}",
                    event["kind"]["data"]["passed"],
                    event["kind"]["data"]["findings"]
                        .as_array()
                        .map_or(0, Vec::len)
                )
            } else {
                kind
            }
        })
        .collect()
}

#[test]
fn an_uncertified_gate_through_the_serve_never_runs_and_never_verdicts() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-gate-http-uncert";
    let serve = gate_serve(directory.path(), execution);

    let (status, reply) = start(&serve, execution, "gate-http-uncert-start");
    assert_eq!(status, 200, "{reply}");
    assert_ne!(
        reply["data"]["status"], "completed",
        "an uncertified gate must not complete the story: {reply}"
    );
    let kinds = event_kinds(&serve, execution);
    assert!(
        !kinds.iter().any(|kind| kind.starts_with("gate_verdict")),
        "no certification, no verdict — over HTTP too: {kinds:?}"
    );
}

#[test]
fn certify_then_resume_runs_the_gate_and_the_verdict_reads_back_over_http() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-gate-http-cert";
    let serve = gate_serve(directory.path(), execution);

    // Phase 1: the uncertified gate refuses to dispatch (the live precondition).
    let (status, reply) = start(&serve, execution, "gate-http-cert-start");
    assert_eq!(status, 200, "{reply}");
    assert_ne!(reply["data"]["status"], "completed");

    // Phase 2: the thymus ritual stamps the receipt onto the SAME stream.
    let output = certify(&serve, "gate-geometry");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("\"gateCertified\":true"),
        "certify must stamp: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Phase 3: pause (resume's precondition — the Task 7 dogfood's own choreography),
    // then resume drives through the SAME serve path start uses — the digest wiring
    // under test — and the certified gate now runs to a verdict.
    let (pause_status, pause_reply) = post_json(
        &format!("{}/v1/executions/{execution}/pause", serve.base),
        &serve.token,
        "gate-http-cert-pause",
        "owner",
        &serde_json::json!({}),
    );
    assert_eq!(pause_status, 200, "{pause_reply}");
    let (resume_status, resume_reply) = post_json(
        &format!("{}/v1/executions/{execution}/resume", serve.base),
        &serve.token,
        "gate-http-cert-resume",
        "owner",
        &serde_json::json!({
            "file": serve.graph.to_str().unwrap(),
            "project": root().to_str().unwrap(),
        }),
    );
    assert_eq!(resume_status, 200, "{resume_reply}");
    assert_eq!(
        resume_reply["data"]["status"], "completed",
        "the certified gate completes the story — and completes FAST: the model route \
         hangs forever, so completion is the no-model-transport proof: {resume_reply}"
    );
    let kinds = event_kinds(&serve, execution);
    assert!(
        kinds.iter().any(|kind| kind == "gate_verdict:true:0"),
        "the passing verdict reads back over HTTP: {kinds:?}"
    );
    assert!(
        kinds.iter().any(|kind| kind == "gate_certified"),
        "the receipt is on the same stream: {kinds:?}"
    );
}

#[test]
fn an_unknown_gate_is_refused_with_the_registry_code() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-gate-http-registry";
    let serve = gate_serve(directory.path(), execution);
    let (status, _) = start(&serve, execution, "gate-http-registry-start");
    assert_eq!(status, 200);

    let output = certify(&serve, "gate-vibes");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("GHCLI018_GATE_INVALID"),
        "the closed registry refuses by code: {stdout}"
    );
    assert!(
        !stdout.contains("\"gateCertified\":true"),
        "nothing was stamped: {stdout}"
    );
}

// ---------------------------------------------------------------------------------------------
// The gate registry is enumerated in exactly ONE place.
// ---------------------------------------------------------------------------------------------

/// `--help` must not enumerate the registry.
///
/// The production change that would make this fail: writing gate ids back into the `--gate` doc
/// comment in `args.rs`.
///
/// **Why the enumeration is removed rather than derived -- corrected after being measured.**
///
/// An earlier version of this comment said the list "cannot be derived because clap renders a
/// compile-time literal". That is true of the DOC COMMENT and FALSE of the arg:
/// `PossibleValuesParser::new(...)` derives it, and adds shell completions. (Found by L.)
///
/// I ran it rather than conceding on argument, and the real reason is the trade it makes.
/// Deriving the values moves rejection into clap, BEFORE this command runs, so the structured
/// `GHCLI018_GATE_INVALID` diagnostic -- with its `path` and `source`, machine-readable --
/// disappears entirely: stdout comes back EMPTY and
/// `an_unknown_gate_is_refused_with_the_registry_code` fails. It also re-enumerates the gates in
/// `--help`: derived, but enumerated.
///
/// So the enumeration stays out because deriving it costs the CLI's structured refusal contract,
/// not because deriving is impossible. Right call, wrong reason -- and a wrong reason stored in a
/// TEST is harder to revisit than one written loose, which is why it is corrected here and not
/// only in the pull request.
#[test]
fn the_help_text_does_not_enumerate_the_gate_registry() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["quality", "certify", "--help"])
        .output()
        .unwrap();
    let help = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    // Positive control: prove the help was actually rendered before concluding anything from an
    // absence. An empty capture would satisfy the assertion below while measuring nothing.
    assert!(
        help.contains("--gate"),
        "HARNESS-BROKE: `certify --help` did not render the --gate flag; got: {help}"
    );
    assert!(
        !help.contains("gate-geometry"),
        "the help text enumerates the registry, so it goes stale the day a gate is added; \
         deriving it via PossibleValuesParser is possible but costs the structured \
         GHCLI018_GATE_INVALID refusal. got: {help}"
    );
}
/// Every gate that actually certifies must be named in the refusal message.
///
/// **What this still covers, now that the registry is ONE array.** Membership is structural: the
/// message maps over the same entries the lookup searches, so *advertised* and *certifiable*
/// cannot diverge -- there is nowhere to write the divergence. What remains testable here is that
/// the message RENDERS those ids at all, which a formatting change could still break.
///
/// **Kept rather than deleted, and the population is the reason.** This iterates a candidate list
/// written in the test, which is a third statement of the registry and can only find gates it
/// already names: a new arm this list does not mention would slip past it. That was a real gap
/// while two lists existed. It is not a gap now, but the test is left in place with its limit
/// stated instead of removed on the strength of an argument -- a check retired because a new
/// invariant "makes it unnecessary" is the shape that goes wrong when the invariant later moves.
#[test]
fn the_refusal_names_every_gate_that_actually_certifies() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-registry-single-source";
    let serve = gate_serve(directory.path(), execution);
    // The stream must exist before certification can stamp onto it -- the precondition every
    // other test in this file establishes. Without it nothing certifies and the loop below is
    // vacuous, which is exactly what the HARNESS-BROKE assert caught.
    let (status, _) = start(&serve, execution, "registry-single-source-start");
    assert_eq!(status, 200);
    let refusal = certify(&serve, "gate-does-not-exist");
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&refusal.stdout),
        String::from_utf8_lossy(&refusal.stderr)
    );

    let mut certifying = Vec::new();
    for id in ["gate-geometry", "gate-retry-lineage"] {
        if certify(&serve, id).status.success() {
            certifying.push(id);
            assert!(
                message.contains(id),
                "{id} certifies but the refusal does not name it; the operator sees only this \
                 message. got: {message}"
            );
        }
    }
    // Without this, a binary where NOTHING certifies would satisfy the loop vacuously.
    assert!(
        !certifying.is_empty(),
        "HARNESS-BROKE: no candidate gate certified at all, so the loop above asserted nothing"
    );
}

/// The registry's SECOND entry certifies through the same command, with its own suite.
///
/// This is what makes the single-source registry load-bearing rather than tidy: until a second
/// entry existed, the refusal message and the check could disagree without anyone noticing.
#[test]
fn the_retry_lineage_gate_certifies_with_its_own_suite() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-retry-lineage-registered";
    let serve = gate_serve(directory.path(), execution);
    let (status, _) = start(&serve, execution, "retry-lineage-registered-start");
    assert_eq!(status, 200);

    let output = certify(&serve, "gate-retry-lineage");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "gate-retry-lineage must certify: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_str(&stdout).expect("certify emits json");
    // The envelope is {ok, command, data, diagnostics}; the payload lives under `data`. Read
    // from the refusal JSON this command already printed, not assumed.
    assert_eq!(body["data"]["gateId"], "gate-retry-lineage");
    assert!(
        body["data"]["specimens"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "a registered gate must carry a NON-EMPTY suite -- certify refuses an empty one, so a \
         zero here would mean the dispatch handed over the wrong suite entirely: {body}"
    );

    // The two gates must not share a suite digest. A registry entry copied from its neighbour and
    // left wired to the neighbour's suite would certify happily and stamp the WRONG immunity --
    // green, plausible, and about a different gate.
    let geometry = certify(&serve, "gate-geometry");
    let geometry_body: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&geometry.stdout)).expect("json");
    assert_ne!(
        body["data"]["suiteDigest"], geometry_body["data"]["suiteDigest"],
        "two gates certified against the same suite means one entry is wired to the other's"
    );
}

/// Every id the refusal message advertises must actually certify.
///
/// The other half of the set equality. `certify_registered`'s `match` needs literal patterns, so
/// the message's list cannot be soldered to the arms in Rust -- it is held equal by OBSERVATION in
/// both directions instead:
///
/// * this test: the advertised list is not WIDER than the arms (a name with no arm is refused,
///   yet the message claims it is registered -- a refusal that contradicts itself);
/// * `the_refusal_names_every_gate_that_actually_certifies`: the arms are not wider than the list.
///
/// **What would make this fail, restated for the single-array registry.** No longer "an id
/// advertised with no adapter": that pairing is structural now, so the mismatch is inexpressible
/// rather than caught. What still fails here is a registry entry whose certification does not
/// SUCCEED -- a gate its own suite fools, or a suite that came back empty.
///
/// The previous wording named `REGISTERED_GATES`, a symbol this branch deleted. A test whose
/// stated failure mode has become IMPOSSIBLE is worse than one merely out of date, because the
/// next reader reasons from it and concludes the test covers something it cannot.
#[test]
fn every_advertised_gate_actually_certifies() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-advertised-certifies";
    let serve = gate_serve(directory.path(), execution);
    let (status, _) = start(&serve, execution, "advertised-certifies-start");
    assert_eq!(status, 200);

    // The advertised list is read from the binary's own refusal, not from a constant the test
    // cannot import: `apps/cli` has no `[lib]`. So this measures what the OPERATOR is told.
    let refusal = certify(&serve, "gate-not-registered-at-all");
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&refusal.stdout),
        String::from_utf8_lossy(&refusal.stderr)
    );
    let advertised: Vec<String> = message
        .split("the registry is closed: ")
        .nth(1)
        .expect("the refusal states the registry")
        .split(['"', ')'])
        .next()
        .expect("the list is delimited")
        .split(", ")
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .collect();

    // Non-emptiness first: an empty list makes the loop below assert nothing.
    assert!(
        !advertised.is_empty(),
        "HARNESS-BROKE: parsed no gate ids out of the refusal; got: {message}"
    );
    for id in &advertised {
        assert!(
            certify(&serve, id).status.success(),
            "the refusal advertises {id} as registered, but it does not certify -- the message \
             promises a gate the dispatch has no arm for"
        );
    }
}

/// #211 C1: the JPD journey-contract gate must be reachable from the operator command.
///
/// Measured before this was written: the gate already certifies INSIDE the pathogens crate --
/// `jpd_gates.rs`'s `journey_contract_gate_rejects_every_specimen_in_its_suite` runs
/// `certify(&JourneyContractGate, &journey_contract_suite())` and is green. So the gap this cell
/// closes is not the gate's discipline; it is that nothing an operator can run reaches it. A gate
/// that cannot be invoked certifies nobody.
///
/// The CLI-facing id is deliberately NOT the gate's own `id()`. `JourneyContractGate::id()` is
/// `gate/jpd-journey-contract`, and that `/` is refused by `GateCertified.gate_id`, typed as
/// `opaqueId`. The registry's doc records the same constraint for the retry-lineage entry; this
/// entry inherits it rather than rediscovering it at runtime.
#[test]
fn the_journey_contract_gate_certifies_through_the_operator_command() {
    let directory = tempfile::tempdir().unwrap();
    let execution = "exec-journey-contract-registered";
    let serve = gate_serve(directory.path(), execution);
    let (status, _) = start(&serve, execution, "journey-contract-registered-start");
    assert_eq!(status, 200);

    let output = certify(&serve, "gate-journey-contract");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "gate-journey-contract must certify through the command: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let body: serde_json::Value = serde_json::from_str(&stdout).expect("certify emits json");
    assert_eq!(body["data"]["gateId"], "gate-journey-contract");
    assert!(
        body["data"]["specimens"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "a registered gate must carry a NON-EMPTY suite -- certify refuses an empty one, so a \
         zero here would mean the dispatch handed over the wrong suite entirely: {body}"
    );

    // The entry must run THIS gate's OWN suite, and that is an identity claim -- so it is checked
    // against the same certification computed in-process, not against a hand-copied constant.
    //
    // The first version of this assertion compared the digest to the two NEIGHBOURS' digests, on
    // the theory that a copied entry stays wired to a sibling's suite. Sabotage refuted it: wiring
    // this entry to `jpd_suite()` -- a THIRD suite -- left the whole harness green at 9/9, because
    // a third digest differs from both neighbours exactly as a correct one does. "Unlike the
    // siblings" is not the property; "its own" is, and only this form says so.
    let expected = pathogens::certify(
        &pathogens::jpd::JourneyContractGate,
        &pathogens::jpd::journey_contract_suite(),
    )
    .expect("the gate certifies against its own suite");
    assert_eq!(
        body["data"]["suiteDigest"], expected.suite_digest,
        "the registry entry is wired to a suite that is not this gate's own"
    );
    assert_eq!(
        body["data"]["specimens"],
        serde_json::json!(expected.specimens),
        "the command reports a different specimen count than this gate's own suite holds"
    );
}

/// The whole choreography for one gate id: certify, pause, resume, read the verdict back.
///
/// Returns the resume reply so a cell can say what actually happened rather than only whether it
/// happened. Mirrors `certify_then_resume_runs_the_gate_and_the_verdict_reads_back_over_http`,
/// which pins this path for gate-geometry.
fn certify_then_resume_for(
    gate_id: &str,
    execution: &str,
) -> (tempfile::TempDir, GateServe, serde_json::Value) {
    certify_then_resume_for_graph(gate_id, gate_id, execution)
}

/// The same choreography with the CERTIFIED gate and the GRAPH's evidence block chosen
/// separately, so a cell can certify `gate-geometry` and still hand the node a contract shaped
/// some other way -- which is what a malformed contract is.
fn certify_then_resume_for_graph(
    gate_id: &str,
    graph_block: &str,
    execution: &str,
) -> (tempfile::TempDir, GateServe, serde_json::Value) {
    let directory = tempfile::tempdir().unwrap();
    let serve = gate_serve_for(directory.path(), execution, graph_block);
    let (status, reply) = start(&serve, execution, &format!("{gate_id}-start"));
    assert_eq!(status, 200, "{reply}");

    let output = certify(&serve, gate_id);
    assert!(
        output.status.success(),
        "ARRANGEMENT: {gate_id} must certify before the run path can be measured: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let (pause_status, pause_reply) = post_json(
        &format!("{}/v1/executions/{execution}/pause", serve.base),
        &serve.token,
        &format!("{gate_id}-pause"),
        "owner",
        &serde_json::json!({}),
    );
    assert_eq!(pause_status, 200, "{pause_reply}");
    let (resume_status, resume_reply) = post_json(
        &format!("{}/v1/executions/{execution}/resume", serve.base),
        &serve.token,
        &format!("{gate_id}-resume"),
        "owner",
        // The graph body resume needs. Omitting it fails EVERY gate identically, which is how the
        // first version of this helper produced three consistent failures that meant nothing.
        &serde_json::json!({
            "file": serve.graph.to_str().unwrap(),
            "project": root().to_str().unwrap(),
        }),
    );
    assert_eq!(resume_status, 200, "{resume_reply}");
    (directory, serve, resume_reply)
}

/// The pinned defect of #668, FLIPPED: a node using the journey-contract gate now runs, and the
/// gate that answers is the one the node named.
///
/// This cell used to assert the ABSENCE of any gate verdict — serve supplied one suite digest
/// (geometry's), the driver compared every gate's receipt against it, and the executor evaluated
/// every dispatched gate as geometry, so this gate was refused as uncertified and never spoke.
/// Its message said the flip would be the fix's receipt. This is that flip.
///
/// **What makes the assertion about DISPATCH rather than about a passing run:** the finding's
/// code. `GHJPD000_WRONG_EVIDENCE_KIND` is produced by `JourneyContractGate` and by nothing else
/// in the registry — geometry emits prose about rendered surfaces and has no such vocabulary. So
/// a refusal carrying that code cannot have come from geometry answering under this gate's name,
/// which is precisely what used to happen.
///
/// The node carries a geometry-shaped surface on purpose (see `gate_check_block`): a journey
/// contract that survives all eighteen checks is its own fixture and is not built here.
#[test]
fn the_journey_contract_gate_runs_and_answers_in_its_own_vocabulary() {
    let (_keep, serve, reply) = certify_then_resume_for("gate-journey-contract", "exec-jc-e2e");
    let verdict = gate_verdict(&serve, "exec-jc-e2e").unwrap_or_else(|| {
        panic!("the journey-contract gate produced no verdict at all: reply={reply}")
    });
    assert_eq!(verdict["gateId"], "gate-journey-contract");
    assert_eq!(
        verdict["passed"], false,
        "the node carries a rendered surface, which this gate does not judge: {verdict}"
    );
    let claims = verdict["findings"]
        .as_array()
        .map(|findings| {
            findings
                .iter()
                .map(|finding| finding["claim"].as_str().unwrap_or("").to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        claims
            .iter()
            .any(|claim| claim.contains("GHJPD000_WRONG_EVIDENCE_KIND")),
        "the refusal must be in THIS gate's vocabulary; geometry cannot produce that code: {claims:?}"
    );
}

/// The same flip on the SECOND registry entry, and it is the stronger half: this gate does not
/// merely answer, it PASSES a real document of the kind it exists to judge.
///
/// The node carries the repository's own positive retry-lineage fixture. Geometry over that
/// document refuses it (there is no rendered surface in it at all), so a clean pass here can only
/// be the retry-lineage evaluator's — the two gates disagree about this evidence, which is what
/// makes the observation discriminating rather than merely present.
#[test]
fn the_retry_lineage_gate_runs_its_own_checks_over_a_lineage_document() {
    let (_keep, serve, reply) = certify_then_resume_for("gate-retry-lineage", "exec-rl-e2e");
    let verdict = gate_verdict(&serve, "exec-rl-e2e").unwrap_or_else(|| {
        panic!("the retry-lineage gate produced no verdict at all: reply={reply}")
    });
    assert_eq!(verdict["gateId"], "gate-retry-lineage");
    assert_eq!(
        verdict["passed"], true,
        "the shipped positive lineage document satisfies all nine declared checks; a refusal here is a broken fixture or another gate answering: {verdict}"
    );
    assert_eq!(
        reply["data"]["status"], "completed",
        "a passing gate lets the execution finish: {reply}"
    );
}

/// CONTROL ON THE HARNESS, not on the registry.
///
/// Two gates failed through `certify_then_resume_for`. That observation is equally consistent with
/// the Codex mechanism and with this helper being broken. gate-geometry completes through the
/// file's original choreography, so running it through THIS helper separates the two causes: it
/// passing here means the helper is sound and the difference lies in the gate.
#[test]
fn the_geometry_gate_runs_to_a_verdict_through_the_same_helper() {
    let (_keep, _serve, reply) = certify_then_resume_for("gate-geometry", "exec-geo-e2e");
    assert_eq!(
        reply["data"]["status"], "completed",
        "CONTROL: the helper itself is broken, so the other two failures say nothing: {reply}"
    );
}

/// The gate verdict event's payload, or `None` when the gate never ran.
///
/// Reads the FINDINGS, not only the event's presence: "a verdict exists" proves dispatch,
/// "the verdict says X" proves WHICH gate dispatched, and #668 was a defect about the second.
fn gate_verdict(serve: &GateServe, execution: &str) -> Option<Value> {
    let url = format!("{}/v1/executions/{execution}/events?after=0", serve.base);
    let response = raw_request(&url, Some(&serve.token)).expect("the events request completes");
    assert_eq!(response.status, 200, "{}", response.body);
    let parsed: Value = serde_json::from_str(&response.body).expect("the events body is JSON");
    parsed["data"]["events"]
        .as_array()?
        .iter()
        .find(|event| event["kind"]["type"] == "gate_verdict")
        .map(|event| event["kind"]["data"].clone())
}

/// A misspelled key in a gate node's contract REFUSES and appends no verdict (#771).
///
/// **The invariant this protects is the append-only store's, not a preference about error
/// shapes.** `GateVerdict` is permanent historical evidence; a failing one claims a delivered
/// surface was examined and refused. This node's surface is coherent and geometry would PASS it
/// -- the only defect is that `manifest` is spelled `manifst` -- so a verdict here would be a
/// permanent High-severity accusation against a surface no gate ever looked at, and fixing the
/// typo could not retract it.
///
/// Before #771 the driver's `deny_unknown_fields` refused this contract as `Unassemblable`.
/// Carrying the evidence unparsed moved the detection into the geometry evaluator; this cell
/// pins that the OUTCOME CLASS moved with it. The gate is certified and the execution reaches
/// the node, so nothing upstream can account for the absence of a verdict.
#[test]
fn a_misspelled_key_in_the_gate_contract_refuses_instead_of_verdicting() {
    let (_keep, serve, reply) =
        certify_then_resume_for_graph("gate-geometry", "gate-geometry-misspelled", "exec-typo-e2e");
    let kinds = event_kinds(&serve, "exec-typo-e2e");
    assert!(
        !kinds.iter().any(|kind| kind.starts_with("gate_verdict")),
        "a contract the gate cannot read must leave no permanent claim: kinds={kinds:?} reply={reply}"
    );
    assert_ne!(
        reply["data"]["status"], "completed",
        "the node was never gated, so the execution cannot have completed through it: {reply}"
    );
}
