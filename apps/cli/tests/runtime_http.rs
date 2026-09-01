//! Milestone 05d Task 9 STEP 6: the async driver over HTTP — a fixture story genuinely driven
//! through `drive_to_quiescence_async`/`FixtureAsyncExecutor` (test 1), throughput over N
//! sequential starts (test 4), and the immediate-stop pause/resume-refusal contract against a
//! real (but fake-backed) `direct_api` route (test 3). Harness copied from `api_http.rs` per this
//! task's own instruction — integration test binaries in this workspace do not share code across
//! files (`apps/cli` is bin-only, no `[lib]` target).
//!
//! Test 2 (`an_agent_and_a_tool_node_run_to_completion_with_sealed_evidence`, the full
//! agent+tool+sealed-evidence+double-replay story) closes the milestone's own acceptance
//! sentence: a real reply seals beside the agent outcome, the tool's record and streams seal
//! beside the tool outcome, and the finished stream replays byte-identically twice.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

// -------------------------------------------------------------------------------------------
// Harness (mirrors `api_http.rs`'s own — duplicated per this task's instruction).
// -------------------------------------------------------------------------------------------

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

/// Extra CLI args appended to `serve` beyond `--events`/`--bind` — the real-executor group, when a
/// test needs it. `env` is applied to the spawned process (e.g. `GRAPHHELM_GATEWAY_KEY`).
#[derive(Default)]
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
    let address = started["data"]["address"].as_str().unwrap().to_owned();

    let token = read_token(&token_path(events));
    let base = format!("http://{address}");
    wait_for_health(&base);
    (ServerGuard { child }, base, token)
}

fn serve(events: &Path) -> (ServerGuard, String, String) {
    serve_with(events, &ServeExtra::default())
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

fn post_request(
    url: &str,
    token: &str,
    extra_headers: &[(&str, &str)],
    body: &Value,
    read_timeout: Duration,
) -> std::io::Result<RawResponse> {
    let (host, port, path) = split_url(url);
    let mut stream = TcpStream::connect((host.as_str(), port))?;
    stream.set_read_timeout(Some(read_timeout))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;

    let payload = serde_json::to_vec(body).unwrap();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        payload.len()
    );
    for (name, value) in extra_headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes())?;
    stream.write_all(&payload)?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&String::from_utf8_lossy(&raw))
}

fn post_json(url: &str, token: &str, extra_headers: &[(&str, &str)], body: &Value) -> (u16, Value) {
    let response = post_request(url, token, extra_headers, body, Duration::from_secs(15))
        .unwrap_or_else(|error| panic!("request to {url} failed: {error}"));
    (response.status, json_body(&response))
}

fn json_body(response: &RawResponse) -> Value {
    serde_json::from_str(&response.body)
        .unwrap_or_else(|error| panic!("response body was not JSON ({error}): {:?}", response.body))
}

fn split_url(url: &str) -> (String, u16, String) {
    let rest = url
        .strip_prefix("http://")
        .expect("test URLs are http://host:port/path");
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, "/".to_owned()),
    };
    let (host, port) = authority
        .split_once(':')
        .expect("test URLs always carry a port");
    (host.to_owned(), port.parse().unwrap(), path)
}

fn parse_response(text: &str) -> std::io::Result<RawResponse> {
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| std::io::Error::other("malformed HTTP response: no header/body split"))?;
    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| std::io::Error::other("malformed HTTP response: no status line"))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| std::io::Error::other("malformed HTTP response: no status code"))?;
    Ok(RawResponse {
        status,
        body: body.to_owned(),
    })
}

fn get_json(url: &str, token: Option<&str>) -> Value {
    let response =
        raw_request(url, token).unwrap_or_else(|error| panic!("request to {url} failed: {error}"));
    json_body(&response)
}

// -------------------------------------------------------------------------------------------
// Fixtures shared by every test: a two-node Agent-only graph — every node type classifies as
// Cognitive (`graphhelm_runtime::classify::work_kind`), so `serve::routes::drive_is_viable_for`
// takes the async path even with no real executor configured (test 1's whole point).
// -------------------------------------------------------------------------------------------

fn write_json(directory: &Path, name: &str, value: &Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

/// A minimal two-node Agent-only graph, `step_one -> step_two`, distinct from
/// `examples/graphs/manual-override-deploy.yaml`'s `deploy`-typed graph specifically so it
/// classifies as async-driveable end to end.
fn agent_chain_graph(directory: &Path, execution_id: &str) -> PathBuf {
    let yaml = format!(
        r#"apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_runtime_http_v1
  name: Runtime HTTP fixture story
  executionId: {execution_id}
  version: 1
spec:
  entrypoints:
    - step_one
  nodes:
    step_one:
      type: agent
      name: Step one
      objective: Do the first thing.
      optionality: required
      agent:
        ephemeral:
          purpose: p
          capabilities:
            - change.plan
          inputSchema: schema://TaskRequest@1
          outputSchema: schema://TaskResult@1
          instructions: i
          completionContract:
            requires:
              - result
      completion:
        requires:
          - outputSchemaValid: true
    step_two:
      type: agent
      name: Step two
      objective: Do the second thing.
      optionality: required
      agent:
        ephemeral:
          purpose: p
          capabilities:
            - change.plan
          inputSchema: schema://TaskRequest@1
          outputSchema: schema://TaskResult@1
          instructions: i
          completionContract:
            requires:
              - result
      completion:
        requires:
          - outputSchemaValid: true
  edges:
    - id: one_to_two
      from: step_one
      to: step_two
      type: data
  budgets:
    maxParallelModelCalls: 1
  completion:
    terminalNodes:
      - step_two
"#
    );
    let path = directory.join("graph.yaml");
    std::fs::write(&path, yaml).unwrap();
    path
}

fn all_success_fixtures(directory: &Path) -> PathBuf {
    write_json(
        directory,
        "fixtures.json",
        &serde_json::json!({ "nodeOutcomes": { "step_one": "success", "step_two": "success" } }),
    )
}

// -------------------------------------------------------------------------------------------
// Test 1: a fixture story genuinely completes over the async driver.
// -------------------------------------------------------------------------------------------

#[test]
fn the_fixture_story_drives_async_and_parity_holds() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-runtime-http-fixture";
    let graph = agent_chain_graph(directory.path(), execution);
    let fixtures = all_success_fixtures(directory.path());

    // No real-executor flags: the server stays fixture-only (05a shape) — the async drive here
    // is reached purely because every node in `agent_chain_graph` classifies as Cognitive.
    let (_guard, base, token) = serve(&events);
    let url = format!("{base}/v1/executions/{execution}/start");
    let body = serde_json::json!({
        "file": graph.to_str().unwrap(),
        "fixtures": fixtures.to_str().unwrap(),
        "mode": "autopilot",
    });
    let headers = [
        ("Idempotency-Key", "runtime-http-fixture-1"),
        ("X-GraphHelm-Actor", "agent-runtime-http"),
        ("X-GraphHelm-Actor-Type", "agent"),
    ];

    let (status, reply) = post_json(&url, &token, &headers, &body);
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["data"]["status"], "completed", "{reply}");
    assert_eq!(reply["data"]["nodeStateCounts"]["succeeded"], 2, "{reply}");

    // The 05d driver's own hops (`node_outcome_recorded`) are attributed to the system actor,
    // matching the sync path's split — proof the async driver reused `PreparedDrive`'s scope
    // correctly rather than inventing a new one.
    let events_url = format!("{base}/v1/executions/{execution}/events");
    let tail = get_json(&format!("{events_url}?limit=100"), Some(&token));
    let outcome = tail["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|event| event["kind"]["type"] == "node_outcome_recorded")
        .unwrap_or_else(|| panic!("expected at least one node_outcome_recorded: {tail}"));
    assert_eq!(outcome["actor"]["type"], "system", "{outcome}");
}

// -------------------------------------------------------------------------------------------
// Test 4: throughput over N sequential fixture starts.
// -------------------------------------------------------------------------------------------

#[test]
fn throughput_is_measured_and_printed() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = all_success_fixtures(directory.path());
    let (_guard, base, token) = serve(&events);

    const N: usize = 10;
    let started = Instant::now();
    for index in 0..N {
        let execution = format!("exec-throughput-{index}");
        let graph = agent_chain_graph(directory.path(), &execution);
        let url = format!("{base}/v1/executions/{execution}/start");
        let body = serde_json::json!({
            "file": graph.to_str().unwrap(),
            "fixtures": fixtures.to_str().unwrap(),
            "mode": "autopilot",
        });
        let headers = [
            ("Idempotency-Key", format!("throughput-{index}")),
            ("X-GraphHelm-Actor", "agent-throughput".to_owned()),
            ("X-GraphHelm-Actor-Type", "agent".to_owned()),
        ];
        let header_refs: Vec<(&str, &str)> = headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect();
        let (status, reply) = post_json(&url, &token, &header_refs, &body);
        assert_eq!(status, 200, "{reply}");
        assert_eq!(reply["data"]["status"], "completed", "{reply}");
    }
    let elapsed = started.elapsed();
    let per_second = N as f64 / elapsed.as_secs_f64().max(f64::EPSILON);
    eprintln!(
        "runtime_http throughput: {N} executions in {:.3}s ({:.2} executions/second)",
        elapsed.as_secs_f64(),
        per_second
    );
}

// -------------------------------------------------------------------------------------------
// Test 3: immediate pause interrupts an in-flight node; resume refuses until approved.
// -------------------------------------------------------------------------------------------

/// Accepts exactly one connection, reads the request in full, then holds the socket open
/// forever without ever writing a response — imitating a provider that never answers, so a
/// `ByokAdapter::call` in flight against it blocks until either the route's own timeout or
/// `cancel_all` (never called here — the interruption is driver-level, not transport-level)
/// intervenes.
fn hang_forever_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buffer = [0_u8; 4096];
        // Drain the request headers so the client's own write does not stall on a full pipe,
        // then simply block forever — no response is ever sent.
        let _ = stream.read(&mut buffer);
        std::thread::sleep(Duration::from_secs(600));
    });
    format!("http://{addr}")
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gateway_key() -> String {
    "ab".repeat(32)
}

/// Bootstraps the broker/keyring and stores one credential via the real `graphhelm gateway
/// credential set` subprocess — the same operator flow `gateway_cli.rs` exercises, reused here
/// rather than hand-rolling `SealedKeyProvider`/`CredentialBroker` calls directly.
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
            use std::io::Write as _;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"sk-ant-runtime-http-test-value")?;
            child.wait_with_output()
        })
        .unwrap();
    assert!(
        output.status.success(),
        "gateway credential set failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn immediate_pause_interrupts_an_in_flight_node_and_resume_refuses_until_approve() {
    let deadline = Instant::now() + Duration::from_secs(60);

    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let broker = directory.path().join("broker");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let staging = directory.path().join("staging");
    let key_id = "runtime-http-key";
    let route_id = "hang_route";

    let base_url = hang_forever_server();
    credential_set(&broker, &keyring, key_id, "cred_hang", route_id);

    let manifest = write_json(
        directory.path(),
        "manifest.json",
        &serde_json::json!({
            "manifestVersion": 1,
            "routes": [{
                "id": route_id,
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": base_url,
                "model": "claude-sonnet-5",
                "credentialRef": "cred_hang",
                "profiles": ["critical_reasoning"],
                "enabled": true,
                "timeoutSeconds": 55
            }]
        }),
    );

    let execution = "exec-runtime-http-pause";
    let graph = agent_chain_graph(directory.path(), execution);

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
        // Both keys are needed: `GRAPHHELM_GATEWAY_KEY` for the credential broker
        // (`ServeModelPort::build`), `GRAPHHELM_EVENTS_KEY` for the driver's own evidence sealer
        // (`ports::build_sealer` — reused eagerly by `drive` before any dispatch, since the
        // `--keyring`/`--key-id` group configured here doubles as both).
        env: vec![
            ("GRAPHHELM_GATEWAY_KEY".to_owned(), gateway_key()),
            ("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key()),
        ],
    };
    let (_guard, base, token) = serve_with(&events, &extra);

    // `start` blocks for the whole story (the hanging model call) — run it on its own thread so
    // this test can poll status/pause concurrently, bounded well under `deadline`.
    let start_base = base.clone();
    let start_token = token.clone();
    let project = root();
    let start_handle = std::thread::spawn(move || {
        let url = format!("{start_base}/v1/executions/{execution}/start");
        let body = serde_json::json!({
            "file": graph.to_str().unwrap(),
            "mode": "autopilot",
            "project": project.to_str().unwrap(),
        });
        let headers = [
            ("Idempotency-Key", "runtime-http-pause-start"),
            ("X-GraphHelm-Actor", "agent-runtime-http"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ];
        // Generous read timeout: this request is expected to be interrupted by the pause below
        // long before it would ever complete on its own.
        post_request(&url, &start_token, &headers, &body, Duration::from_secs(50))
    });

    // Wait for the node to actually be dispatched (`running`) before pausing — polling the same
    // status read every mutation replies with.
    let status_url = format!("{base}/v1/executions/{execution}");
    loop {
        if let Ok(response) = raw_request(&status_url, Some(&token))
            && response.status == 200
        {
            let value = json_body(&response);
            if value["data"]["nodeStateCounts"]["running"] == 1 {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the node never reached running before the deadline"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    // Immediate pause: interrupts the in-flight node.
    let pause_url = format!("{base}/v1/executions/{execution}/pause");
    let (pause_status, pause_reply) = post_json(
        &pause_url,
        &token,
        &[
            ("Idempotency-Key", "runtime-http-pause-immediate"),
            ("X-GraphHelm-Actor", "owner-runtime-http"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({ "mode": "immediate" }),
    );
    assert_eq!(pause_status, 200, "{pause_reply}");
    assert_eq!(pause_reply["data"]["status"], "paused", "{pause_reply}");

    // The interrupted `start` request itself now returns — its own drive was cancelled.
    let start_response = start_handle
        .join()
        .unwrap()
        .unwrap_or_else(|error| panic!("the start request itself failed: {error}"));
    let start_reply = json_body(&start_response);
    assert_eq!(start_response.status, 200, "{start_reply}");
    assert_eq!(start_reply["data"]["status"], "paused", "{start_reply}");

    // Resume refuses: the interrupted node is untriaged.
    let resume_url = format!("{base}/v1/executions/{execution}/resume");
    let (refused_status, refused_reply) = post_json(
        &resume_url,
        &token,
        &[
            ("Idempotency-Key", "runtime-http-pause-resume-refused"),
            ("X-GraphHelm-Actor", "owner-runtime-http"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({ "file": agent_chain_graph(directory.path(), execution).to_str().unwrap() }),
    );
    assert_eq!(refused_status, 409, "{refused_reply}");
    assert_eq!(
        refused_reply["diagnostics"][0]["code"],
        "GHCLI005_EXECUTION_STATE"
    );

    // Approve the interrupted node — the refusal lifts (checked via status, per this task's own
    // bounded-time instruction, rather than re-driving into the still-hanging fake provider).
    let approve_url = format!("{base}/v1/executions/{execution}/approve");
    let interrupted_node = {
        let status = get_json(&status_url, Some(&token));
        let interruptions = status["data"]["untriagedInterruptions"].as_array().cloned();
        interruptions
            .and_then(|list| list.first().cloned())
            .and_then(|value| value.as_str().map(str::to_owned))
    };
    let interrupted_node = interrupted_node.unwrap_or_else(|| {
        panic!(
            "expected an untriaged interruption after immediate pause: {}",
            get_json(&status_url, Some(&token))
        )
    });
    let (approve_status, approve_reply) = post_json(
        &approve_url,
        &token,
        &[
            ("Idempotency-Key", "runtime-http-pause-approve"),
            ("X-GraphHelm-Actor", "owner-runtime-http"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({ "node": interrupted_node }),
    );
    assert_eq!(approve_status, 200, "{approve_reply}");
    assert_eq!(
        approve_reply["data"]["untriagedInterruptions"]
            .as_array()
            .map(Vec::len),
        Some(0),
        "the refusal lifts once the interrupted node is approved: {approve_reply}"
    );

    assert!(
        Instant::now() < deadline,
        "immediate_pause_interrupts_an_in_flight_node_and_resume_refuses_until_approve exceeded its 60s budget"
    );
}

// -------------------------------------------------------------------------------------------
// Test 2: the milestone's §8 acceptance sentence as one test — an agent node (fake Anthropic)
// and a tool node (real git in an ephemeral worktree) run to completion over HTTP, every
// outcome carries sealed evidence, and the finished stream replays byte-identically twice.
// -------------------------------------------------------------------------------------------

/// A fake Anthropic that REPLIES: accepts connections in a loop and answers every request with
/// a real Messages-shaped 200.
fn replying_anthropic_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut content_length = 0_usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if let Some(rest) = lower.strip_prefix("content-length:") {
                    content_length = rest.trim().parse().unwrap_or(0);
                }
                if line == "\r\n" {
                    break;
                }
            }
            let mut body = vec![0_u8; content_length];
            let _ = reader.read_exact(&mut body);
            let reply = serde_json::json!({
                "content": [{"type": "text", "text": "the model did the thing"}],
                "usage": {"input_tokens": 12, "output_tokens": 5}
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                reply.len(),
                reply
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://127.0.0.1:{}", address.port())
}

/// A scratch git repository for the tool node's ephemeral worktree (the 05c pattern).
fn scratch_project(directory: &Path) -> PathBuf {
    let project = directory.join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// scratch\n").unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "scratch")
            .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
            .env("GIT_COMMITTER_NAME", "scratch")
            .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "scratch"]);
    project
}

/// Agent → tool chain: the agent speaks to the fake provider; the tool runs `git status` in
/// the ephemeral Tier 1 worktree of the scratch project.
fn agent_tool_graph(directory: &Path, execution_id: &str) -> PathBuf {
    let yaml = format!(
        r#"apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_runtime_http_real_v1
  name: Runtime HTTP real story
  executionId: {execution_id}
  version: 1
spec:
  entrypoints:
    - implement
  nodes:
    implement:
      type: agent
      name: Implement
      objective: Do the real thing.
      optionality: required
      agent:
        ephemeral:
          purpose: p
          capabilities:
            - change.plan
          inputSchema: schema://TaskRequest@1
          outputSchema: schema://TaskResult@1
          instructions: i
          completionContract:
            requires:
              - result
      completion:
        requires:
          - outputSchemaValid: true
    verify:
      type: tool
      name: Verify
      objective: Run the check.
      optionality: required
      input:
        schema: schema://TaskResult@1
      output:
        schema: schema://TestReport@1
      tool:
        call:
          tool: shell
          program: git
          arguments:
            - status
      completion:
        requires:
          - expression: output.executed > 0
  edges:
    - id: implement_to_verify
      from: implement
      to: verify
      type: data
  budgets:
    maxParallelModelCalls: 1
  completion:
    terminalNodes:
      - verify
"#
    );
    let path = directory.join("real-graph.yaml");
    std::fs::write(&path, yaml).unwrap();
    path
}

#[test]
fn an_agent_and_a_tool_node_run_to_completion_with_sealed_evidence() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let broker = directory.path().join("broker");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let staging = directory.path().join("staging");
    let key_id = "runtime-http-real-key";
    let route_id = "real_route";
    let project = scratch_project(directory.path());

    let base_url = replying_anthropic_server();
    credential_set(&broker, &keyring, key_id, "cred_real", route_id);

    let manifest = write_json(
        directory.path(),
        "real-manifest.json",
        &serde_json::json!({
            "manifestVersion": 1,
            "routes": [{
                "id": route_id,
                "provider": "anthropic",
                "transport": "direct_api",
                "authentication": "api_key",
                "billingMode": "per_token",
                "baseUrl": base_url,
                "model": "claude-sonnet-5",
                "credentialRef": "cred_real",
                "profiles": ["critical_reasoning"],
                "enabled": true,
                "timeoutSeconds": 30
            }]
        }),
    );

    let execution = "exec-runtime-http-real";
    let graph = agent_tool_graph(directory.path(), execution);

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
    let (_guard, base, token) = serve_with(&events, &extra);

    let (status_code, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "real-start"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "mode": "autopilot",
            "project": project.to_str().unwrap(),
        }),
    );
    assert_eq!(status_code, 200, "{reply}");
    assert_eq!(
        reply["data"]["status"], "completed",
        "the real story must complete: {reply}"
    );
    assert_eq!(reply["data"]["nodeStateCounts"]["succeeded"], 2, "{reply}");

    // Every real outcome carries sealed evidence, readable from the store's own tail.
    let events_reply = get_json(
        &format!("{base}/v1/executions/{execution}/events?limit=1000"),
        Some(&token),
    );
    let entries = events_reply["data"]["events"].as_array().unwrap();
    let outcome_refs = |node: &str| -> usize {
        entries
            .iter()
            .filter_map(|entry| {
                let kind = &entry["kind"];
                (kind["type"] == "node_outcome_recorded"
                    && kind["data"]["nodeId"] == node
                    && kind["data"]["outcome"] == "succeeded")
                    .then(|| entry["evidenceRefs"].as_array().map_or(0, Vec::len))
            })
            .next()
            .unwrap_or(0)
    };
    assert!(
        outcome_refs("implement") >= 1,
        "the agent reply must seal: {entries:?}"
    );
    assert!(
        outcome_refs("verify") >= 3,
        "the tool record and streams must seal: {entries:?}"
    );

    // Byte-identical double replay of the finished stream — the §8 clause verbatim.
    let replay = |_: ()| {
        let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["graph", "replay", "--events", events.to_str().unwrap()])
            .output()
            .unwrap();
        assert!(output.status.success());
        output.stdout
    };
    assert_eq!(replay(()), replay(()), "replay must be byte-identical");
}
