//! Issue #82: neither `start` nor `resume`'s MCP tool schema exposes a `project` field
//! (`apps/cli/src/commands/mcp/tools.rs`'s `start_schema`/`resume_schema`), so when a request
//! omits it, `drive()` (`apps/cli/src/commands/serve/routes.rs`) falls back to the server
//! process's own working directory — which collides with `--staging` whenever `serve` happens
//! to run from a `--staging` ancestor, an entirely ordinary deployment layout. The second judge
//! story's paid run (2026-08-19) reproduced this 3x, deterministically, calling `resume` through
//! the real graphhelm MCP server with no channel to avoid it.
//!
//! This test reproduces the same collision directly (no MCP client needed — the MCP tool schema
//! is exactly what omits `"project"` from the request body, so a raw HTTP body that also omits
//! it is a faithful stand-in) and proves the fix: a deployer-configured `--project` default
//! resolves the collision without requiring any caller, MCP or otherwise, to know a workspace
//! path at all.
//!
//! Harness copied from `runtime_http.rs` (itself copied from `api_http.rs`) per that file's own
//! note: integration test binaries in this workspace do not share code across files (`apps/cli`
//! is bin-only, no `[lib]` target).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

mod support;
use support::{RawResponse, parse_response, raw_request, split_url};

// -------------------------------------------------------------------------------------------
// Harness
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

/// Extra CLI args appended to `serve` beyond `--events`/`--bind`, and (the addition this test
/// needs beyond `runtime_http.rs`'s own harness) the working directory the server process itself
/// is spawned from — `--staging`'s ancestor-of-cwd collision cannot be reproduced without
/// controlling that cwd explicitly, since a bare `Command::spawn` inherits the test binary's own
/// cwd rather than the tempdir this test builds.
#[derive(Default)]
struct ServeExtra {
    args: Vec<String>,
    cwd: Option<PathBuf>,
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
    if let Some(cwd) = &extra.cwd {
        command.current_dir(cwd);
    }
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

fn get_json(url: &str, token: Option<&str>) -> Value {
    let response =
        raw_request(url, token).unwrap_or_else(|error| panic!("request to {url} failed: {error}"));
    json_body(&response)
}

fn write_text(path: &Path, text: &str) -> PathBuf {
    std::fs::write(path, text).unwrap();
    path.to_owned()
}

/// A minimal one-node Tool graph. Its `tool.call` content is never actually run by this test —
/// the setup server is fixture-only (`FixtureExecutor`, which answers every node by id alone and
/// never touches `program`/`arguments`), and the resume server never gets far enough to dispatch
/// it either way: the workspace-setup step this test is about happens in `drive()` before any
/// node is ever considered.
fn one_tool_node_graph(directory: &Path, execution_id: &str) -> PathBuf {
    let yaml = format!(
        r#"apiVersion: p50.dev/graph/v1
kind: ExecutionGraph
metadata:
  id: exec_issue82_v1
  name: Issue 82 regression
  executionId: {execution_id}
  version: 1
spec:
  entrypoints:
    - only
  nodes:
    only:
      type: tool
      name: Only
      objective: Stand in for any tool node; never actually run in this test.
      optionality: required
      input:  {{ schema: schema://TaskRequest@1 }}
      output: {{ schema: schema://TestReport@1 }}
      tool:
        call:
          tool: shell
          program: git
          arguments: [status]
      completion:
        requires:
          - expression: output.executed > 0
  edges: []
  budgets:
    maxParallelModelCalls: 1
  completion:
    terminalNodes:
      - only
"#
    );
    let path = directory.join("graph.yaml");
    write_text(&path, &yaml)
}

fn gateway_key() -> String {
    "cd".repeat(32)
}

/// Bootstraps the broker/keyring and stores one never-leased dummy credential — mirrors
/// `runtime_http.rs`'s own `credential_set`, needed here only because `serve`'s real-executor
/// group requires `--broker`/`--keyring`/`--key-id` together; the manifest route below is
/// `native_runtime`, which never leases this credential at all (`ServeModelPort::build`'s
/// `NativeRuntime` arm wraps the route with no broker call), and no node in this test's graph
/// ever dispatches far enough to need a model reply regardless.
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
                .write_all(b"never-leased-issue82-test-value")?;
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

// -------------------------------------------------------------------------------------------
// The test
// -------------------------------------------------------------------------------------------

/// Reproduces the paid run's exact collision, then proves the fix.
///
/// Phase 1 (setup, fixture-only, no real-executor wiring at all — `project`/`--staging` are not
/// even in play yet): start the execution with no fixture table, so its one node answers
/// `NeedsInput` (`FixtureExecutor`'s documented absent-fixture convention) and the async drive
/// quiesces immediately with nothing dispatchable; `pause` it, reaching the paused state `resume`
/// requires.
///
/// Phase 2 (the regression, real-executor wiring, `--staging` a genuine subdirectory of the
/// server's OWN working directory — the ordinary layout the issue names): a fresh `serve`,
/// pointed at the same events directory, spawned with its cwd deliberately set to that ancestor.
/// `resume` is called with NO `"project"` field — the exact shape any MCP tool call produces,
/// since neither `start_schema` nor `resume_schema` exposes one. `--project` here is set to a
/// sibling of `--staging`, never overlapping it — the deployer-side fix from issue #82.
///
/// # Failure mode before the fix
/// Before `RuntimeWiring` carried `project` and `drive()`'s fallback chain consulted it, this
/// same phase-2 call had no way to avoid `project` defaulting to the server's cwd (a `--staging`
/// ancestor by construction here) and returned 500 `GHCLI016_DRIVER_FAILURE`: "workspace
/// configuration refused: the staging area must not overlap the project" — reproduced 3x,
/// identically, by the real judge in the second judge story's paid run (2026-08-19).
#[test]
fn resume_without_project_succeeds_when_the_deployer_configured_a_default() {
    let deadline = Instant::now() + Duration::from_secs(30);

    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let execution = "exec-issue82-regression";
    let graph = one_tool_node_graph(directory.path(), execution);

    // Phase 1: fixture-only setup, no real-executor flags at all.
    {
        let (_guard, base, token) = serve_with(&events, &ServeExtra::default());

        let start_url = format!("{base}/v1/executions/{execution}/start");
        let start_body = serde_json::json!({
            "file": graph.to_str().unwrap(),
            "mode": "autopilot",
        });
        let headers = [
            ("Idempotency-Key", "issue82-start"),
            ("X-GraphHelm-Actor", "agent-issue82"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ];
        let (status, reply) = post_json(&start_url, &token, &headers, &start_body);
        assert_eq!(status, 200, "phase 1 start: {reply}");
        assert_eq!(
            reply["data"]["nodeStateCounts"]["waiting_input"], 1,
            "the one node has no fixture entry and must park on NeedsInput, \
             proving the drive quiesced without needing any real work: {reply}"
        );

        let pause_url = format!("{base}/v1/executions/{execution}/pause");
        let (status, reply) = post_json(
            &pause_url,
            &token,
            &[
                ("Idempotency-Key", "issue82-pause"),
                ("X-GraphHelm-Actor", "agent-issue82"),
                ("X-GraphHelm-Actor-Type", "agent"),
            ],
            &Value::Null,
        );
        assert_eq!(status, 200, "phase 1 pause: {reply}");
        assert_eq!(reply["data"]["status"], "paused", "{reply}");
    } // server A dropped/killed here — its cwd/staging never enters the picture again.

    assert!(
        Instant::now() < deadline,
        "phase 1 took too long; aborting before phase 2 to fail fast"
    );

    // Phase 2: real-executor wiring, spawned with cwd = staging's own parent.
    let cwd_root = directory.path().join("server-b-cwd");
    let staging = cwd_root.join("staging"); // a genuine subdirectory of the server's own cwd
    let project_default = directory.path().join("project-default"); // a sibling, never nested
    std::fs::create_dir_all(&cwd_root).unwrap();
    std::fs::create_dir_all(&project_default).unwrap();
    let broker = directory.path().join("broker");
    let keyring = directory.path().join("keyring");
    std::fs::create_dir_all(&keyring).unwrap();
    let key_id = "issue82-key";
    let route_id = "issue82_native_route";

    credential_set(&broker, &keyring, key_id, "cred_issue82", route_id);

    let manifest = write_text(
        &directory.path().join("manifest.json"),
        &serde_json::to_string(&serde_json::json!({
            "manifestVersion": 1,
            "routes": [{
                "id": route_id,
                "provider": "anthropic",
                "transport": "native_runtime",
                "runtime": "claude_code",
                "authentication": "account_subscription",
                "billingMode": "subscription_quota",
                // Never actually spawned: this test's graph has no Cognitive node, so nothing
                // ever calls this route's own command.
                "command": { "program": "graphhelm-issue82-placeholder", "args": [] },
                "profiles": ["software_execution"],
                "enabled": true
            }]
        }))
        .unwrap(),
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
            "--route".into(),
            route_id.into(),
            "--staging".into(),
            staging.to_str().unwrap().into(),
            "--allow-program".into(),
            "git".into(),
            "--allow-program".into(),
            "cargo".into(),
            // The fix under test: the deployer's own default, set once, never overlapping
            // --staging. Remove this one pair of args (with nothing else changed) to observe the
            // pre-fix collision this test replaces as the project's strongest available red.
            "--project".into(),
            project_default.to_str().unwrap().into(),
        ],
        cwd: Some(cwd_root.clone()),
        // Both keys needed, matching `runtime_http.rs`'s own note: `GRAPHHELM_GATEWAY_KEY` for
        // the credential broker (unused by `native_runtime` specifically, but set anyway to
        // match the established pattern) and `GRAPHHELM_EVENTS_KEY` for the driver's own
        // evidence sealer (`ports::build_sealer`, reused eagerly by `drive` before any
        // dispatch — the actual reason this test needs it at all) — same value as
        // `credential_set` used, since both open the same `SealedKeyProvider` location.
        env: vec![
            ("GRAPHHELM_GATEWAY_KEY".to_owned(), gateway_key()),
            ("GRAPHHELM_EVENTS_KEY".to_owned(), gateway_key()),
        ],
    };
    let (_guard, base, token) = serve_with(&events, &extra);

    let resume_url = format!("{base}/v1/executions/{execution}/resume");
    // No "project" field — the exact body shape neither `start_schema` nor `resume_schema`
    // (`apps/cli/src/commands/mcp/tools.rs`) can ever produce, since neither exposes the field.
    let resume_body = serde_json::json!({
        "file": graph.to_str().unwrap(),
    });
    let (status, reply) = post_json(
        &resume_url,
        &token,
        &[
            ("Idempotency-Key", "issue82-resume"),
            ("X-GraphHelm-Actor", "agent-issue82"),
            ("X-GraphHelm-Actor-Type", "agent"),
        ],
        &resume_body,
    );
    assert_eq!(
        status, 200,
        "resume must succeed once the deployer's --project default routes around the CWD/staging \
         collision, with no \"project\" field in the request at all — a 500 here means the fix did \
         not take: {reply}"
    );
    assert_ne!(
        reply.get("data"),
        None,
        "a successful resume always carries a data payload: {reply}"
    );

    // The workspace-setup step this test is about happens before any node's own state matters,
    // but confirm the execution is still coherent (not silently corrupted) as a sanity floor.
    let status_url = format!("{base}/v1/executions/{execution}");
    let final_status = get_json(&status_url, Some(&token));
    assert_eq!(
        final_status["data"]["nodeStateCounts"]["waiting_input"], 1,
        "the parked node's state must survive the resume unchanged, since nothing in this \
         story's world ever satisfies its NeedsInput: {final_status}"
    );
}
