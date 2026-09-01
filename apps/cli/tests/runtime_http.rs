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
/// A base URL nothing listens on: bound to learn a free port, then dropped so a connection to it
/// is refused immediately rather than hanging. Used as the DEPLOYER'S DEFAULT route in the tests
/// below, so "the request's route was ignored" and "the request's route was honoured" produce
/// visibly different outcomes instead of two indistinguishable successes.
fn unreachable_base_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    format!("http://127.0.0.1:{port}")
}

/// The two-route wiring the route-selection tests share: `dead_route` is what `--route` names (and
/// it cannot answer), `live_route` points at a provider that replies. Returns the serve arguments
/// and the scratch project path.
fn two_route_wiring(directory: &Path, live_base_url: &str) -> (ServeExtra, PathBuf) {
    let broker = directory.join("broker");
    let keyring = directory.join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let staging = directory.join("staging");
    let key_id = "runtime-http-two-route-key";

    credential_set(&broker, &keyring, key_id, "cred_dead", "dead_route");
    credential_set(&broker, &keyring, key_id, "cred_live", "live_route");

    let route = |id: &str, credential: &str, base: &str| {
        serde_json::json!({
            "id": id,
            "provider": "anthropic",
            "transport": "direct_api",
            "authentication": "api_key",
            "billingMode": "per_token",
            "baseUrl": base,
            "model": "claude-sonnet-5",
            "credentialRef": credential,
            "profiles": ["critical_reasoning"],
            "enabled": true,
            "timeoutSeconds": 30
        })
    };
    let manifest = write_json(
        directory,
        "two-route-manifest.json",
        &serde_json::json!({
            "manifestVersion": 1,
            "routes": [
                route("dead_route", "cred_dead", &unreachable_base_url()),
                route("live_route", "cred_live", live_base_url),
            ]
        }),
    );

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
            // The DEPLOYER'S default is the route that cannot answer. Every test below that
            // reaches a provider therefore proves the request's own choice was used.
            "--route".into(),
            "dead_route".into(),
            "--staging".into(),
            staging.to_str().unwrap().into(),
        ],
        env: vec![
            ("GRAPHHELM_GATEWAY_KEY".to_owned(), gateway_key()),
            ("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key()),
        ],
    };
    (extra, scratch_project(directory))
}

/// A start request naming a route in the manifest RUNS ON THAT ROUTE, not on `--route`.
///
/// The discriminator is the deployer's default pointing at a dead port: before `prepare_drive`
/// resolved the request's `"route"`, this drive built its port from `wiring.route`, the agent node
/// could not reach a provider, and the execution did not complete. There is no way to read the
/// reply text back today (evidence is sealed), so REACHABILITY is what separates the two routes -
/// a discriminator that survives the fact that both routes would otherwise answer identically.
#[test]
fn a_start_request_runs_on_the_route_it_names_not_the_servers_default() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let live = replying_anthropic_server();
    let (extra, project) = two_route_wiring(directory.path(), &live);

    let execution = "exec-route-selected";
    let graph = agent_tool_graph(directory.path(), execution);
    let (_guard, base, token) = serve_with(&events, &extra);

    let (status_code, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "route-selected"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "mode": "autopilot",
            "project": project.to_str().unwrap(),
            "route": "live_route",
        }),
    );

    assert_eq!(status_code, 200, "{reply}");
    assert_eq!(
        reply["data"]["status"], "completed",
        "the named route must be the one that ran: {reply}"
    );
    assert_eq!(reply["data"]["nodeStateCounts"]["succeeded"], 2, "{reply}");
}

/// A `"route"` naming nothing in the manifest is REFUSED, and the refusal points at the field.
///
/// Without this the unknown id falls through to the deployer's default and the operator is told
/// the run succeeded on a model they did not ask for.
#[test]
fn a_start_request_naming_an_unknown_route_is_refused_at_the_field() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let live = replying_anthropic_server();
    let (extra, project) = two_route_wiring(directory.path(), &live);

    let execution = "exec-route-unknown";
    let graph = agent_tool_graph(directory.path(), execution);
    let (_guard, base, token) = serve_with(&events, &extra);

    let (status_code, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "route-unknown"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "mode": "autopilot",
            "project": project.to_str().unwrap(),
            "route": "no_such_route",
        }),
    );

    assert_eq!(status_code, 400, "{reply}");
    assert_eq!(reply["ok"], false, "{reply}");
    let diagnostic = &reply["diagnostics"][0];
    assert_eq!(diagnostic["code"], "GHCLI001_ARGUMENT_INVALID", "{reply}");
    assert_eq!(
        diagnostic["path"], "/route",
        "the refusal must point at the field the caller got wrong: {reply}"
    );
}

/// A `"route"` that is PRESENT but not a string is refused, never silently ignored.
///
/// THIS IS THE GUARD FOR A SPELLING, and the spelling is the natural one. Reading the field as
/// `payload.get("route").and_then(Value::as_str)` folds "absent" and "present but the wrong type"
/// into one `None`, and the `None` branch runs the deployer's default. A caller who sent
/// `"route": 7` - a typo, a form that posted a number, a client that serialized an enum wrong -
/// would then be told their run succeeded, on a model they did not choose, with nothing anywhere
/// recording that their choice was discarded. The two-route wiring makes that failure observable:
/// the ignored path reaches the dead default, so the mistyped request would come back as a drive
/// outcome rather than as this refusal.
#[test]
fn a_route_that_is_not_a_string_is_refused_rather_than_quietly_ignored() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let live = replying_anthropic_server();
    let (extra, project) = two_route_wiring(directory.path(), &live);

    let execution = "exec-route-mistyped";
    let graph = agent_tool_graph(directory.path(), execution);
    let (_guard, base, token) = serve_with(&events, &extra);

    let (status_code, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "route-mistyped"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "mode": "autopilot",
            "project": project.to_str().unwrap(),
            "route": 7,
        }),
    );

    assert_eq!(status_code, 400, "{reply}");
    assert_eq!(
        reply["diagnostics"][0]["path"], "/route",
        "a mistyped route must be refused at the field, not folded into the default: {reply}"
    );
}
/// THE MODEL'S OWN WORDS COME BACK OUT.
///
/// This is the end of the loop the sealing opened: `replying_anthropic_server` answers with the
/// literal text below, the executor seals that reply into encrypted Evidence and records only a
/// reference and a token count, and until now nothing could read it again. The assertion is on the
/// PROVIDER'S text, not on a length or a digest, because a digest would pass just as happily on
/// ciphertext and a length would pass on any string of the same size.
///
/// It also proves the two halves are actually joined: the `LocalEventRepository` half (which had
/// no `EvidenceRepository` impl at all, only PostgreSQL did) and the opener half (which existed
/// and was reachable only from the Governor).
#[test]
fn a_sealed_model_reply_can_be_read_back_as_the_text_the_provider_sent() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let broker = directory.path().join("broker");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let staging = directory.path().join("staging");
    let key_id = "runtime-http-evidence-key";
    let route_id = "evidence_route";
    let project = scratch_project(directory.path());

    let base_url = replying_anthropic_server();
    credential_set(&broker, &keyring, key_id, "cred_evidence", route_id);
    let manifest = write_json(
        directory.path(),
        "evidence-manifest.json",
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
                "credentialRef": "cred_evidence",
                "profiles": ["critical_reasoning"],
                "enabled": true,
                "timeoutSeconds": 30
            }]
        }),
    );

    let execution = "exec-runtime-http-evidence";
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
            ("Idempotency-Key", "evidence-start"),
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
    assert_eq!(reply["data"]["status"], "completed", "{reply}");

    // The agent node's outcome carries the reference to its sealed reply. Found by walking the
    // event stream the way any reader would, rather than by reconstructing the id from the naming
    // convention: a test that rebuilds the id would keep passing if the recorded reference and the
    // stored blob ever stopped agreeing, which is one of the things this route must not do.
    let events_reply = get_json(
        &format!("{base}/v1/executions/{execution}/events?limit=1000"),
        Some(&token),
    );
    let entries = events_reply["data"]["events"].as_array().unwrap();
    let reference = entries
        .iter()
        .filter(|entry| {
            entry["kind"]["type"] == "node_outcome_recorded"
                && entry["kind"]["data"]["nodeId"] == "implement"
        })
        .filter_map(|entry| entry["evidenceRefs"].as_array())
        .flatten()
        .find(|reference| {
            reference["evidenceId"]
                .as_str()
                .is_some_and(|id| id.ends_with("reply"))
        })
        .unwrap_or_else(|| {
            panic!("the agent outcome must reference its sealed reply: {entries:?}")
        });
    let evidence_id = reference["evidenceId"].as_str().unwrap();

    let opened = get_json(
        &format!("{base}/v1/executions/{execution}/evidence/{evidence_id}"),
        Some(&token),
    );
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(opened["data"]["mediaType"], "application/json", "{opened}");
    // Named in the reply so a caller knows the class of what it is holding, not left to be
    // inferred from where the id came from.
    assert_eq!(opened["data"]["sensitivity"], "confidential", "{opened}");
    let content = opened["data"]["content"].as_str().unwrap();
    assert!(
        content.contains("the model did the thing"),
        "the provider's own text must survive sealing and come back: {opened}"
    );
    // AN EVIDENCE ID THIS EXECUTION NEVER RECORDED IS REFUSED.
    //
    // This is what the store's `evidence_exists` gate is for, and the id has to be one whose
    // VALIDITY is not in question: a made-up id gets refused by the key encoder before the gate is
    // ever consulted, so a test built on one passes whether the gate is there or not. That test
    // existed here and was deleted - it survived a sabotage that removed the gate entirely.
    //
    // So the id is the one the assertion above just opened successfully, with a single character
    // changed. Same shape, same length, same charset - it parses and encodes exactly as the real
    // one did, three lines up, provably - and no recorded event references it. The only thing
    // between this request and an answer is the gate.
    //
    // AN EARLIER VERSION STARTED A SECOND EXECUTION to borrow its scope, and that second execution
    // was a second writer against the same store: measured at 4 passes in 5 runs ALONE, failing
    // with `GHE008_STORAGE_FAILURE` from the store's own locking rather than from anything this
    // test is about. A guard that is right four times out of five is not a guard, and the fix is
    // to stop writing, not to retry until it agrees.
    let unrecorded = flip_last_character(evidence_id);
    assert_ne!(
        unrecorded, evidence_id,
        "the probe id must differ from the real one"
    );

    let refused = raw_request(
        &format!("{base}/v1/executions/{execution}/evidence/{unrecorded}"),
        Some(&token),
    )
    .unwrap();
    assert_eq!(
        refused.status, 409,
        "evidence no event references must be refused by the gate, not attempted: {}",
        refused.body
    );
    let refused_reply: Value = serde_json::from_str(&refused.body).unwrap();
    assert_eq!(
        refused_reply["diagnostics"][0]["code"], "GHCLI023_EVIDENCE_UNREADABLE",
        "{refused_reply}"
    );
}

/// One character of an evidence id changed, keeping its shape. Used to build an id that is
/// unquestionably well-formed - it is a real id with a digit moved - and that nothing recorded.
fn flip_last_character(id: &str) -> String {
    let mut characters: Vec<char> = id.chars().collect();
    let last = characters.len() - 1;
    characters[last] = match characters[last] {
        'a' => 'b',
        'z' => 'y',
        '0' => '1',
        '9' => '8',
        other if other.is_ascii_digit() => '0',
        _ => 'a',
    };
    characters.into_iter().collect()
}
/// Creates the sealing key through the real `graphhelm gateway keyring init`, with no broker and no
/// credential anywhere — the setup an operator who wants the message path and no model performs.
fn keyring_init(keyring: &Path, key_id: &str) {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "gateway",
            "keyring",
            "init",
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            key_id,
        ])
        .env("GRAPHHELM_EVENTS_KEY", gateway_key())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "`gateway keyring init` failed: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// THE SETUP THE MESSAGE PATH ACTUALLY NEEDS, with nothing else in it.
///
/// Sealing evidence needs a key. Until `gateway keyring init` existed, the only way to put one in a
/// keyring was `gateway credential set`, which stores a BYOK model credential and reads a secret
/// from stdin — so an operator who wanted to send a message and had no model to wire had to invent
/// an API key to get past the setup. Worse, nothing said so: `serve` starts happily against an
/// empty keyring directory and the first message comes back refused with `the sealed keyring could
/// not be opened` (measured 2026-08-28).
///
/// This drives the whole loop with NO broker and NO credential: create the key, start, say
/// something with no path on the host, and read the words back out.
#[test]
fn a_runtime_can_seal_a_message_with_a_keyring_and_no_credential_at_all() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let key_id = "runtime-http-keyring-only";
    keyring_init(&keyring, key_id);

    let extra = ServeExtra {
        args: vec![
            "--keyring".into(),
            keyring.to_str().unwrap().into(),
            "--key-id".into(),
            key_id.into(),
        ],
        env: vec![("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key())],
    };
    let (_guard, base, token) = serve_with(&events, &extra);

    let execution = "exec-keyring-only";
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = all_success_fixtures(directory.path());
    let (started, start_reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "keyring-only-start"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "fixtures": fixtures.to_str().unwrap(),
            "mode": "supervised",
        }),
    );
    assert_eq!(started, 200, "{start_reply}");

    let note = "no broker, no credential, and this still has to arrive";
    let (status, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/signal"),
        &token,
        &[
            ("Idempotency-Key", "keyring-only-signal"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "signal": {
                "id": "signal-keyring-only",
                "source": {"type": "user", "id": "studio-operator"},
                "type": "operator_note",
                "severity": "low",
                "description": note,
                "evidence": [execution],
                "emittedAt": "2026-08-28T00:00:00Z"
            }
        }),
    );
    assert_eq!(
        status, 200,
        "a keyring alone must be enough to record: {reply}"
    );

    let events_reply = get_json(
        &format!("{base}/v1/executions/{execution}/events?limit=1000"),
        Some(&token),
    );
    let entries = events_reply["data"]["events"].as_array().unwrap();
    let reference = entries
        .iter()
        .filter(|entry| entry["kind"]["type"] == "signal_recorded")
        .filter_map(|entry| entry["evidenceRefs"].as_array())
        .flatten()
        .next()
        .unwrap_or_else(|| {
            panic!("the recorded message must reference its sealed envelope: {entries:?}")
        });

    let opened = get_json(
        &format!(
            "{base}/v1/executions/{execution}/evidence/{}",
            reference["evidenceId"].as_str().unwrap()
        ),
        Some(&token),
    );
    assert!(
        opened["data"]["content"]
            .as_str()
            .unwrap_or_default()
            .contains(note),
        "the words must come back out of a credential-free Runtime: {opened}"
    );
}

/// A second `init` is refused rather than silently rotating the key.
///
/// Overwriting would orphan everything already sealed under the old one: the events keep their
/// references and nothing can open the content again, and the loss is INVISIBLE, because an
/// unopenable reference looks the same as one whose Runtime simply lacks the key. A repeated setup
/// step is a far likelier reason for a second `init` than a deliberate rotation.
#[test]
fn initialising_a_key_twice_is_refused_rather_than_replacing_it() {
    let directory = tempfile::tempdir().unwrap();
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    keyring_init(&keyring, "twice");

    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "gateway",
            "keyring",
            "init",
            "--keyring",
            keyring.to_str().unwrap(),
            "--key-id",
            "twice",
        ])
        .env("GRAPHHELM_EVENTS_KEY", gateway_key())
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "a second init must be refused: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "the refusal must be a readable envelope ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    });
    assert_eq!(reply["ok"], false);
    let message = reply["diagnostics"][0]["message"]
        .as_str()
        .unwrap_or_default();
    assert!(
        message.contains("already holds that key"),
        "the refusal must say why, not just fail: {reply}"
    );
}

/// THE CALL SHAPE THAT PREDATES `evidenceOut` BEING OPTIONAL — a sealed Runtime, a signal that
/// DOES name a path. It is here as a control, and it dates the defect it covers.
///
/// Making `evidenceOut` optional did not break sealed signals over HTTP; sealed signals over HTTP
/// had never once been exercised. Every earlier signal test drove an UNSEALED server, where
/// `sealing` is `None` and the seal never runs, or drove the CLI, where building a runtime is
/// correct because there is no reactor to collide with. So the panic at `signal.rs:262` sat
/// behind `--keyring` from Milestone 05d Task 9 onwards, reachable by any operator who started
/// the Runtime the documented way, and no test could see it.
///
/// Without this test the fix would look like a repair to my own regression. With it, the two
/// tests fail together before the fix and pass together after — which is what says the defect was
/// already shipped, and says it in a form someone can re-run rather than take on my word.
#[test]
fn a_sealed_runtime_records_a_signal_that_also_names_a_path() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let broker = directory.path().join("broker");
    let key_id = "runtime-http-signal-path-key";
    credential_set(
        &broker,
        &keyring,
        key_id,
        "cred_signal_path",
        "unused_route",
    );

    let extra = ServeExtra {
        args: vec![
            "--keyring".into(),
            keyring.to_str().unwrap().into(),
            "--key-id".into(),
            key_id.into(),
        ],
        env: vec![
            ("GRAPHHELM_GATEWAY_KEY".to_owned(), gateway_key()),
            ("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key()),
        ],
    };
    let (_guard, base, token) = serve_with(&events, &extra);

    let execution = "exec-signal-with-path";
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = all_success_fixtures(directory.path());
    let (started, start_reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "signal-path-start"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "fixtures": fixtures.to_str().unwrap(),
            "mode": "supervised",
        }),
    );
    assert_eq!(started, 200, "{start_reply}");

    let evidence_out = directory.path().join("signal-envelope.json");
    let (status, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/signal"),
        &token,
        &[
            ("Idempotency-Key", "signal-with-path"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "signal": {
                "id": "signal-with-a-path",
                "source": {"type": "node", "id": "implementation"},
                "type": "risk_identified",
                "severity": "high",
                "description": "the path and the seal are both satisfied here",
                "evidence": [execution],
                "emittedAt": "2026-08-28T00:00:00Z"
            },
            "evidenceOut": evidence_out.to_str().unwrap(),
        }),
    );
    assert_eq!(
        status, 200,
        "a sealed Runtime with a path must record: {reply}"
    );
    assert!(
        evidence_out.exists(),
        "the operator-supplied path is still written when the Runtime also seals"
    );
}

/// THE LOOP A BROWSER NEEDS, END TO END.
///
/// A message from the Studio is a signal, and a signal's envelope has to be durable BEFORE the
/// event exists - that rule never relaxes. What changed is which copy satisfies it: with a keyring
/// the envelope seals into the Evidence store before the append, so the operator-supplied file is
/// a second copy of something already safe. A browser has no path on the Runtime's host, and until
/// this it could not send a message at all.
///
/// The assertion is on the TEXT coming back out, not on the 200: recording something unreadable
/// would satisfy a status check and defeat the entire point, which is that the person on the other
/// end can read what was said.
#[test]
fn a_signal_needs_no_path_when_the_runtime_can_seal_it_and_reads_back() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let broker = directory.path().join("broker");
    let key_id = "runtime-http-signal-key";
    // Initialises the keyring the same way every other sealed test here does.
    credential_set(&broker, &keyring, key_id, "cred_signal", "unused_route");

    let extra = ServeExtra {
        args: vec![
            "--keyring".into(),
            keyring.to_str().unwrap().into(),
            "--key-id".into(),
            key_id.into(),
        ],
        env: vec![
            ("GRAPHHELM_GATEWAY_KEY".to_owned(), gateway_key()),
            ("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key()),
        ],
    };
    let (_guard, base, token) = serve_with(&events, &extra);

    let execution = "exec-signal-no-path";
    let graph = root().join("examples/graphs/manual-override-deploy.yaml");
    let fixtures = all_success_fixtures(directory.path());
    let (started, start_reply) = post_json(
        &format!("{base}/v1/executions/{execution}/start"),
        &token,
        &[
            ("Idempotency-Key", "signal-start"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "fixtures": fixtures.to_str().unwrap(),
            "mode": "supervised",
        }),
    );
    assert_eq!(started, 200, "{start_reply}");

    // NO `evidenceOut`. This is the request a browser can actually make.
    let note = "the fixture is missing, not flaky - I am looking at it";
    let (status, reply) = post_json(
        &format!("{base}/v1/executions/{execution}/signal"),
        &token,
        &[
            ("Idempotency-Key", "signal-no-path"),
            ("X-GraphHelm-Actor", "owner-local"),
            ("X-GraphHelm-Actor-Type", "owner"),
        ],
        &serde_json::json!({
            "signal": {
                "id": "signal-from-the-studio",
                "source": {"type": "node", "id": "implementation"},
                "type": "risk_identified",
                "severity": "high",
                "description": note,
                "evidence": [execution],
                "emittedAt": "2026-08-28T00:00:00Z"
            }
        }),
    );
    assert_eq!(status, 200, "a sealed Runtime needs no path: {reply}");

    // The envelope sealed, and the message is readable by whoever is watching.
    let events_reply = get_json(
        &format!("{base}/v1/executions/{execution}/events?limit=1000"),
        Some(&token),
    );
    let entries = events_reply["data"]["events"].as_array().unwrap();
    let reference = entries
        .iter()
        .filter(|entry| entry["kind"]["type"] == "signal_recorded")
        .filter_map(|entry| entry["evidenceRefs"].as_array())
        .flatten()
        .next()
        .unwrap_or_else(|| {
            panic!("the recorded signal must reference its sealed envelope: {entries:?}")
        });

    let opened = get_json(
        &format!(
            "{base}/v1/executions/{execution}/evidence/{}",
            reference["evidenceId"].as_str().unwrap()
        ),
        Some(&token),
    );
    assert!(
        opened["data"]["content"]
            .as_str()
            .unwrap_or_default()
            .contains(note),
        "the words sent from the Studio must come back out: {opened}"
    );
}
