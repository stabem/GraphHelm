//! #353: the owner reviews and approves journey-flow drafts from the Studio, so the flow list and
//! the approval exist identically on the CLI (`journey flows`, `journey approve`), over HTTP
//! (`GET /v1/journey-flows`, `POST /v1/journey-flows/{id}/approve`) and over MCP
//! (`journey_flows`, `journey_approve`). Credible regressions: a route that approves a flow the
//! CLI would refuse, a listing that hides validate findings or a stale approval, and an agent
//! credential that can approve. Cost: one `serve` subprocess plus CLI/MCP subprocesses on
//! tempdirs; no network beyond loopback, no browser, model or credentials; seconds after the build.

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
        let binding = format!("{AGENT_CREDENTIAL}=agent-planner|project-local|{RUN}");
        let mut command = graphhelm();
        command
            .args(["serve", "--events"])
            .arg(&self.events)
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
        let mut name = self.events.file_name().unwrap().to_os_string();
        name.push(".token");
        let token = std::fs::read_to_string(self.events.with_file_name(name))
            .unwrap()
            .trim()
            .to_owned();
        let deadline = Instant::now() + Duration::from_secs(10);
        while http(&base, "GET", "/health", None).0 != 200 {
            assert!(Instant::now() < deadline, "the server never became healthy");
            std::thread::sleep(Duration::from_millis(50));
        }
        (guard, base, token)
    }

    fn mcp(&self, base: &str, token: &str, tool: &str, arguments: &Value) -> (bool, Value) {
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
            .args(["--actor", "agent-chat", "--actor-type", "agent"])
            .write_stdin(input)
            .timeout(Duration::from_secs(60))
            .output()
            .unwrap();
        let reply = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .find(|value| value["id"] == 2)
            .expect("a reply to the tool call");
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
    let address = base.strip_prefix("http://").unwrap();
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: 0\r\n"
    );
    if let Some(bearer) = bearer {
        head.push_str(&format!("Authorization: Bearer {bearer}\r\n"));
    }
    head.push_str("\r\n");
    let Ok(mut stream) = TcpStream::connect(address) else {
        return (0, Value::Null);
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
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
    let (code, cli_again) = harness.cli(&["approve", "checkout"]);
    assert_eq!(code, Some(0), "{cli_again}");
    assert_eq!(cli_again["data"]["approved"], approved["data"]["approved"]);
    assert_eq!(harness.flow_text("checkout"), http_flow);

    // MCP approve reaches the same path.
    let (ok, over_mcp) = harness.mcp(&base, &token, "journey_approve", &json!({"id": "basket"}));
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
