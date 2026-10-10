//! #353: the owner reviews and approves journey-flow drafts from the Studio, so the flow list and
//! the approval exist identically on the CLI (`journey flows`, `journey approve`), over HTTP
//! (`GET /v1/journey-flows`, `POST /v1/journey-flows/{id}/approve`) and over MCP
//! (`journey_flows`, `journey_approve`). Credible regressions: a route that approves a flow the
//! CLI would refuse, a listing that hides validate findings or a stale approval, and an agent
//! credential that can approve. Cost: one `serve` subprocess plus CLI/MCP subprocesses on
//! tempdirs; no network beyond loopback, no browser, model or credentials; seconds after the build.

#[path = "support/time_scale.rs"]
mod time_scale;
use time_scale::scaled;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const EVENTS_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const KEY_ID: &str = "owner-key";
const RUN: &str = "flows-surfaces";
const AGENT_CREDENTIAL: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const EXAMPLE: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");

fn graphhelm() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env("GRAPHHELM_EVENTS_KEY", EVENTS_KEY);
    command
}

fn git(project: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(project)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct ServerGuard(Child);
impl Drop for ServerGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Harness {
    scratch: tempfile::TempDir,
    events: PathBuf,
    keyring: PathBuf,
    project: PathBuf,
}

/// Three flows in one committed SHA-1 project: `basket` and `checkout` are clean drafts, `broken`
/// names a scope path that does not exist (a validate finding that must block approval).
fn prepared() -> Harness {
    let scratch = tempfile::tempdir().unwrap();
    let events = scratch.path().join("runtime-data");
    let keyring = scratch.path().join("keyring");
    std::fs::create_dir(&keyring).unwrap();
    graphhelm_sealed_key_provider::SealedKeyProvider::create(
        &keyring,
        KEY_ID,
        graphhelm_events::SecretBytes::new(vec![1; 32]),
    )
    .unwrap();
    let fixtures = scratch.path().join("fixtures.json");
    std::fs::write(&fixtures, br#"{"nodeOutcomes":{}}"#).unwrap();
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let output = graphhelm()
        .args(["execution", "start", "--events"])
        .arg(&events)
        .args(["--execution", RUN, "--file"])
        .arg(&graph)
        .arg("--fixtures")
        .arg(&fixtures)
        .args(["--mode", "manual", "--held"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );

    let project = scratch.path().join("project");
    for file in [
        "app/cart/page.tsx",
        "app/checkout/page.tsx",
        "app/api/pay/route.ts",
    ] {
        let path = project.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "export {}").unwrap();
    }
    let journeys = project.join(".graphhelm/journeys");
    std::fs::create_dir_all(&journeys).unwrap();
    std::fs::write(journeys.join("checkout.journey.yaml"), EXAMPLE).unwrap();
    std::fs::write(
        journeys.join("basket.journey.yaml"),
        EXAMPLE.replace("id: checkout", "id: basket"),
    )
    .unwrap();
    std::fs::write(
        journeys.join("broken.journey.yaml"),
        EXAMPLE
            .replace("id: checkout", "id: broken")
            .replace("scope: [app/cart/page.tsx]", "scope: [app/gone.tsx]"),
    )
    .unwrap();
    git(&project, &["init", "-q", "--object-format=sha1"]);
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "init"]);
    owner_token(&project);
    Harness {
        scratch,
        events,
        keyring,
        project,
    }
}

impl Harness {
    fn flow_text(&self, id: &str) -> String {
        std::fs::read_to_string(
            self.project
                .join(format!(".graphhelm/journeys/{id}.journey.yaml")),
        )
        .unwrap()
    }

    fn cli(&self, args: &[&str]) -> (Option<i32>, Value) {
        let output = graphhelm()
            .args(["--json", "journey"])
            .args(args)
            .arg("--project")
            .arg(&self.project)
            .output()
            .unwrap();
        (
            output.status.code(),
            serde_json::from_slice(&output.stdout).unwrap_or(Value::Null),
        )
    }

    fn flows(&self) -> Value {
        let (code, envelope) = self.cli(&["flows"]);
        assert_eq!(code, Some(0), "{envelope}");
        assert_eq!(envelope["command"], "journey.flows");
        envelope["data"].clone()
    }

    fn serve(&self, project: bool) -> (ServerGuard, String, String) {
        let events = self.events.clone();
        self.serve_with(&events, project)
    }

    /// The Runtime on a given store; #534's cells serve the project's own owner store, as a real
    /// Runtime started from the project does.
    fn serve_with(&self, events: &Path, project: bool) -> (ServerGuard, String, String) {
        let binding = format!("{AGENT_CREDENTIAL}=agent-planner|project-local|{RUN}");
        let mut command = graphhelm();
        command
            .args(["serve", "--events"])
            .arg(events)
            .args(["--bind", "127.0.0.1:0", "--keyring"])
            .arg(&self.keyring)
            .args(["--key-id", KEY_ID]);
        if project {
            command.arg("--project").arg(&self.project);
        }
        let mut child = command
            .env("GRAPHHELM_AGENT_CREDENTIALS", binding)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let guard = ServerGuard(child);
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let started: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(started["command"], "serve.started", "{started}");
        let base = format!("http://{}", started["data"]["address"].as_str().unwrap());
        let mut name = events.file_name().unwrap().to_os_string();
        name.push(".token");
        let token = std::fs::read_to_string(events.with_file_name(name))
            .unwrap()
            .trim()
            .to_owned();
        let deadline = Instant::now() + scaled(Duration::from_secs(10));
        while http(&base, "GET", "/health", None).0 != 200 {
            assert!(Instant::now() < deadline, "the server never became healthy");
            std::thread::sleep(Duration::from_millis(50));
        }
        (guard, base, token)
    }

    /// One MCP tool call as an `agent`-typed session (how every lane runs).
    fn mcp(&self, base: &str, token: &str, tool: &str, arguments: &Value) -> (bool, Value) {
        self.mcp_as("agent", base, token, tool, arguments)
    }

    /// One MCP tool call; a JSON-RPC error comes back as `(false, <the whole reply>)`.
    fn mcp_as(
        &self,
        actor_type: &str,
        base: &str,
        token: &str,
        tool: &str,
        arguments: &Value,
    ) -> (bool, Value) {
        let token_file = self.scratch.path().join("mcp-token");
        std::fs::write(&token_file, token).unwrap();
        let lines = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"conformance","version":"0"}}}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
                "params":{"name":tool,"arguments": arguments}}),
        ];
        let mut input = String::new();
        for line in &lines {
            input.push_str(&line.to_string());
            input.push('\n');
        }
        let output = assert_cmd::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(["mcp", "--url", base, "--token-file"])
            .arg(&token_file)
            .args(["--actor", "agent-chat", "--actor-type", actor_type])
            .write_stdin(input)
            .timeout(scaled(Duration::from_secs(60)))
            .output()
            .unwrap();
        let reply = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|value| value["id"] == 2)
            .expect("a reply to the tool call");
        if reply.get("error").is_some() {
            return (false, reply);
        }
        let text = reply["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no tool text: {reply}"));
        (
            reply["result"]["isError"] == false,
            serde_json::from_str(text).unwrap(),
        )
    }
}

fn http(base: &str, method: &str, path: &str, bearer: Option<&str>) -> (u16, Value) {
    http_body(base, method, path, bearer, "")
}

fn http_body(
    base: &str,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: &str,
) -> (u16, Value) {
    let address = base.strip_prefix("http://").unwrap();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    if !body.is_empty() {
        head.push_str("Content-Type: application/json\r\n");
    }
    if let Some(bearer) = bearer {
        head.push_str(&format!("Authorization: Bearer {bearer}\r\n"));
    }
    head.push_str("\r\n");
    head.push_str(body);
    let Ok(mut stream) = TcpStream::connect(address) else {
        return (0, Value::Null);
    };
    stream
        .set_read_timeout(Some(scaled(Duration::from_secs(60))))
        .unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap();
    let mut body = raw[split + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let line_end = raw.windows(2).position(|w| w == b"\r\n").unwrap();
        let size_text = String::from_utf8_lossy(&raw[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap().trim(), 16).unwrap();
        raw = &raw[line_end + 2..];
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&raw[..size]);
        raw = &raw[size + 2..];
    }
}

fn flow<'a>(data: &'a Value, id: &str) -> &'a Value {
    data["flows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|flow| flow["id"] == id)
        .unwrap_or_else(|| panic!("no flow {id}: {data}"))
}

fn codes(flow: &Value) -> Vec<String> {
    flow["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn flows_list_status_drift_findings_and_graph_identically_on_cli_http_and_mcp() {
    let harness = prepared();
    let cli = harness.flows();
    let ids: Vec<_> = cli["flows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].clone())
        .collect();
    assert_eq!(ids, vec!["basket", "broken", "checkout"], "{cli}");
    let checkout = flow(&cli, "checkout");
    assert_eq!(checkout["status"], "draft");
    assert_eq!(checkout["title"], "Shopper pays for the cart");
    assert_eq!(checkout["drift"], json!([]));
    assert_eq!(checkout["findings"], json!([]));
    assert_eq!(checkout["approvable"], true);
    assert_eq!(
        checkout["paths"]["main"],
        json!(["cart.checkout", "pay.submit"])
    );
    assert_eq!(checkout["screens"].as_array().unwrap().len(), 3, "{cli}");
    assert_eq!(checkout["edges"][0]["from"], "cart");
    assert_eq!(checkout["edges"][0]["to"], "pay");
    let broken = flow(&cli, "broken");
    assert_eq!(codes(broken), vec!["flow.scope_path_missing"], "{cli}");
    assert_eq!(broken["approvable"], false);

    let (_server, base, token) = harness.serve(true);
    let (status, over_http) = http(&base, "GET", "/v1/journey-flows", Some(&token));
    assert_eq!(status, 200, "{over_http}");
    assert_eq!(over_http["command"], "journey.flows");
    assert_eq!(over_http["data"], cli, "HTTP differs from the CLI");
    let (ok, over_mcp) = harness.mcp(&base, &token, "journey_flows", &json!({}));
    assert!(ok, "{over_mcp}");
    assert_eq!(over_mcp["data"], cli, "MCP differs from the CLI");

    // Owner-only: a scoped agent credential is refused.
    let (status, _) = http(&base, "GET", "/v1/journey-flows", Some(AGENT_CREDENTIAL));
    assert_eq!(status, 401);
}

#[test]
fn approve_over_http_and_mcp_does_what_the_cli_does_and_refuses_findings() {
    let harness = prepared();
    let (_server, base, token) = harness.serve(true);

    // A flow with validate findings is refused, names the code, and nothing is written.
    let before = harness.flow_text("broken");
    let (status, refused) = http(
        &base,
        "POST",
        "/v1/journey-flows/broken/approve",
        Some(&token),
    );
    assert_eq!(status, 400, "{refused}");
    assert_eq!(refused["command"], "journey.approve");
    assert_eq!(
        refused["data"]["files"][0]["findings"][0]["code"], "flow.scope_path_missing",
        "{refused}"
    );
    assert_eq!(harness.flow_text("broken"), before);

    // Owner-only and id-checked.
    let (status, _) = http(
        &base,
        "POST",
        "/v1/journey-flows/checkout/approve",
        Some(AGENT_CREDENTIAL),
    );
    assert_eq!(status, 401);
    let (status, _) = http(
        &base,
        "POST",
        "/v1/journey-flows/..%2Fx/approve",
        Some(&token),
    );
    assert_eq!(status, 400);
    assert_eq!(flow(&harness.flows(), "checkout")["status"], "draft");

    // HTTP approve == CLI approve: the flow is approved, bound, and its contract compiled.
    let (status, approved) = http(
        &base,
        "POST",
        "/v1/journey-flows/checkout/approve",
        Some(&token),
    );
    assert_eq!(status, 200, "{approved}");
    assert_eq!(approved["command"], "journey.approve");
    assert_eq!(approved["data"]["status"], "approved");
    let http_flow = harness.flow_text("checkout");
    assert!(
        harness
            .project
            .join(".graphhelm/journeys/checkout.json")
            .exists()
    );
    let owner = owner_token(&harness.project);
    let (code, cli_again) = harness.cli(&["approve", "checkout", "--token-file", &owner]);
    assert_eq!(code, Some(0), "{cli_again}");
    assert_eq!(cli_again["data"]["approved"], approved["data"]["approved"]);
    assert_eq!(harness.flow_text("checkout"), http_flow);

    // Only the owner approves: an agent-typed MCP session is refused with a stable code before
    // any request, even holding the owner token, and nothing is written.
    let basket = harness.flow_text("basket");
    let (ok, refused) = harness.mcp(&base, &token, "journey_approve", &json!({"id": "basket"}));
    assert!(!ok, "{refused}");
    assert_eq!(
        refused["error"]["data"]["code"], "GHCLI036_JOURNEY_APPROVE_OWNER_ONLY",
        "{refused}"
    );
    assert_eq!(harness.flow_text("basket"), basket);
    assert_eq!(flow(&harness.flows(), "basket")["status"], "draft");

    // An owner-typed MCP session reaches the same path as the CLI.
    let (ok, over_mcp) = harness.mcp_as(
        "owner",
        &base,
        &token,
        "journey_approve",
        &json!({"id": "basket"}),
    );
    assert!(ok, "{over_mcp}");
    assert_eq!(over_mcp["data"]["status"], "approved");
    let listed = harness.flows();
    assert_eq!(flow(&listed, "basket")["status"], "approved");
    assert_eq!(flow(&listed, "checkout")["status"], "approved");
    assert_eq!(flow(&listed, "checkout")["approvable"], false);

    // Editing an approved flow without re-approving shows the stale approval.
    std::fs::write(
        harness
            .project
            .join(".graphhelm/journeys/checkout.journey.yaml"),
        http_flow.replace("Shopper pays for the cart", "Shopper pays"),
    )
    .unwrap();
    let stale = flow(&harness.flows(), "checkout").clone();
    assert_eq!(stale["status"], "approval_stale", "{stale}");
    assert!(codes(&stale).contains(&"flow.approval_stale".to_owned()));
    assert_eq!(stale["approvable"], true, "{stale}");
}

/// #380: the declared actor type is not a credential. The Runtime's agent session token
/// (`events.agent.token`, minted beside the owner token) cannot approve a flow over HTTP or
/// through an MCP session that declares itself `owner`; nothing is written.
#[test]
fn an_agent_session_token_cannot_approve_whatever_type_it_declares() {
    let harness = prepared();
    let (_server, base, _owner) = harness.serve(true);
    let agent_token = std::fs::read_to_string(harness.events.with_extension("agent.token"))
        .expect("serve mints the agent session token beside the owner token");
    let agent_token = agent_token.trim();
    let before = harness.flow_text("checkout");

    let (status, refused) = http(
        &base,
        "POST",
        "/v1/journey-flows/checkout/approve",
        Some(agent_token),
    );
    assert_eq!(status, 403, "{refused}");
    let (ok, over_mcp) = harness.mcp_as(
        "owner",
        &base,
        agent_token,
        "journey_approve",
        &json!({"id": "checkout"}),
    );
    assert!(
        !ok,
        "a self-declared owner holding the agent token approved: {over_mcp}"
    );
    assert_eq!(harness.flow_text("checkout"), before);
    assert_eq!(flow(&harness.flows(), "checkout")["status"], "draft");

    // The same token still reads what agents read.
    let (status, listed) = http(&base, "GET", "/v1/journey-flows", Some(agent_token));
    assert_eq!(status, 200, "{listed}");
    let (status, run) = http(
        &base,
        "GET",
        &format!("/v1/executions/{RUN}"),
        Some(agent_token),
    );
    assert_eq!(status, 200, "{run}");

    // Not only the named owner actions: everything outside the agent allow-list is the owner's,
    // so starting, approving or cancelling a run and rewriting the gateway routes are refused
    // before any handler, and the run is untouched.
    for (method, path) in [
        ("POST", format!("/v1/executions/{RUN}/approve")),
        ("POST", format!("/v1/executions/{RUN}/cancel")),
        ("POST", format!("/v1/executions/{RUN}/start")),
        ("PUT", "/v1/gateway/routes".to_owned()),
    ] {
        let (status, refused) = http(&base, method, &path, Some(agent_token));
        assert_eq!(status, 403, "{method} {path}: {refused}");
    }
    let (_, after) = http(
        &base,
        "GET",
        &format!("/v1/executions/{RUN}"),
        Some(agent_token),
    );
    assert_eq!(after["data"]["status"], run["data"]["status"], "{after}");
}

/// #518 (`keel.invariant.permissions`): marking an edge safe is the owner's. Defect named: the
/// route added to the agent allow-list, or served without the owner check, so the agent that wrote
/// a draft's destructive act also blesses it. The agent session token is refused before any
/// handler and nothing is written; the owner credential writes the mark. Cost: one server, three
/// requests.
#[test]
fn only_the_owner_credential_marks_an_edge_safe() {
    let harness = prepared();
    let (_server, base, owner) = harness.serve(true);
    let agent_token = std::fs::read_to_string(harness.events.with_extension("agent.token"))
        .expect("serve mints the agent session token beside the owner token");
    let before = harness.flow_text("checkout");
    const ROUTE: &str = "/v1/journey-flows/checkout/edges/pay.submit/safe";

    let (status, refused) = http(&base, "POST", ROUTE, Some(agent_token.trim()));
    assert_eq!(status, 403, "{refused}");
    let (status, refused) = http(&base, "POST", ROUTE, Some(AGENT_CREDENTIAL));
    assert_ne!(status, 200, "{refused}");
    assert_eq!(harness.flow_text("checkout"), before);

    let (status, marked) = http(&base, "POST", ROUTE, Some(&owner));
    assert_eq!(status, 200, "{marked}");
    let digest = marked["data"]["safe"]["digest"]
        .as_str()
        .unwrap_or_default();
    assert!(digest.starts_with("sha256:"), "{marked}");
    assert!(
        harness
            .flow_text("checkout")
            .contains(&format!("safe: {{digest: {digest}}}\n")),
        "{}",
        harness.flow_text("checkout")
    );
    // The review list carries the mark, so the Studio can show the step as marked.
    let listed = flow(&harness.flows(), "checkout").clone();
    assert!(
        listed["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["id"] == "pay.submit" && edge["safe"]["digest"] == digest),
        "{listed}"
    );
}

#[test]
fn the_flow_routes_without_a_project_refuse_naming_project() {
    let harness = prepared();
    let (_server, base, token) = harness.serve(false);
    for (method, path) in [
        ("GET", "/v1/journey-flows"),
        ("POST", "/v1/journey-flows/checkout/approve"),
    ] {
        let (status, body) = http(&base, method, path, Some(&token));
        assert_eq!(status, 409, "{method} {path}: {body}");
        assert_eq!(body["diagnostics"][0]["path"], "/project", "{body}");
    }
}

/// #427: a malformed `POST /v1/keel/plan` body is an argument error (400 `GHCLI001`) at the field
/// that broke it, like every other route, not a 409 execution-state refusal. Credible regression:
/// the route goes back to one catch-all 409. Cost: one server, five requests.
#[test]
fn a_malformed_keel_plan_body_is_a_400_naming_the_field() {
    let harness = prepared();
    let (_server, base, token) = harness.serve(true);
    let post = |body: &str| http_body(&base, "POST", "/v1/keel/plan", Some(&token), body);
    let (status, reply) = post(r#"{"task":"t-1","paths":["app/cart/page.tsx"]}"#);
    assert_eq!(status, 200, "{reply}");
    for (body, pointer) in [
        (r#"{"task":"t-1","paths":["a"],"repo":"C:/"}"#, "/body/repo"),
        (r#"{"paths":["a"]}"#, "/body/task"),
        (r#"{"task":"t-1","paths":"a"}"#, "/body/paths"),
        ("not json", "/body"),
    ] {
        let (status, reply) = post(body);
        assert_eq!(status, 400, "{body}: {reply}");
        let diagnostic = &reply["diagnostics"][0];
        assert_eq!(diagnostic["code"], "GHCLI001_ARGUMENT_INVALID", "{reply}");
        assert_eq!(diagnostic["path"], pointer, "{body}: {reply}");
    }
}

/// #534: approving is the owner's; `graphhelm init` makes the project's owner store and token.
fn owner_token(project: &Path) -> String {
    // init ignores all of `.graphhelm/`; a real project keeps its flows tracked and its owner
    // store (events, token, keys, owner records) out of git, so the test ignores exactly that.
    let gitignore = project.join(".gitignore");
    let kept = std::fs::read(&gitignore).ok();
    let out = std::process::Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "init", "--project"])
        .arg(project)
        .args(["--harness", "claude-code"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let mut ignore = kept.unwrap_or_default();
    ignore.extend_from_slice(
        b"
.graphhelm/*
!.graphhelm/journeys/
/.mcp.json
",
    );
    std::fs::write(&gitignore, ignore).unwrap();
    project
        .join(".graphhelm/events.token")
        .to_string_lossy()
        .into_owned()
}

/// #569 review: the agent session token could post a signal of another kind into the reserved
/// owner execution under an approval's id, so the owner's later approval collided on it and failed:
/// an agent could block any flow's approval. Admission now refuses every non-owner signal into
/// `graphhelm-owner` and every other kind under a `journey-approved-` id, and the owner's record
/// uses a key the agent cannot compute. Cost: one Runtime on the project's own store, seconds.
#[test]
fn an_agent_cannot_squat_the_owner_record_execution_and_the_owner_still_approves() {
    let harness = prepared();
    let events = harness.project.join(".graphhelm/events");
    let (_server, base, owner) = harness.serve_with(&events, true);
    // The owner's first approval starts the reserved execution.
    let (status, first) = http(
        &base,
        "POST",
        "/v1/journey-flows/basket/approve",
        Some(&owner),
    );
    assert_eq!(status, 200, "{first}");
    let agent = std::fs::read_to_string(events.with_extension("agent.token")).unwrap();
    let agent = agent.trim();
    // The id the owner's approval of `checkout` will use: the digest depends only on the flow, so
    // approving an identical copy of the project tells the test (and an attacker) what it is.
    let twin = prepared();
    let twin_owner = owner_token(&twin.project);
    let (code, twin_approved) = twin.cli(&["approve", "checkout", "--token-file", &twin_owner]);
    assert_eq!(code, Some(0), "{twin_approved}");
    let digest = twin_approved["data"]["approved"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let wanted = {
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(format!("checkout\n{digest}").as_bytes());
        format!("journey-approved-{}", &hex::encode(hash)[..32])
    };
    // A squat sent exactly as a lane sends a note (`source.type` user), under that id and under a
    // plain id: both refused by the owner-execution rule itself.
    let squats: Vec<(u16, Value)> = [wanted.as_str(), "agent-note-into-owner"]
        .into_iter()
        .map(|id| {
            let body = json!({"signal": {"id": id, "type": "operator_note", "severity": "low",
                "source": {"type": "user", "id": "agent-chat"}, "description": "squat",
                "evidence": ["squat"], "emittedAt": "2026-10-09T00:00:00Z"}})
            .to_string();
            signal_as_agent(&base, agent, id, &body)
        })
        .collect();
    // The owner then approves the flow whose id was squatted. Its outcome is in every message
    // below, so a run with the guard off shows what the squat did to the owner.
    let (status, approved) = http(
        &base,
        "POST",
        "/v1/journey-flows/checkout/approve",
        Some(&owner),
    );
    for (squat_status, reply) in &squats {
        assert!(
            *squat_status != 200 && reply.to_string().contains("owner record execution"),
            "an agent's squat was not refused by the owner-execution rule: {squat_status} {reply}; \
             the owner's approval afterwards: {status} {approved}"
        );
    }
    assert_eq!(status, 200, "{approved}");
    let (_, validated) = harness.cli(&["validate", "--all"]);
    assert!(
        !validated.to_string().contains("flow.approval_unsigned"),
        "{validated}"
    );
}

/// A signal POST exactly as a lane sends it: idempotency key and actor headers included, so a
/// refusal is the admission's own answer.
fn signal_as_agent(base: &str, bearer: &str, key: &str, body: &str) -> (u16, Value) {
    let address = base.strip_prefix("http://").unwrap();
    let head = [
        "POST /v1/executions/graphhelm-owner/signal HTTP/1.1".to_owned(),
        format!("Host: {address}"),
        "Connection: close".to_owned(),
        format!("Content-Length: {}", body.len()),
        "Content-Type: application/json".to_owned(),
        format!("Authorization: Bearer {bearer}"),
        format!("Idempotency-Key: {key}"),
        "X-GraphHelm-Actor: agent-chat".to_owned(),
        "X-GraphHelm-Actor-Type: agent".to_owned(),
    ]
    .join("\r\n");
    let request = format!("{head}\r\n\r\n{body}");
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(scaled(Duration::from_secs(60))))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let status = String::from_utf8_lossy(&raw[..split])
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    (
        status,
        serde_json::from_slice(&raw[split + 4..]).unwrap_or(Value::Null),
    )
}

/// #519 (`keel.invariant.permissions`, `external effects`): the Studio's observer setup edits the
/// owner's package.json and downloads Chromium, so it is the owner's alone and takes no caller
/// input. Defect named: the route added to the agent allow-list, or a body passed through to npm.
/// The agent session token is refused before any handler; a body is refused by the owner's own
/// request; neither writes a file. The last outcome is also owner-only (#627); a fresh Runtime
/// answers none. Cost: one server, five requests, no install.
#[test]
fn only_the_owner_sets_up_the_observer_and_nothing_reaches_npm() {
    let harness = prepared();
    let (_server, base, owner) = harness.serve(true);
    let agent_token = std::fs::read_to_string(harness.events.with_extension("agent.token"))
        .expect("serve mints the agent session token beside the owner token");
    const ROUTE: &str = "/v1/journey-observer/setup";
    let untouched = || {
        assert!(!harness.project.join("package.json").exists());
        assert!(
            !harness
                .project
                .join(".graphhelm/observers/journey_driver.mjs")
                .exists()
        );
    };

    let (status, record) = http(&base, "GET", ROUTE, Some(&owner));
    assert_eq!(status, 200, "{record}");
    assert_eq!(record["data"]["state"], "none");
    let (status, refused) = http(&base, "GET", ROUTE, Some(agent_token.trim()));
    assert_eq!(status, 403, "{refused}");

    let (status, refused) = http(&base, "POST", ROUTE, Some(agent_token.trim()));
    assert_eq!(status, 403, "{refused}");
    let (status, refused) = http(&base, "POST", ROUTE, Some(AGENT_CREDENTIAL));
    assert_ne!(status, 200, "{refused}");
    untouched();

    let (status, refused) = http_body(&base, "POST", ROUTE, Some(&owner), r#"{"package":"evil"}"#);
    assert_eq!(status, 400, "{refused}");
    untouched();
}
