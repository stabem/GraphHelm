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
    mcp_session_inner(args, env, &[], lines)
}

/// Like `mcp_session_with`, but the named variables are REMOVED from the child's environment
/// rather than inherited (#855): a cell about a token being absent constructs the absence instead
/// of trusting the test runner's environment to have none.
fn mcp_session_without(args: &[&str], removed: &[&str], lines: &[serde_json::Value]) -> McpSession {
    mcp_session_inner(args, &[], removed, lines)
}

fn mcp_session_inner(
    args: &[&str],
    env: &[(&str, &str)],
    removed: &[&str],
    lines: &[serde_json::Value],
) -> McpSession {
    let mut input = String::new();
    for line in lines {
        input.push_str(&line.to_string());
        input.push('\n');
    }
    let mut command = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.arg("mcp").args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    for name in removed {
        command.env_remove(name);
    }
    let output = command
        .write_stdin(input)
        .timeout(Duration::from_secs(30))
        .output()
        .expect("the mcp server runs to EOF");
    let replies = String::from_utf8(output.stdout.clone())
        .expect("stdout is UTF-8")
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value.get("jsonrpc").is_some())
        .collect();
    McpSession { replies, output }
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

    // EOF exits cleanly within the harness's bounded process timeout, with no panic output.
    assert!(session.output.status.success(), "{:?}", session.output);
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
    // ABSENT IS CONSTRUCTED, NOT INHERITED (#855): the variable is removed from the child's
    // environment, so a token set on the host or by the harness cannot turn this cell's
    // "absent" into "present" without anyone seeing it.
    let session = mcp_session_without(
        &["--url", "http://127.0.0.1:9", "--actor", "agent-chat"],
        &["GRAPHHELM_API_TOKEN"],
        &[],
    );
    // WITH THE EVIDENCE ITS SIBLING CARRIES (#855). This cell went red exactly once, under a full
    // gate on a loaded box, and the bare `assert!` said nothing: no status, no stdout, no
    // stderr, so nothing about the mechanism could be recovered from the record. The next red
    // names what it saw.
    assert!(
        !session.output.status.success(),
        "must be refused without a token: {:?}",
        session.output
    );
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

/// Every tool the MCP surface exposes, in the order it exposes them.
///
/// ONE list, because there were two and they drifted. `the_packaging_is_valid_and_names_only_real_tools`
/// kept its own hand-written table of fourteen names to check skill references against; when
/// `evidence` was added to the surface that table was not updated, so a skill naming a REAL tool
/// would have been reported as naming one that "does not exist" -- the guard failing honest work
/// while a genuinely wrong name in a skill nobody had added yet would have been caught for the
/// wrong reason. A second copy of a set is a second thing to forget.
const MCP_TOOL_NAMES: [&str; 31] = [
    "start",
    "list",
    "topology",
    "status",
    "briefing",
    "events",
    "evidence",
    "signal",
    "document_read",
    "document_save",
    "approve",
    "pause",
    "resume",
    "cancel",
    "routes",
    "wake_arm",
    "wake_status",
    "amend_budget",
    "wake_wait",
    "route_set",
    "probe",
    "resolve_contract",
    "memory_status",
    "present",
    "compile_context",
    "memory_propose",
    "accounting",
    "sweep",
    "claim",
    "clear",
    "synthesize",
];

#[test]
fn document_tools_forward_exact_bodies_actor_and_file_concurrency() {
    use std::collections::BTreeMap;
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let observer = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut captured = Vec::new();
        for _ in 0..4 {
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "expected four document requests");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("listener failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            let mut headers = BTreeMap::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.split_once(':').expect("a request header");
                headers.insert(name.to_ascii_lowercase(), value.trim().to_owned());
            }
            let length: usize = headers["content-length"].parse().unwrap();
            assert!(length <= 800_000);
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let denied = headers
                .get("x-graphhelm-actor-type")
                .is_some_and(|kind| kind == "agent");
            let command = if request_line.contains("/documents/read ") {
                "execution.document_read"
            } else {
                "execution.document_save"
            };
            let response = serde_json::json!({"ok":!denied,"command":command,"data":{"notification":{"status":"recorded"}},"diagnostics":if denied {serde_json::json!([{"code":"OWNER_REQUIRED"}])}else{serde_json::json!([])}}).to_string();
            let status = if denied { "403 Forbidden" } else { "200 OK" };
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).unwrap();
            captured.push((request_line, headers, body));
        }
        captured
    });
    let document = serde_json::json!({"evidenceId":"evidence-docs-1","index":0});
    let edit = serde_json::json!({"executionId":"run-1","document":document,"content":"Updated rule","expectedSha256":"a".repeat(64),"reason":"Owner clarified the rule"});
    let owner = mcp_session_with(
        &[
            "--url",
            &base,
            "--actor",
            "owner-editor",
            "--actor-type",
            "owner",
        ],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!(2),
                "document_read",
                serde_json::json!({"executionId":"run-1","document":document}),
            ),
            tool_call(serde_json::json!(3), "document_save", edit.clone()),
            tool_call(serde_json::json!(3), "document_save", edit.clone()),
        ],
    );
    assert!(owner.output.status.success());
    assert_eq!(owner.replies.len(), 4);
    for reply in &owner.replies[1..] {
        assert!(reply.get("error").is_none(), "{reply}");
        assert!(reply["result"].is_object(), "{reply}");
        assert!(!tool_envelope(reply).0, "{reply}");
    }
    let agent = mcp_session_with(
        &[
            "--url",
            &base,
            "--actor",
            "agent-editor",
            "--actor-type",
            "agent",
        ],
        &[("GRAPHHELM_API_TOKEN", "test-token")],
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(serde_json::json!(2), "document_save", edit),
        ],
    );
    assert_eq!(agent.replies.len(), 2);
    assert!(
        agent.replies[1].get("error").is_none(),
        "{:?}",
        agent.replies
    );
    assert!(agent.replies[1]["result"].is_object());
    let (is_error, denied) = tool_envelope(&agent.replies[1]);
    assert!(is_error);
    assert_eq!(denied["diagnostics"][0]["code"], "OWNER_REQUIRED");
    let captured = observer.join().unwrap();
    assert!(
        captured[0]
            .0
            .starts_with("POST /v1/executions/run-1/documents/read ")
    );
    assert_eq!(captured[0].2, document);
    assert!(!captured[0].1.contains_key("idempotency-key"));
    for (line, headers, body) in &captured[1..] {
        assert!(line.starts_with("POST /v1/executions/run-1/documents/save "));
        assert_eq!(body["idempotencyKey"], headers["idempotency-key"]);
        assert!(!headers.contains_key("if-match"));
        assert_eq!(body["expectedSha256"], "a".repeat(64));
        assert!(body.get("executionId").is_none());
    }
    assert_eq!(captured[1].1["x-graphhelm-actor"], "owner-editor");
    assert_eq!(captured[1].1["x-graphhelm-actor-type"], "owner");
    assert_eq!(
        captured[1].1["idempotency-key"],
        captured[2].1["idempotency-key"]
    );
    assert_eq!(captured[1].2, captured[2].2);
    assert_eq!(captured[3].1["x-graphhelm-actor"], "agent-editor");
    assert_eq!(captured[3].1["x-graphhelm-actor-type"], "agent");
}

#[test]
fn document_tools_reject_head_pins_key_overrides_and_oversized_arguments() {
    let valid = serde_json::json!({"executionId":"run-1","document":{"evidenceId":"evidence-docs-1","index":0},"content":"Rule","expectedSha256":"a".repeat(64),"reason":"Owner revision"});
    for (field, value) in [
        ("ifMatch", serde_json::json!(1)),
        ("idempotencyKey", serde_json::json!("override")),
        ("reason", serde_json::json!("x".repeat(2049))),
        ("content", serde_json::json!("x".repeat(131073))),
    ] {
        let mut invalid = valid.clone();
        invalid[field] = value;
        let session = mcp_session(&[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(serde_json::json!(2), "document_save", invalid),
        ]);
        assert_eq!(session.replies.len(), 2);
        assert!(session.replies[1].get("result").is_none());
        assert_eq!(session.replies[1]["error"]["code"], -32602, "{field}");
    }
}

#[test]
fn tools_list_names_exactly_the_registered_tools_with_closed_schemas() {
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
        MCP_TOOL_NAMES.to_vec(),
        "exactly the tools in `MCP_TOOL_NAMES`, in order, and NOTHING else — no credential tool exists by \
         design (omission is the enforcement); #223 added resolve_contract, memory_status, \
         present, compile_context, memory_propose, then accounting, each after the one before \
         it; #288 added sweep after those; #105 added list beside start, the read a caller \
         reaches for before it knows an execution id; #107 added synthesize, the Graph Architect, \
         last; #1063 added briefing beside status, the read a harness picking a run up makes \
         first. This pin is a LIST and not a count, so a tool added \
         to TOOLS without a line here fails on the NAME rather than on a number -- which is what \
         happened to present (#357 moved TOOLS and not this list, and the gate that PR chose did \
         not run this file)."
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

/// THE `evidence` TOOL'S PATH ACTUALLY REACHES THE ROUTE.
///
/// The parity guards around it prove the tool EXISTS, that its schema is closed, and that the
/// route named in its description is registered. None of them touches the path the dispatch
/// composes, so a tool that built `/v1/executions/{id}/evidence` - or put the two ids the wrong
/// way round, or dropped a segment - would satisfy every one of them and fail only in an
/// operator's hands.
///
/// The discriminator is which REFUSAL comes back, and the two are unmistakable. A path that
/// matches no route answers `GHCLI008_SERVE_NOT_FOUND`, from the router, before any handler runs.
/// A path that reaches the evidence handler answers `GHCLI023_EVIDENCE_UNREADABLE`, because this
/// execution's events reference no such evidence - the store's own reachability gate, which only
/// exists inside the handler. Getting the second one is proof the request arrived where the
/// description claims it goes.
///
/// A fixture-driven server seals nothing (`RefusingSealer`), so there is no plaintext to read here
/// and this test does not pretend to check the decrypt - that is
/// `a_sealed_model_reply_can_be_read_back_as_the_text_the_provider_sent`, over in `runtime_http`,
/// against a real provider. What is missing until now, and what this closes, is the wiring between
/// the two.
#[test]
fn the_evidence_tool_reaches_the_evidence_route_and_not_the_router() {
    let harness = wired("exec-mcp-evidence");

    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(9),
            "evidence",
            serde_json::json!({
                "executionId": "exec-mcp-evidence",
                "evidenceId": "ev-this-execution-never-recorded-one",
            }),
        ),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);

    assert!(is_error, "a refusal travels as isError true: {envelope}");
    let code = envelope["diagnostics"][0]["code"]
        .as_str()
        .unwrap_or_default();
    assert_ne!(
        code, "GHCLI008_SERVE_NOT_FOUND",
        "the path the tool composed matched no route, so the tool points somewhere the route is \
         not: {envelope}"
    );
    assert_eq!(
        code, "GHCLI023_EVIDENCE_UNREADABLE",
        "the request must arrive at the evidence handler and be refused by its own gate: \
         {envelope}"
    );
}

/// #1083 F1, the third door: the MCP `status` and `briefing` tools refuse a well-formed id that
/// names no execution exactly as HTTP and the CLI do - `isError` with
/// `GHCLI028_EXECUTION_NOT_FOUND` - and the same session still reads the run that exists, so a
/// tool that refused everything would fail here too.
#[test]
fn the_status_and_briefing_tools_refuse_an_unknown_execution_id() {
    let harness = wired("exec-mcp-known");

    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(2),
            "status",
            serde_json::json!({"executionId": "exec-mcp-typo"}),
        ),
        tool_call(
            serde_json::json!(3),
            "briefing",
            serde_json::json!({"executionId": "exec-mcp-typo"}),
        ),
        tool_call(
            serde_json::json!(4),
            "status",
            serde_json::json!({"executionId": "exec-mcp-known"}),
        ),
    ]);
    for (index, command) in [(1, "execution.status"), (2, "execution.briefing")] {
        let (is_error, envelope) = tool_envelope(&session.replies[index]);
        assert!(
            is_error,
            "an unknown id is a refusal, not a calm run: {envelope}"
        );
        assert_eq!(envelope["command"], command, "{envelope}");
        assert_eq!(
            envelope["diagnostics"][0]["code"], "GHCLI028_EXECUTION_NOT_FOUND",
            "{envelope}"
        );
    }
    let (is_error, envelope) = tool_envelope(&session.replies[3]);
    assert!(
        !is_error,
        "the control: the run that exists still reads: {envelope}"
    );
    assert_eq!(envelope["data"]["executionId"], "exec-mcp-known");
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
            // #1057: the MCP surface names its session on every mutation, so a direct call it is
            // compared against has to name one too. Without it the two sides are not the same
            // story: one records a session boundary and the other cannot, and the delta this cell
            // measures would be about the header rather than about the tool.
            ("X-GraphHelm-Actor-Session", "direct-session-1"),
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
    // #1057: the session token rides every MCP mutation, so the API half of this parity story
    // names a session too. Otherwise the MCP side records one `agent_presence_declared` boundary
    // the API side cannot, and `headSequence` differs for a reason that is not a parity defect.
    let actor: [(&str, &str); 3] = [
        ("X-GraphHelm-Actor", "owner-parity"),
        ("X-GraphHelm-Actor-Type", "owner"),
        ("X-GraphHelm-Actor-Session", "http-parity-session"),
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
/// #1063, MVP promise 3 - continuity across harnesses. Harness A (`claude-code`) approves and
/// pauses through MCP; harness B (`codex`), a FRESH stdio process against the same server, reads
/// `briefing` and gets, byte for byte, what the CLI reads from the store directly - and every
/// decision in it names the actor that made it.
#[test]
fn a_second_harness_reads_the_same_briefing_the_first_one_left_in_the_store() {
    let harness = wired("exec-two-harness");
    let graph = root_dir().join("examples/graphs/manual-override-deploy.yaml");

    let claude = harness.session_as(
        "claude-code",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("claude-approve"),
                "approve",
                serde_json::json!({"executionId": "exec-two-harness", "node": "implementation"}),
            ),
            tool_call(
                serde_json::json!("claude-pause"),
                "pause",
                serde_json::json!({"executionId": "exec-two-harness"}),
            ),
        ],
    );
    assert_eq!(claude.replies.len(), 3, "{:?}", claude.replies);
    for reply in &claude.replies[1..] {
        let (is_error, envelope) = tool_envelope(reply);
        assert!(!is_error, "harness A's step must succeed: {envelope}");
    }

    let codex = harness.session_as(
        "codex",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("codex-briefing"),
                "briefing",
                serde_json::json!({"executionId": "exec-two-harness"}),
            ),
        ],
    );
    let (is_error, envelope) = tool_envelope(&codex.replies[1]);
    assert!(!is_error, "{envelope}");
    assert_eq!(envelope["command"], "execution.briefing", "{envelope}");
    let over_mcp = envelope["data"].clone();

    let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "execution",
            "briefing",
            "--events",
            harness.events.to_str().unwrap(),
            "--execution",
            "exec-two-harness",
        ])
        .output()
        .unwrap();
    let over_cli: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(over_cli["ok"], true, "{over_cli}");

    assert!(MCP_PARITY_EXCEPTIONS.is_empty(), "empty by design");
    assert_eq!(
        normalise_instants(over_mcp.clone()),
        normalise_instants(over_cli["data"].clone()),
        "the second harness reads exactly what the store holds"
    );

    // Each section names what harness A did.
    let decisions = over_mcp["decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), 2, "{over_mcp}");
    assert_eq!(decisions[0]["kind"], "approval");
    assert_eq!(decisions[0]["node"], "implementation");
    assert_eq!(decisions[1]["kind"], "paused");
    for decision in decisions {
        assert_eq!(
            decision["actor"]["id"], "claude-code",
            "the actor is the one the envelope recorded: {decision}"
        );
        assert_eq!(decision["actor"]["type"], "agent");
    }
    assert_eq!(over_mcp["name"], "Deploy com override manual");
    assert_eq!(over_mcp["objective"], "Produzir build implantável.");
    assert_eq!(
        over_mcp["executor"], "fixture",
        "no --manifest on this serve"
    );
    assert_eq!(over_mcp["nextStep"]["kind"], "resume_held", "{over_mcp}");
    assert!(
        over_mcp["nextStep"]["command"]
            .as_str()
            .unwrap()
            .contains("exec-two-harness")
    );
    assert_eq!(
        over_mcp["asOfSequence"],
        serde_json::json!(harness.head_sequence("exec-two-harness"))
    );

    // The graph hash is the one `resume --file` will be checked against: harness B can verify
    // the file before it acts, using nothing but the briefing and the topology tool.
    let topology = harness.session_as(
        "codex",
        "agent",
        &[
            initialize_request(1, "2025-06-18"),
            initialized_notification(),
            tool_call(
                serde_json::json!("codex-topology"),
                "topology",
                serde_json::json!({"file": graph.to_str().unwrap()}),
            ),
        ],
    );
    let (is_error, topology) = tool_envelope(&topology.replies[1]);
    assert!(!is_error, "{topology}");
    assert_eq!(
        topology["data"]["semanticHash"], over_mcp["graphHash"],
        "the briefing's graphHash is the file's semantic hash: {topology}"
    );
}

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

/// Every skill the plugin actually ships, read from disk.
///
/// This used to be a hand-written list of two names in the loop below, which meant the guard's
/// POPULATION was chosen by whoever last edited the test rather than by what the plugin contains:
/// a skill added afterwards referenced tools nothing checked, and the guard stayed green while
/// covering less than it appeared to. Enumerating the directory makes coverage follow the
/// package. `expect` rather than `unwrap`: an empty or missing skills directory is a packaging
/// defect, and it should say so instead of failing as an unwrap on a path nobody printed.
fn shipped_skills() -> Vec<String> {
    let directory = plugin_root().join("claude-code-plugin/skills");
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| {
            panic!("the plugin must ship a skills directory ({directory:?}): {error}")
        })
        .filter_map(|entry| {
            let entry = entry.ok()?;
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            // A directory without a SKILL.md is not a skill; skipping it here keeps the guard
            // about skills rather than about whatever else lands in the folder.
            entry
                .path()
                .join("SKILL.md")
                .is_file()
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    assert!(
        !names.is_empty(),
        "the plugin ships no skills at all ({directory:?})"
    );
    names
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
    // The SAME set the surface actually exposes, not a second copy of it. See `MCP_TOOL_NAMES`.
    let table = MCP_TOOL_NAMES;
    let shipped = shipped_skills();
    assert!(
        shipped.len() >= 3,
        "the plugin's three skills must all be present, found: {shipped:?}"
    );
    for skill in &shipped {
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
    // Before either wake event, every event on this freshly-started stream is content. Use that
    // observed cursor for the replacement: a fixed older cursor is already satisfied and may be
    // consumed by the asynchronous sweep before `wake_status` reads it.
    let replacement_cursor = harness.head_sequence("exec-mcp-wake");
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
                "cursor": replacement_cursor}),
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
    assert_eq!(live["data"]["cursor"], replacement_cursor, "{live}");
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

/// The `sweep` tool actually reaches `POST /v1/executions/{id}/sweep` and the record lands.
///
/// WHY THIS EXISTS BESIDE THE COMPLETENESS GUARD RATHER THAN INSTEAD OF IT.
/// `surface_completeness.rs` joins the tool table to the router on the route each description
/// NAMES — metadata, not behaviour. A dispatch arm that builds a different path, or forgets the
/// key, or drops the body, satisfies that guard completely: the description would still name the
/// right route while the tool went somewhere else. The guard proves the verb is not MISSING; only
/// a live call proves it is not LYING.
#[test]
fn the_sweep_tool_reaches_the_route_it_names_and_the_record_lands() {
    let harness = wired("exec-mcp-sweep");
    let before = harness.head_sequence("exec-mcp-sweep");

    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(9),
            "sweep",
            serde_json::json!({"executionId": "exec-mcp-sweep"}),
        ),
    ]);

    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "the sweep tool call succeeds: {envelope}");
    assert_eq!(
        envelope["command"], "execution.sweep",
        "the reply must come from the sweep command, which is what proves the dispatch arm went \
         to the route the description names: {envelope}"
    );
    assert!(
        harness.head_sequence("exec-mcp-sweep") > before,
        "a sweep over a clean stream still appends its own record: {envelope}"
    );
}

/// #105: the `list` tool reaches `GET /v1/executions` and relays that route's envelope verbatim.
///
/// Two assertions, and the second is the one that matters: the tool's `data` must equal the data
/// the API itself replies with for the same store. A tool that answered from anywhere else -- a
/// cached list, a second read path, the monitor page -- would still produce a plausible array,
/// and only a comparison against the route can tell the two apart.
#[test]
fn the_list_tool_reaches_the_execution_index_and_relays_it_verbatim() {
    let harness = wired("exec-mcp-list");

    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(serde_json::json!(2), "list", serde_json::json!({})),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "{envelope}");
    assert_eq!(envelope["command"], "execution.list");

    let listed: Vec<&str> = envelope["data"]["executions"]
        .as_array()
        .unwrap_or_else(|| panic!("the index carried no array: {envelope}"))
        .iter()
        .map(|row| row["executionId"].as_str().unwrap())
        .collect();
    assert_eq!(
        listed,
        vec!["exec-mcp-list"],
        "the seeded store holds exactly the one stream this harness started: {envelope}"
    );

    let direct = http_get_json(&harness.base, &harness.token, "/v1/executions");
    assert_eq!(
        envelope["data"], direct["data"],
        "the tool must relay the route's own answer, not compose a second one"
    );
}

/// The tool's bounds are the API's bounds: an over-large limit is refused through the tool with
/// the route's own diagnostic, never clamped into a plausible short page.
#[test]
fn the_list_tool_relays_the_routes_refusal_rather_than_clamping() {
    let harness = wired("exec-mcp-list-bounds");

    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(
            serde_json::json!(2),
            "list",
            serde_json::json!({"limit": 500}),
        ),
    ]);
    let (_is_error, envelope) = tool_envelope(&session.replies[1]);
    assert_eq!(envelope["ok"], serde_json::json!(false), "{envelope}");
    assert_eq!(
        envelope["diagnostics"][0]["code"],
        serde_json::json!("GHCLI001_ARGUMENT_INVALID"),
        "{envelope}"
    );
}

/// #107: the `synthesize` tool reaches `POST /v1/graphs/synthesize` and relays the route's own
/// reply verbatim — the same `data` the HTTP door returns for the same fixture, which is the
/// same document the CLI writes (`api_http.rs` holds that half).
#[test]
fn the_synthesize_tool_reaches_the_architect_route_and_relays_its_document() {
    let harness = wired("exec-mcp-synthesize");
    let fixture = root_dir().join("core/architect/fixtures/first-compile/replies.json");
    let goal =
        std::fs::read_to_string(root_dir().join("core/architect/fixtures/first-compile/GOAL.txt"))
            .unwrap()
            .trim_end()
            .to_owned();
    let arguments = serde_json::json!({
        "goal": goal,
        "allowPrograms": ["cargo"],
        "fixture": fixture.to_str().unwrap(),
    });

    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(serde_json::json!(2), "synthesize", arguments.clone()),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "{envelope}");
    assert_eq!(envelope["command"], "graph.synthesize");
    assert_eq!(
        envelope["data"]["stampedCustoms"],
        serde_json::json!(["build_check", "summarize"])
    );
    assert_eq!(envelope["data"]["rounds"], 1);

    let (status, direct) = post_json(
        &harness.base,
        &harness.token,
        "/v1/graphs/synthesize",
        &[],
        &arguments,
    );
    assert_eq!(status, 200, "{direct}");
    assert_eq!(
        envelope["data"], direct["data"],
        "the tool must relay the route's own answer, not compose a second one"
    );
}

/// #1123 (spec D10): `judgeFixture` travels through the tool and the route's reply carries the
/// judgments — the same `data` the HTTP door returns for the same two recordings.
#[test]
fn the_synthesize_tool_forwards_the_judge_fixture_and_relays_the_judgments() {
    let harness = wired("exec-mcp-synthesize-judge");
    let fixture = root_dir().join("core/architect/fixtures/first-compile/replies.json");
    let judge = root_dir().join("core/architect/fixtures/judge/nodes-below-threshold.json");
    let goal =
        std::fs::read_to_string(root_dir().join("core/architect/fixtures/first-compile/GOAL.txt"))
            .unwrap()
            .trim_end()
            .to_owned();
    let arguments = serde_json::json!({
        "goal": goal,
        "allowPrograms": ["cargo"],
        "fixture": fixture.to_str().unwrap(),
        "judgeFixture": judge.to_str().unwrap(),
    });

    let session = harness.session(&[
        initialize_request(1, "2025-06-18"),
        initialized_notification(),
        tool_call(serde_json::json!(2), "synthesize", arguments.clone()),
    ]);
    let (is_error, envelope) = tool_envelope(&session.replies[1]);
    assert!(!is_error, "{envelope}");
    assert_eq!(envelope["command"], "graph.synthesize");
    assert_eq!(
        envelope["data"]["judgments"]["unresolved"],
        serde_json::json!(["summarize"]),
        "{envelope}"
    );
    assert_eq!(envelope["data"]["rounds"], 1);

    let (status, direct) = post_json(
        &harness.base,
        &harness.token,
        "/v1/graphs/synthesize",
        &[],
        &arguments,
    );
    assert_eq!(status, 200, "{direct}");
    assert_eq!(
        serde_json::to_vec(&envelope["data"]).unwrap(),
        serde_json::to_vec(&direct["data"]).unwrap(),
        "the tool must relay the route's own answer, judge included"
    );
}
