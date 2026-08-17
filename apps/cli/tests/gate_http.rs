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

fn get_json(url: &str, token: &str) -> Value {
    let response = raw_request(url, Some(token)).unwrap();
    serde_json::from_str(&response.body).unwrap_or(Value::Null)
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

fn event_kinds(serve: &GateServe, execution: &str) -> Vec<String> {
    let events = get_json(
        &format!("{}/v1/executions/{execution}/events?after=0", serve.base),
        &serve.token,
    );
    events["data"]["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
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
