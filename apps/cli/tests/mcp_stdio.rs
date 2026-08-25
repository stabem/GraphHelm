//! Black-box conformance for `graphhelm mcp` — the chat surface's stdio MCP server. The
//! harness spawns the real binary with piped stdio, writes newline-delimited JSON-RPC
//! request lines, closes stdin, and reads every protocol reply back (the CLI's final
//! `CommandOutput` envelope carries no `"jsonrpc"` member and is filtered out — stdout is
//! shared between the protocol stream and the house CLI contract's closing envelope).

use std::time::{Duration, Instant};

/// One whole session: feed `lines` to `graphhelm mcp`, close stdin, wait for exit, return
/// the protocol replies (in order) plus the process output for exit/stderr assertions.
struct McpSession {
    replies: Vec<serde_json::Value>,
    output: std::process::Output,
    elapsed: Duration,
}

/// Task 3: the server now refuses to start without config (loopback URL + token + actor),
/// so every session (the lifecycle tests included) runs under a valid default: a loopback
/// URL nothing listens on (the lifecycle needs no API) and the token via env, never argv.
fn mcp_session(lines: &[serde_json::Value]) -> McpSession {
    mcp_session_with(
        &["--url", "http://127.0.0.1:9", "--actor", "agent-chat"],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        lines,
    )
}

fn mcp_session_with(
    args: &[&str],
    env: &[(&str, &str)],
    lines: &[serde_json::Value],
) -> McpSession {
    let mut input = String::new();
    for line in lines {
        input.push_str(&line.to_string());
        input.push('\n');
    }
    let started = Instant::now();
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.arg("mcp").args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command
        .write_stdin(input)
        .timeout(Duration::from_secs(30))
        .output()
        .expect("the mcp server runs to EOF");
    let elapsed = started.elapsed();
    let replies = String::from_utf8(output.stdout.clone())
        .expect("stdout is UTF-8")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value.get("jsonrpc").is_some())
        .collect();
    McpSession {
        replies,
        output,
        elapsed,
    }
}

fn initialize_request(id: u64, protocol_version: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": protocol_version,
            "capabilities": {},
            "clientInfo": {"name": "conformance", "version": "0"}
        }
    })
}

fn initialized_notification() -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"})
}

#[test]
fn initialize_negotiates_and_reports_tools_capability() {
    let session = mcp_session(&[initialize_request(1, "2025-06-18")]);
    assert_eq!(session.replies.len(), 1, "{:?}", session.replies);
    let result = &session.replies[0]["result"];
    assert_eq!(result["protocolVersion"], "2025-06-18");
    assert!(
        result["capabilities"].get("tools").is_some(),
        "the tools capability must be declared: {result}"
    );
    assert_eq!(result["serverInfo"]["name"], "graphhelm");

    // A client asking for a DIFFERENT revision gets our version back, never an error —
    // the spec's rule: the server answers with what it supports; the client decides.
    let session = mcp_session(&[initialize_request(1, "2024-11-05")]);
    assert!(
        session.replies[0].get("error").is_none(),
        "a version mismatch is not an error: {:?}",
        session.replies[0]
    );
    assert_eq!(
        session.replies[0]["result"]["protocolVersion"],
        "2025-06-18"
    );
}

#[test]
fn tools_are_refused_before_initialize_and_work_after_initialized() {
    // Before initialize: the MCP lifecycle rule — a JSON-RPC error naming the lifecycle.
    let session =
        mcp_session(&[serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})]);
    let refusal = &session.replies[0];
    assert!(
        refusal.get("error").is_some(),
        "tools/list before initialize must be refused: {refusal}"
    );
    assert!(
        refusal["error"]["message"]
            .as_str()
            .unwrap()
            .contains("initialize"),
        "the refusal names the lifecycle: {refusal}"
    );

    // After initialize + notifications/initialized: the tool array.
    let session = mcp_session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ]);
    assert_eq!(session.replies.len(), 2, "{:?}", session.replies);
    assert_eq!(session.replies[1]["id"], serde_json::json!(2));
    assert!(
        session.replies[1]["result"]["tools"].is_array(),
        "tools/list answers the tool array: {:?}",
        session.replies[1]
    );
}

#[test]
fn ping_pongs_and_eof_exits_cleanly() {
    // Ping is answerable before initialize (the one request the lifecycle rule exempts).
    let session =
        mcp_session(&[serde_json::json!({"jsonrpc": "2.0", "id": "p1", "method": "ping"})]);
    assert_eq!(session.replies.len(), 1, "{:?}", session.replies);
    assert_eq!(session.replies[0]["id"], serde_json::json!("p1"));
    assert!(session.replies[0].get("result").is_some());

    // EOF exits cleanly and promptly: status 0, well under the bound, no panic output.
    assert!(session.output.status.success(), "{:?}", session.output);
    assert!(
        session.elapsed < Duration::from_secs(5),
        "EOF must exit promptly, took {:?}",
        session.elapsed
    );
    let stderr = String::from_utf8_lossy(&session.output.stderr);
    assert!(!stderr.contains("panicked"), "no panic output: {stderr}");
}

// ---------------------------------------------------------------------------------------------
// Task 3: config, auth, and the loopback-only API client.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_non_loopback_url_is_refused_fail_closed_including_userinfo_shapes() {
    // The #36 blocker's shapes applied to this surface: userinfo must be stripped before any
    // host inspection, so a loopback-looking label in the userinfo position never fools the
    // check. Refusal is GHCLI015, before any request and before any protocol reply.
    for url in [
        "http://api.example.com",
        "http://[::1]@evil.com",
        "http://localhost:tok@attacker.example",
    ] {
        let session = mcp_session_with(
            &["--url", url, "--actor", "agent-chat"],
            &[("GRAPHHELM_API_TOKEN", "test-token")],
            &[serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})],
        );
        assert!(
            !session.output.status.success(),
            "{url} must be refused: {:?}",
            session.output
        );
        assert!(session.replies.is_empty(), "no protocol reply for {url}");
        let stdout = String::from_utf8_lossy(&session.output.stdout);
        assert!(
            stdout.contains("GHCLI015"),
            "the refusal for {url} carries GHCLI015: {stdout}"
        );
    }
}

#[test]
fn both_token_sources_absent_is_a_ghcli015_naming_the_two_options() {
    let session = mcp_session_with(
        &["--url", "http://127.0.0.1:9", "--actor", "agent-chat"],
        &[],
        &[],
    );
    assert!(!session.output.status.success());
    let stdout = String::from_utf8_lossy(&session.output.stdout);
    assert!(stdout.contains("GHCLI015"), "{stdout}");
    assert!(
        stdout.contains("--token-file") && stdout.contains("GRAPHHELM_API_TOKEN"),
        "the refusal names both options: {stdout}"
    );
}

#[test]
fn the_token_never_travels_via_argv_and_never_appears_in_output() {
    let directory = tempfile::tempdir().unwrap();
    let token_path = directory.path().join("token");
    std::fs::write(
        &token_path,
        "SENTINEL-mcp-token-value
",
    )
    .unwrap();

    // One full session: lifecycle, a tools/call that fails (nothing serves 127.0.0.1:9),
    // and EOF. The sentinel may appear in NO stdout line and NO stderr line, ever.
    let session = mcp_session_with(
        &[
            "--url",
            "http://127.0.0.1:9",
            "--token-file",
            token_path.to_str().unwrap(),
            "--actor",
            "agent-chat",
        ],
        &[],
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            // A REAL tool against a dead port: the ApiClient genuinely attempts the
            // request (A's Task 3 review note (2) — the scan crosses the transport).
            serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "status", "arguments": {"executionId": "exec-x"}}}),
        ],
    );
    let stdout = String::from_utf8_lossy(&session.output.stdout);
    let stderr = String::from_utf8_lossy(&session.output.stderr);
    assert!(
        !stdout.contains("SENTINEL-mcp-token-value"),
        "the token must never reach stdout: {stdout}"
    );
    assert!(
        !stderr.contains("SENTINEL-mcp-token-value"),
        "the token must never reach stderr: {stderr}"
    );
}

#[test]
fn an_invalid_actor_is_refused_with_ghcli015() {
    let session = mcp_session_with(
        &["--url", "http://127.0.0.1:9", "--actor", "NOT AN ID!!"],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[],
    );
    assert!(!session.output.status.success());
    let stdout = String::from_utf8_lossy(&session.output.stdout);
    assert!(stdout.contains("GHCLI015"), "{stdout}");
}

// ---------------------------------------------------------------------------------------------
// The fourteen tools — one live serve, one MCP process wired to it.
// ---------------------------------------------------------------------------------------------

use std::io::{BufRead, BufReader, Read as IoRead, Write as IoWrite};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

struct ServerGuard {
    child: Child,
}

impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Mirrors `commands::serve::token_path` (sibling `<events>.token`) — the same second copy
/// `api_http.rs` keeps, for the same bin-only reason.
fn token_path(events: &Path) -> PathBuf {
    let mut name = events.file_name().map_or_else(
        || std::ffi::OsString::from("events"),
        std::ffi::OsStr::to_os_string,
    );
    name.push(".token");
    events.with_file_name(name)
}

/// Spawns `graphhelm serve` on an ephemeral port and returns (guard, base URL, token) — the
/// `api_http.rs` harness pattern, trimmed to what these tests need.
fn serve(events: &Path) -> (ServerGuard, String, String) {
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "serve",
            "--events",
            events.to_str().unwrap(),
            "--bind",
            "127.0.0.1:0",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let started: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(started["command"], "serve.started", "{started}");
    let address = started["data"]["address"].as_str().unwrap().to_owned();
    let token = std::fs::read_to_string(token_path(events))
        .unwrap()
        .trim()
        .to_owned();
    let base = format!("http://{address}");
    wait_for_health(&address);
    (ServerGuard { child }, base, token)
}

fn wait_for_health(address: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(mut stream) = TcpStream::connect(address) {
            let request =
                format!("GET /health HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
            if stream.write_all(request.as_bytes()).is_ok() {
                let mut reply = String::new();
                let _ = stream.read_to_string(&mut reply);
                if reply.starts_with("HTTP/1.1 200") {
                    return;
                }
            }
        }
        assert!(Instant::now() < deadline, "the server never became healthy");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// One direct API POST — the parity oracle for the delta assertions (hand-rolled over
/// `TcpStream`, the `api_http.rs` pattern).
fn post_json(
    base: &str,
    token: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &serde_json::Value,
) -> (u16, serde_json::Value) {
    let address = base.strip_prefix("http://").unwrap();
    let payload = serde_json::to_vec(body).unwrap();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        payload.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(&payload).unwrap();
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).unwrap();
    let text = String::from_utf8_lossy(&reply);
    let status: u16 = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap();
    let json_body = text
        .split("\r\n\r\n")
        .nth(1)
        .and_then(|body| {
            serde_json::from_str(
                body.trim_start_matches(|c: char| c.is_ascii_hexdigit() || c == '\r' || c == '\n'),
            )
            .ok()
        })
        .unwrap_or(serde_json::Value::Null);
    (status, json_body)
}

fn signal_envelope_value(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "source": {"type": "node", "id": "implementation"},
        "type": "no_progress",
        "severity": "high",
        "description": "the deploy stage needs a manual review",
        "evidence": ["exec-1"],
        "emittedAt": "2026-08-16T00:00:00Z"
    })
}

/// A live wired session: serve + a started execution + `graphhelm mcp` pointed at it.
struct WiredHarness {
    _directory: tempfile::TempDir,
    _server: ServerGuard,
    base: String,
    token: String,
    token_file: PathBuf,
    events: PathBuf,
}

fn wired(execution: &str) -> WiredHarness {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let fixtures = write_json_file(
        directory.path(),
        "fixtures.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "failure", "deploy": "success"}}),
    );
    let (server, base, token) = serve(&events);
    let graph = root_dir().join("examples/graphs/manual-override-deploy.yaml");
    let (status, reply) = post_json(
        &base,
        &token,
        &format!("/v1/executions/{execution}/start"),
        &[
            ("Idempotency-Key", "mcp-harness-start-1"),
            ("X-GraphHelm-Actor", "agent-harness"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "fixtures": fixtures.to_str().unwrap(),
            "mode": "supervised",
        }),
    );
    assert_eq!(status, 200, "the harness start succeeds: {reply}");
    let token_file = directory.path().join("token");
    std::fs::write(&token_file, &token).unwrap();
    WiredHarness {
        _directory: directory,
        _server: server,
        base,
        token,
        token_file,
        events,
    }
}

fn root_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn write_json_file(directory: &Path, name: &str, value: &serde_json::Value) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

impl WiredHarness {
    fn session(&self, lines: &[serde_json::Value]) -> McpSession {
        self.session_as("agent-chat", "agent", lines)
    }

    /// The Task 6 choreography needs distinct actors per MCP process: same wiring, chosen
    /// identity. Each spawn is its own process with its own OS-random nonce — two sessions
    /// never share an idempotency key by construction.
    fn session_as(&self, actor: &str, actor_type: &str, lines: &[serde_json::Value]) -> McpSession {
        mcp_session_with(
            &[
                "--url",
                &self.base,
                "--token-file",
                self.token_file.to_str().unwrap(),
                "--actor",
                actor,
                "--actor-type",
                actor_type,
            ],
            &[],
            lines,
        )
    }

    fn head_sequence(&self, execution: &str) -> u64 {
        let value = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args([
                "execution",
                "status",
                "--events",
                self.events.to_str().unwrap(),
                "--execution",
                execution,
            ])
            .output()
            .unwrap();
        let reply: serde_json::Value = serde_json::from_slice(&value.stdout).unwrap();
        reply["data"]["headSequence"].as_u64().unwrap()
    }
}

fn tool_call(id: serde_json::Value, name: &str, arguments: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": {"name": name, "arguments": arguments}})
}

/// The tool result contract: content [{type:"text",text:<API envelope JSON>}], isError flag.
fn tool_envelope(reply: &serde_json::Value) -> (bool, serde_json::Value) {
    let result = &reply["result"];
    let is_error = result["isError"].as_bool().unwrap_or(false);
    let text = result["content"][0]["text"].as_str().unwrap_or("null");
    (
        is_error,
        serde_json::from_str(text).unwrap_or(serde_json::Value::Null),
    )
}

#[test]
fn tools_list_names_exactly_the_eighteen_tools_with_closed_schemas() {
    let session = mcp_session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ]);
    let tools = session.replies[1]["result"]["tools"]
        .as_array()
        .expect("a tool array")
        .clone();
    let names: Vec<&str> = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec![
            "start",
            "status",
            "events",
            "signal",
            "approve",
            "pause",
            "resume",
            "cancel",
            "routes",
            "wake_arm",
            "wake_status",
            "amend_budget",
            "wake_wait",
            "probe",
            "resolve_contract",
            "memory_status",
            "present",
            "compile_context"
        ],
        "exactly the eighteen tools, in order, and NOTHING else — no credential tool exists by \
         design (omission is the enforcement); #223 added resolve_contract, memory_status, \
         present, then compile_context, each after the one before it. NOTE: this list was found \
         out of date on main (stopped at memory_status, missing present -- #357 added the tool \
         but not this pin) while resolving this merge; corrected here alongside adding \
         compile_context, not a defect introduced by this change."
    );
    for tool in &tools {
        let schema = &tool["inputSchema"];
        assert_eq!(
            schema["additionalProperties"],
            serde_json::json!(false),
            "{}: every schema is closed",
            tool["name"]
        );
        assert!(
            schema["required"].is_array(),
            "{}: required fields listed",
            tool["name"]
        );
        // What this loop CHECKS is that a /v1/ path is named. What its message used to
        // CLAIM was that the tool maps to that call -- a stronger statement, true of the
        // first twelve by accident and never asserted anywhere. `wake_wait` is the first
        // tool where the claim went false while the check stayed true: it CONSULTS a
        // request (the lease read that enforces sleeper-only) and maps to none, because
        // its work is a local block.
        //
        // The exception is a closed list in code, not a sentence in a description --
        // the same shape as PARITY_EXCEPTIONS. A fourteenth non-mapping tool fails the
        // arity below and has to be argued for in a diff.
        assert!(
            tool["description"].as_str().unwrap().contains("/v1/"),
            "{}: the description names a /v1/ request -- the one it maps to, or, for the consult-only list, the one it consults",
            tool["name"]
        );
    }

    /// Tools that NAME a request without MAPPING to one. Exactly one exists.
    const CONSULT_ONLY: [&str; 1] = ["wake_wait"];
    for name in CONSULT_ONLY {
        let tool = tools
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("{name} is listed as consult-only but is not a tool"));
        assert!(
            tool["description"]
                .as_str()
                .unwrap()
                .contains("is NOT an API call"),
            "{name}: a consult-only tool must SAY the block is not the request it names"
        );
    }
    // The per-tool mapping is proven by `each_tool_maps_to_exactly_one_api_request_and_
    // returns_the_envelope`, which exercises tools BY NAME rather than iterating this
    // table -- so it does not cover `wake_wait`, and this list is where that is said out
    // loud instead of being discovered by the next reader.
    assert!(
        CONSULT_ONLY.len() == 1 && CONSULT_ONLY[0] == "wake_wait",
        "a new consult-only tool needs its own justification, not a longer list"
    );
}

#[test]
fn each_tool_maps_to_exactly_one_api_request_and_returns_the_envelope() {
    let harness = wired("exec-mcp-map");

    // status: the tool's envelope data equals the CLI's status for the same execution.
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(2),
            "status",
            serde_json::json!({"executionId": "exec-mcp-map"}),
        ),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "{envelope}");
    assert_eq!(envelope["command"], "execution.status");
    assert_eq!(envelope["data"]["executionId"], "exec-mcp-map");

    // signal: the head advances by exactly the delta the direct API call produces.
    let before = harness.head_sequence("exec-mcp-map");
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!("sig-1"),
            "signal",
            serde_json::json!({
                "executionId": "exec-mcp-map",
                "signal": signal_envelope_value("signal-mcp-1"),
                "evidenceOut": harness.token_file.with_file_name("evidence.json").to_str().unwrap(),
            }),
        ),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "{envelope}");
    assert_eq!(envelope["command"], "execution.signal");
    let after_mcp = harness.head_sequence("exec-mcp-map");
    let mcp_delta = after_mcp - before;

    let (status, _reply) = post_json(
        &harness.base,
        &harness.token,
        "/v1/executions/exec-mcp-map/signal",
        &[
            ("Idempotency-Key", "direct-signal-1"),
            ("X-GraphHelm-Actor", "agent-direct"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &serde_json::json!({
            "signal": signal_envelope_value("signal-direct-1"),
            "evidenceOut": harness.token_file.with_file_name("evidence2.json").to_str().unwrap(),
        }),
    );
    assert_eq!(status, 200);
    let after_direct = harness.head_sequence("exec-mcp-map");
    assert_eq!(
        mcp_delta,
        after_direct - after_mcp,
        "the MCP tool appends exactly what the direct API call appends"
    );

    // events: the tail is readable through the tool and carries the signal just recorded.
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(4),
            "events",
            serde_json::json!({"executionId": "exec-mcp-map", "limit": 50}),
        ),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "{envelope}");
    assert!(
        serde_json::to_string(&envelope)
            .unwrap()
            .contains("signal_recorded"),
        "the events tail shows the recorded signal: {envelope}"
    );

    // An API failure travels as the envelope with isError true — codes reach the chat
    // (a garbage signal envelope is a real 400 from the API's own validation).
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(5),
            "signal",
            serde_json::json!({
                "executionId": "exec-mcp-map",
                "signal": {"not": "a signal"},
                "evidenceOut": harness.token_file.with_file_name("ev-bad.json").to_str().unwrap(),
            }),
        ),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(is_error, "an API failure is isError true: {envelope}");
    assert_eq!(envelope["ok"], serde_json::json!(false));
    assert!(
        serde_json::to_string(&envelope).unwrap().contains("GHCLI"),
        "the envelope's own code reaches the chat intact: {envelope}"
    );
}

#[test]
fn pause_passes_mode_immediate_through() {
    let harness = wired("exec-mcp-pause");
    // The API receives the 05d body shape: the reply is the pause envelope (accepted or a
    // state refusal), never a 400 body-shape error — and the same holds with mode absent
    // (the graceful default).
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(2),
            "pause",
            serde_json::json!({"executionId": "exec-mcp-pause", "mode": "immediate"}),
        ),
        tool_call(
            serde_json::json!(3),
            "pause",
            serde_json::json!({"executionId": "exec-mcp-pause"}),
        ),
    ]);
    for reply in &session.replies[1..=2] {
        let (_is_error, envelope) = tool_envelope(reply);
        assert_eq!(
            envelope["command"], "execution.pause",
            "the pause body shape reached the API: {envelope}"
        );
    }
}

#[test]
fn a_secret_shaped_argument_is_refused_and_never_echoed() {
    let harness = wired("exec-mcp-secret");
    for secret in [
        "sk-ant-SENTINEL123",
        "sk-proj-SENTINEL456",
        "-----BEGIN SENTINEL KEY-----",
    ] {
        let mut envelope = signal_envelope_value("signal-secret-1");
        envelope["description"] = serde_json::json!(secret);
        let session = harness.session(&[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("sec-1"),
                "signal",
                serde_json::json!({
                    "executionId": "exec-mcp-secret",
                    "signal": envelope,
                    "evidenceOut": harness.token_file.with_file_name("ev-secret.json").to_str().unwrap(),
                }),
            ),
        ]);
        let refusal = &session.replies[1];
        assert!(
            refusal.get("error").is_some(),
            "a secret-shaped value is refused: {refusal}"
        );
        assert!(
            refusal["error"]["message"]
                .as_str()
                .unwrap()
                .contains("secret-shaped"),
            "the refusal names the rule: {refusal}"
        );
        let stdout = String::from_utf8_lossy(&session.output.stdout);
        let stderr = String::from_utf8_lossy(&session.output.stderr);
        assert!(!stdout.contains("SENTINEL"), "never echoed to stdout");
        assert!(!stderr.contains("SENTINEL"), "never echoed to stderr");
    }
}

// ---------------------------------------------------------------------------------------------
// Task 6: parity, retry, and the §5 choreography.
// ---------------------------------------------------------------------------------------------

/// Mirrors `api_http.rs`'s (empty) list: a field that may legitimately differ between the two
/// surfaces would be named here with its reason. Empty by design — a difference is a real
/// finding, not something to paper over.
const MCP_PARITY_EXCEPTIONS: &[&str] = &[];

/// M08's liveness instants are NORMALISED here, never excluded, for the same reason the
/// HTTP parity guard normalises them: the two halves drive the same story against two
/// independent stores at two different moments, so `startedAt`/`lastEventAt` cannot be equal
/// -- that is the clock, not a divergence between surfaces.
///
/// Removing the fields would stop this guard watching them, and a surface that dropped
/// `lastEventAt` altogether would then pass. Replacing each value with a marker keeps
/// PRESENCE and, for `nodeLastEventAt`, the SET OF NODES under comparison. Only the
/// unequal-by-construction value goes, so `MCP_PARITY_EXCEPTIONS` stays empty by design.
fn normalise_instants(mut data: serde_json::Value) -> serde_json::Value {
    if let Some(object) = data.as_object_mut() {
        for field in ["startedAt", "lastEventAt"] {
            if let Some(value) = object.get_mut(field)
                && !value.is_null()
            {
                *value = serde_json::Value::String("<instant>".to_owned());
            }
        }
        if let Some(serde_json::Value::Object(per_node)) = object.get_mut("nodeLastEventAt") {
            for value in per_node.values_mut() {
                *value = serde_json::Value::String("<instant>".to_owned());
            }
        }
    }
    data
}

/// The 05a scripted story (start → signal → approve → pause → resume → completion) driven
/// entirely through MCP tools against a fresh store, returning the final status envelope's
/// `data` read through the same MCP surface.
fn run_story_over_mcp(directory: &Path) -> serde_json::Value {
    let events = directory.join("events");
    let (_guard, base, token) = serve(&events);
    let token_file = directory.join("token");
    std::fs::write(&token_file, &token).unwrap();
    let graph = root_dir().join("examples/graphs/manual-override-deploy.yaml");
    let blocking = write_json_file(
        directory,
        "mcp-blocking.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "failure"}}),
    );
    let recovery = write_json_file(
        directory,
        "mcp-recovery.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "success", "deploy": "success"}}),
    );
    let evidence_out = directory.join("mcp-parity-evidence.json");

    let session = mcp_session_with(
        &[
            "--url",
            &base,
            "--token-file",
            token_file.to_str().unwrap(),
            "--actor",
            "owner-parity",
            "--actor-type",
            "owner",
        ],
        &[],
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("story-start"),
                "start",
                serde_json::json!({
                    "executionId": "exec-parity-guard",
                    "file": graph.to_str().unwrap(),
                    "fixtures": blocking.to_str().unwrap(),
                    "mode": "supervised",
                }),
            ),
            tool_call(
                serde_json::json!("story-signal"),
                "signal",
                serde_json::json!({
                    "executionId": "exec-parity-guard",
                    "signal": signal_envelope_value("signal-parity-mcp"),
                    "evidenceOut": evidence_out.to_str().unwrap(),
                }),
            ),
            tool_call(
                serde_json::json!("story-approve"),
                "approve",
                serde_json::json!({"executionId": "exec-parity-guard", "node": "implementation"}),
            ),
            tool_call(
                serde_json::json!("story-pause"),
                "pause",
                serde_json::json!({"executionId": "exec-parity-guard"}),
            ),
            tool_call(
                serde_json::json!("story-resume"),
                "resume",
                serde_json::json!({
                    "executionId": "exec-parity-guard",
                    "file": graph.to_str().unwrap(),
                    "fixtures": recovery.to_str().unwrap(),
                }),
            ),
            tool_call(
                serde_json::json!("story-status"),
                "status",
                serde_json::json!({"executionId": "exec-parity-guard"}),
            ),
        ],
    );
    assert_eq!(session.replies.len(), 7, "{:?}", session.replies);
    for reply in &session.replies[1..] {
        let (is_error, envelope) = tool_envelope(reply);
        assert!(!is_error, "every story step must succeed: {envelope}");
    }
    let (_, resume_envelope) = tool_envelope(&session.replies[5]);
    assert_eq!(
        resume_envelope["data"]["status"], "completed",
        "the story must complete: {resume_envelope}"
    );
    let (_, status_envelope) = tool_envelope(&session.replies[6]);
    status_envelope["data"].clone()
}

/// One direct API GET — the read half of the oracle (the `post_json` pattern above).
fn http_get_json(base: &str, token: &str, path: &str) -> serde_json::Value {
    let address = base.strip_prefix("http://").unwrap();
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nAuthorization: Bearer {token}\r\n\r\n"
    );
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).unwrap();
    let text = String::from_utf8_lossy(&reply);
    text.split("\r\n\r\n")
        .nth(1)
        .and_then(|body| {
            serde_json::from_str(
                body.trim_start_matches(|c: char| c.is_ascii_hexdigit() || c == '\r' || c == '\n'),
            )
            .ok()
        })
        .unwrap_or(serde_json::Value::Null)
}

/// The same story through direct HTTP against its own fresh store — the oracle half.
fn run_story_over_http_data(directory: &Path) -> serde_json::Value {
    let events = directory.join("events");
    let (_guard, base, token) = serve(&events);
    let graph = root_dir().join("examples/graphs/manual-override-deploy.yaml");
    let blocking = write_json_file(
        directory,
        "http-blocking.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "failure"}}),
    );
    let recovery = write_json_file(
        directory,
        "http-recovery.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "success", "deploy": "success"}}),
    );
    let evidence_out = directory.join("http-parity-evidence.json");
    let actor: [(&str, &str); 2] = [
        ("X-GraphHelm-Actor", "owner-parity"),
        ("X-GraphHelm-Actor-Type", "owner"),
    ];

    let step = |path: &str, key: &str, body: &serde_json::Value| -> serde_json::Value {
        let mut headers = vec![("Idempotency-Key", key)];
        headers.extend(actor);
        let (status, reply) = post_json(&base, &token, path, &headers, body);
        assert_eq!(status, 200, "{path}: {reply}");
        reply
    };
    step(
        "/v1/executions/exec-parity-guard/start",
        "parity-start",
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "fixtures": blocking.to_str().unwrap(),
            "mode": "supervised",
        }),
    );
    step(
        "/v1/executions/exec-parity-guard/signal",
        "parity-signal",
        &serde_json::json!({
            "signal": signal_envelope_value("signal-parity-mcp"),
            "evidenceOut": evidence_out.to_str().unwrap(),
        }),
    );
    step(
        "/v1/executions/exec-parity-guard/approve",
        "parity-approve",
        &serde_json::json!({"node": "implementation"}),
    );
    step(
        "/v1/executions/exec-parity-guard/pause",
        "parity-pause",
        &serde_json::json!({}),
    );
    let resume = step(
        "/v1/executions/exec-parity-guard/resume",
        "parity-resume",
        &serde_json::json!({
            "file": graph.to_str().unwrap(),
            "fixtures": recovery.to_str().unwrap(),
        }),
    );
    assert_eq!(resume["data"]["status"], "completed", "{resume}");

    http_get_json(&base, &token, "/v1/executions/exec-parity-guard")["data"].clone()
}

#[test]
fn the_mcp_and_the_api_report_identical_status_for_the_same_story() {
    let mcp_directory = tempfile::tempdir().unwrap();
    let mcp_data = run_story_over_mcp(mcp_directory.path());

    let http_directory = tempfile::tempdir().unwrap();
    let http_data = run_story_over_http_data(http_directory.path());

    assert!(MCP_PARITY_EXCEPTIONS.is_empty(), "empty by design");
    assert_eq!(
        normalise_instants(mcp_data),
        normalise_instants(http_data),
        "the MCP surface and the API must report identical status data for the identical \
         story; MCP_PARITY_EXCEPTIONS is empty by design — a difference here is a real \
         finding (D-039's sentence as a test, the tripwire for every serve change under \
         the MCP layer)"
    );
}

#[test]
fn a_retried_tool_call_reuses_the_key_and_a_divergent_reuse_travels_as_409() {
    let harness = wired("exec-mcp-retry");
    let signal_args = serde_json::json!({
        "executionId": "exec-mcp-retry",
        "signal": signal_envelope_value("signal-retry-1"),
        "evidenceOut": harness.events.with_file_name("retry-evidence.json").to_str().unwrap(),
    });
    let mut divergent_args = signal_args.clone();
    divergent_args["signal"]["description"] = serde_json::json!("a DIFFERENT body, same id");

    let before = harness.head_sequence("exec-mcp-retry");
    // The same tools/call line twice (same rpc id, same arguments), then the SAME id with
    // DIFFERENT arguments — one session, one nonce, so the key is identical across all three.
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(serde_json::json!(7), "signal", signal_args.clone()),
        tool_call(serde_json::json!(7), "signal", signal_args),
        tool_call(serde_json::json!(7), "signal", divergent_args),
    ]);
    assert_eq!(session.replies.len(), 4, "{:?}", session.replies);

    let (first_error, first) = tool_envelope(&session.replies[1]);
    assert!(!first_error, "the first call succeeds: {first}");
    let (second_error, second) = tool_envelope(&session.replies[2]);
    assert!(
        !second_error,
        "the retry is a success, absorbed by the API's Complete state: {second}"
    );
    let after = harness.head_sequence("exec-mcp-retry");
    let single_delta = first["data"]["headSequence"].as_u64().unwrap() - before;
    assert_eq!(
        after - before,
        single_delta,
        "the retry and the divergent reuse appended nothing"
    );

    let (divergent_error, divergent) = tool_envelope(&session.replies[3]);
    assert!(
        divergent_error,
        "a divergent reuse is an error: {divergent}"
    );
    assert!(
        divergent
            .to_string()
            .contains("GHE003_IDEMPOTENCY_CONFLICT"),
        "the API's 409 names the reused key's conflict: {divergent}"
    );
}

/// Polls the events tail through a fresh MCP session (its own process each time) until an
/// event of `kind` attributed to `actor` appears; panics after ten attempts. The events tail
/// is the ONLY channel — the test passes nothing between the sessions but ids.
fn poll_for_attributed_event(
    harness: &WiredHarness,
    polling_actor: &str,
    execution: &str,
    kind: &str,
    expected_actor: &str,
) -> serde_json::Value {
    for _ in 0..10 {
        let session = harness.session_as(
            polling_actor,
            "agent",
            &[
                initialize_request(1, "2025-06-18"),
                initialized_notification(),
                tool_call(
                    serde_json::json!("poll-events"),
                    "events",
                    serde_json::json!({"executionId": execution, "limit": 1000}),
                ),
            ],
        );
        let (is_error, envelope) = tool_envelope(&session.replies[1]);
        assert!(!is_error, "{envelope}");
        if let Some(found) = envelope["data"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|event| event["kind"]["type"] == kind && event["actor"]["id"] == expected_actor)
        {
            return found.clone();
        }
    }
    panic!("no {kind} attributed to {expected_actor} appeared in ten polls");
}

#[test]
fn two_chat_sessions_coordinate_through_events_alone_and_resolve_a_race() {
    let harness = wired("exec-mcp-choreo");
    let evidence = harness
        .events
        .with_file_name("choreo-evidence.json")
        .to_str()
        .unwrap()
        .to_owned();

    // Scout signals — the only thing it does that builder could possibly learn from.
    let scout = harness.session_as(
        "agent-scout",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("scout-signal"),
                "signal",
                serde_json::json!({
                    "executionId": "exec-mcp-choreo",
                    "signal": signal_envelope_value("signal-choreo-1"),
                    "evidenceOut": evidence,
                }),
            ),
        ],
    );
    let (is_error, envelope) = tool_envelope(&scout.replies[1]);
    assert!(!is_error, "{envelope}");

    // Builder discovers the signal purely by reading the tail, fully attributed to scout.
    let observed = poll_for_attributed_event(
        &harness,
        "agent-builder",
        "exec-mcp-choreo",
        "signal_recorded",
        "agent-scout",
    );
    assert_eq!(observed["actor"]["type"], "agent");

    // Builder approves the blocked node — its own decision, informed only by what it read.
    let builder = harness.session_as(
        "agent-builder",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("builder-approve"),
                "approve",
                serde_json::json!({"executionId": "exec-mcp-choreo", "node": "implementation"}),
            ),
        ],
    );
    let (is_error, envelope) = tool_envelope(&builder.replies[1]);
    assert!(!is_error, "{envelope}");

    // Scout discovers the approval the same way — polling, never told.
    poll_for_attributed_event(
        &harness,
        "agent-scout",
        "exec-mcp-choreo",
        "node_outcome_recorded",
        "agent-builder",
    );

    // The race: both sessions pin If-Match to the SAME head. Scout's pause lands first and
    // moves the head; builder's resume carries the now-stale head and 409s — exactly one.
    let stale_head = harness.head_sequence("exec-mcp-choreo");
    let scout_race = harness.session_as(
        "agent-scout",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("scout-pause"),
                "pause",
                serde_json::json!({"executionId": "exec-mcp-choreo", "ifMatch": stale_head}),
            ),
        ],
    );
    let (is_error, envelope) = tool_envelope(&scout_race.replies[1]);
    assert!(!is_error, "scout wins the race: {envelope}");

    let graph = root_dir().join("examples/graphs/manual-override-deploy.yaml");
    let recovery = write_json_file(
        harness.events.parent().unwrap(),
        "choreo-recovery.json",
        &serde_json::json!({"nodeOutcomes": {"implementation": "success", "deploy": "success"}}),
    );
    let builder_race = harness.session_as(
        "agent-builder",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            // The stale If-Match: exactly this one 409s.
            tool_call(
                serde_json::json!("builder-resume"),
                "resume",
                serde_json::json!({
                    "executionId": "exec-mcp-choreo",
                    "file": graph.to_str().unwrap(),
                    "fixtures": recovery.to_str().unwrap(),
                    "ifMatch": stale_head,
                }),
            ),
            // The loser re-reads status...
            tool_call(
                serde_json::json!("builder-reread"),
                "status",
                serde_json::json!({"executionId": "exec-mcp-choreo"}),
            ),
        ],
    );
    let (lost, refusal) = tool_envelope(&builder_race.replies[1]);
    assert!(lost, "the stale If-Match must 409: {refusal}");
    assert!(
        refusal.to_string().contains("currentHead"),
        "the refusal carries the current head for the re-read: {refusal}"
    );
    let (is_error, status_envelope) = tool_envelope(&builder_race.replies[2]);
    assert!(!is_error, "{status_envelope}");
    let fresh_head = status_envelope["data"]["headSequence"].as_u64().unwrap();
    assert!(fresh_head > stale_head, "the head moved under the loser");

    // ...and retries ONCE with the fresh head, succeeding.
    let builder_retry = harness.session_as(
        "agent-builder",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("builder-resume-retry"),
                "resume",
                serde_json::json!({
                    "executionId": "exec-mcp-choreo",
                    "file": graph.to_str().unwrap(),
                    "fixtures": recovery.to_str().unwrap(),
                    "ifMatch": fresh_head,
                }),
            ),
        ],
    );
    let (is_error, envelope) = tool_envelope(&builder_retry.replies[1]);
    assert!(
        !is_error,
        "the retry with the fresh head succeeds: {envelope}"
    );
    assert_eq!(envelope["data"]["status"], "completed", "{envelope}");
}

// ---------------------------------------------------------------------------------------------
// Task 7: packaging validation + the notification-call decision.
// ---------------------------------------------------------------------------------------------

/// A's Task 5 review note, decided here: a tools/call WITHOUT an id (notification form) is
/// never executed — a mutation with no response channel cannot participate in the retry
/// choreography (its key would be unretryable), so the server refuses to run it at all. The
/// head not moving is the observable; no reply exists by the notification rule.
#[test]
fn a_notification_form_tool_call_is_never_executed() {
    let harness = wired("exec-mcp-notify");
    let before = harness.head_sequence("exec-mcp-notify");
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        // No "id": notification form.
        serde_json::json!({"jsonrpc": "2.0", "method": "tools/call",
        "params": {"name": "signal", "arguments": {
            "executionId": "exec-mcp-notify",
            "signal": signal_envelope_value("signal-notify-1"),
            "evidenceOut": harness.token_file.with_file_name("ev-notify.json").to_str().unwrap(),
        }}}),
    ]);
    assert_eq!(session.replies.len(), 1, "only the initialize reply exists");
    let after = harness.head_sequence("exec-mcp-notify");
    assert_eq!(
        before, after,
        "a notification-form tool call must not execute"
    );
}

fn plugin_root() -> PathBuf {
    root_dir().join("examples/chat-surface")
}

/// Every `tool:`-marked name in a SKILL.md (the marker convention the skills define).
fn skill_tool_names(skill: &str) -> Vec<String> {
    let text = std::fs::read_to_string(
        plugin_root().join(format!("claude-code-plugin/skills/{skill}/SKILL.md")),
    )
    .unwrap();
    let mut names = Vec::new();
    for part in text.split("`tool:").skip(1) {
        if let Some(name) = part.split('`').next() {
            names.push(name.to_owned());
        }
    }
    names.sort();
    names.dedup();
    names
}

#[test]
fn the_packaging_is_valid_and_names_only_real_tools() {
    // plugin.json and .mcp.json parse, and the server registration carries --token-file and
    // never an inline token value.
    let plugin: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            plugin_root().join("claude-code-plugin/.claude-plugin/plugin.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(plugin["name"], "graphhelm-chat");
    let mcp: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(plugin_root().join("claude-code-plugin/.mcp.json")).unwrap(),
    )
    .unwrap();
    let args: Vec<&str> = mcp["mcpServers"]["graphhelm"]["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert!(args.contains(&"--token-file"), "{args:?}");
    assert!(
        !args
            .iter()
            .any(|arg| arg.starts_with("sk-") || arg.contains("Bearer")),
        "never an inline token: {args:?}"
    );

    // The Codex TOML: validated by line shape rather than a toml dependency (`toml` is NOT a
    // workspace dep, and adding one for a two-line snippet is the wrong trade — stated per
    // the plan).
    let toml_text = std::fs::read_to_string(plugin_root().join("codex/config.toml")).unwrap();
    assert!(toml_text.contains("[mcp_servers.graphhelm]"));
    assert!(toml_text.contains("command = \"graphhelm\""));
    assert!(toml_text.contains("--token-file"));

    // Every tool a skill names exists in the current closed table; the credential-refusal sentence
    // is present verbatim in operate-execution; both READMEs carry the deletability sentence.
    let table = [
        "start",
        "status",
        "events",
        "signal",
        "approve",
        "pause",
        "resume",
        "cancel",
        "routes",
        "wake_arm",
        "wake_status",
        "amend_budget",
        "wake_wait",
        "probe",
    ];
    for skill in ["operate-execution", "observe-agents"] {
        let names = skill_tool_names(skill);
        assert!(!names.is_empty(), "{skill} names its tools");
        for name in &names {
            assert!(
                table.contains(&name.as_str()),
                "{skill} references a tool that does not exist: {name}"
            );
        }
    }
    let operate = std::fs::read_to_string(
        plugin_root().join("claude-code-plugin/skills/operate-execution/SKILL.md"),
    )
    .unwrap();
    assert!(
        operate.contains("a pasted secret is refused, never echoed")
            || operate.contains("A pasted secret is refused, never echoed"),
        "the credential-refusal instruction is present verbatim"
    );
    for readme in ["claude-code-plugin/README.md", "codex/README.md"] {
        let text = std::fs::read_to_string(plugin_root().join(readme)).unwrap();
        assert!(
            text.contains("deleting it loses convenience only"),
            "{readme} carries the deletability sentence"
        );
    }
}

/// 05g Task 3: the sleeper-only surface end to end — `wake_arm` arms THIS session's lease
/// through the API (sessionId is the session's own nonce, never an argument), the armed
/// lease is visible to `wake_status` AND on the events tail as a `wake_lease` kind, and
/// re-arming replaces (the fold invariant read back through the tool).
#[test]
fn wake_arm_arms_this_session_and_wake_status_reads_it_back() {
    let harness = wired("exec-mcp-wake");
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!("arm-1"),
            "wake_arm",
            serde_json::json!({"executionId": "exec-mcp-wake", "rendezvousId": "rdv-mcp-one"}),
        ),
        tool_call(
            serde_json::json!("arm-2"),
            "wake_arm",
            serde_json::json!({"executionId": "exec-mcp-wake", "rendezvousId": "rdv-mcp-two",
                "cursor": 3}),
        ),
        tool_call(
            serde_json::json!("read-1"),
            "wake_status",
            serde_json::json!({"executionId": "exec-mcp-wake"}),
        ),
        tool_call(
            serde_json::json!("tail-1"),
            "events",
            serde_json::json!({"executionId": "exec-mcp-wake", "limit": 1000}),
        ),
    ]);
    assert_eq!(session.replies.len(), 5, "{:?}", session.replies);

    let (is_error, armed) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "{armed}");
    assert_eq!(armed["command"], "execution.wake_lease");
    let session_id = armed["data"]["sessionId"].as_str().unwrap().to_owned();
    assert!(
        armed["data"]["armedCursor"].as_u64().unwrap() > 0,
        "the default cursor is the current head: {armed}"
    );

    let (is_error, replaced) = tool_envelope(&session.replies[2]);
    assert!(!is_error, "{replaced}");

    let (is_error, live) = tool_envelope(&session.replies[3]);
    assert!(!is_error, "{live}");
    assert_eq!(live["data"]["live"], true, "{live}");
    assert_eq!(
        live["data"]["rendezvousId"], "rdv-mcp-two",
        "re-arming replaces — the fold invariant read back through the tool: {live}"
    );
    assert_eq!(live["data"]["cursor"], 3, "{live}");
    assert_eq!(
        live["data"]["sessionId"].as_str().unwrap(),
        session_id,
        "one session, one identity"
    );

    let (is_error, tail) = tool_envelope(&session.replies[4]);
    assert!(!is_error, "{tail}");
    let wake_events = tail["data"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"]["type"] == "wake_lease")
        .count();
    assert_eq!(
        wake_events, 2,
        "both armings are ordinary durable events: {tail}"
    );
}

/// M08 Task 4 and M09 decision B: the blocking wait as an MCP primitive, and the rule that
/// keeps it safe -- **a session may block only on a lease it holds itself** (the 05g
/// sleeper-only rule, which the CLI sidecar got for free by being started by the sleeper).
///
/// The rule used to be ENFORCED: the caller named a rendezvous and the tool compared it with
/// the one the session held. It is now STRUCTURAL: the caller names no rendezvous at all, and
/// no duration either -- both come from the lease this session armed. So the old test's
/// interesting case, an armed session reaching for a peer's rendezvous, is no longer a thing
/// that can be said, and a guard on the refusal message would be a guard on dead code.
///
/// What is asserted instead is the shape that makes it unsayable, plus the refusal that
/// remains reachable: a lease with no declared bound promises nothing, so waiting on it is
/// refused rather than becoming a wait with no end.
#[test]
fn a_session_may_block_only_on_the_lease_it_holds() {
    let harness = wired("exec-mcp-wait");
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!("arm-mine"),
            "wake_arm",
            serde_json::json!({"executionId": "exec-mcp-wait", "rendezvousId": "rdv-mine"}),
        ),
        tool_call(
            serde_json::json!("wait-unbounded"),
            "wake_wait",
            serde_json::json!({"executionId": "exec-mcp-wait"}),
        ),
    ]);

    let (is_error, armed) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "the fixture must really hold a lease: {armed}");
    assert_eq!(armed["data"]["rendezvousId"], "rdv-mine", "{armed}");

    // The lease was armed with no bound, so the wait is refused rather than endless. This is
    // the reachable refusal, and it is the one that matters: an endless wait is the silent
    // failure the whole decision exists to remove.
    let refused = &session.replies[2]["result"];
    assert_eq!(
        refused["isError"],
        serde_json::json!(true),
        "a lease that declared no bound promises nothing: {refused}"
    );
    let text = refused["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("maturesInSeconds"),
        "the refusal names the declaration that is missing: {text}"
    );
}

/// The structural half of the rule above: naming someone else's rendezvous, or a deadline of
/// one's own, is not something the tool's surface allows to be said.
///
/// Sabotage: put `rendezvousId` or `timeoutSeconds` back on the schema. This falls, and it is
/// the guard that keeps the CLI and the MCP halves from drifting into two definitions of one
/// tool -- which is exactly what they were between two commits of this milestone.
#[test]
fn the_wait_tool_accepts_an_identity_and_never_a_rendezvous_or_a_deadline() {
    let harness = wired("exec-mcp-wait-shape");
    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
    ]);
    let tools = session.replies[1]["result"]["tools"].as_array().unwrap();
    let wait = tools
        .iter()
        .find(|tool| tool["name"] == "wake_wait")
        .expect("wake_wait is in the closed table");
    let properties = &wait["inputSchema"]["properties"];
    assert!(
        properties["rendezvousId"].is_null() && properties["timeoutSeconds"].is_null(),
        "the caller supplies an identity, never a rendezvous and never a duration: {wait}"
    );
    assert!(
        !properties["executionId"].is_null(),
        "the identity it does supply is the execution: {wait}"
    );
}
