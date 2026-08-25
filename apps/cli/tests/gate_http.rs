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

struct RawResponse {
    status: u16,
    body: String,
}

fn raw_request(url: &str, token: Option<&str>) -> std::io::Result<RawResponse> {
    let (host, port, path) = split_url(url);
    let mut stream = TcpStream::connect((host.as_str(), port))?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let mut request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(token) = token {
        request.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&String::from_utf8_lossy(&raw))
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

fn split_url(url: &str) -> (String, u16, String) {
    let rest = url.strip_prefix("http://").expect("http:// urls");
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    let (host, port) = authority.split_once(':').expect("a port");
    (host.to_owned(), port.parse().unwrap(), path)
}

fn parse_response(text: &str) -> std::io::Result<RawResponse> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("no header/body split"))?;
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| std::io::Error::other("no status code"))?;
    Ok(RawResponse {
        status,
        body: body.to_owned(),
    })
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

/// A single-gate graph whose contract carries a coherent delivered surface — the same
/// passing subject shape the Task 7 dogfood ran.
fn gate_graph(directory: &Path, execution: &str) -> PathBuf {
    let gate_block = serde_json::json!({"check": {
        "gateId": "gate-geometry",
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
    }});
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
        ],
        env: vec![
            ("GRAPHHELM_GATEWAY_KEY".to_owned(), gateway_key()),
            ("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key()),
        ],
    };
    let (guard, base, token) = serve_with(&events, &extra);
    let graph = gate_graph(directory, execution);
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
/// **Why the help text is the worst place to list them.** It is what the operator reads FIRST,
/// before running anything; it lives in a DIFFERENT FILE from the check that decides membership;
/// and clap renders it from a doc comment, which is a compile-time literal that cannot be derived
/// from the registry. So it cannot be kept in sync by construction — only by memory. Removing the
/// enumeration removes the need for the sync, which is the version that does not age.
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
        "the help text enumerates the registry, so it goes stale the day a gate is added and \
         cannot be derived from the registry because clap renders a compile-time literal. \
         got: {help}"
    );
}

/// Every gate that ACTUALLY certifies must be named in the refusal message.
///
/// The population is discovered by driving the binary rather than read from a constant, because
/// `apps/cli` has no `[lib]` and a test cannot import one. That is the better guard anyway: it
/// checks what the operator sees.
///
/// The production change that would make this fail: adding a registry entry while leaving the
/// refusal message's own literal list alone.
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
/// The production change that would make this fail: adding an id to `REGISTERED_GATES` without
/// adding its arm to `certify_registered`.
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
