//! #353: the owner reviews and approves journey-flow drafts from the Studio, so the flow list and
//! the approval exist identically on the CLI (`journey flows`, `journey approve`), over HTTP
//! (`GET /v1/journey-flows`, `POST /v1/journey-flows/{id}/approve`) and over MCP
//! (`journey_flows`, `journey_approve`). Credible regressions: a route that approves a flow the
//! CLI would refuse, a listing that hides validate findings or a stale approval, and an agent
//! credential that can approve. Cost: one `serve` subprocess plus CLI/MCP subprocesses on
//! tempdirs; no network beyond loopback, no browser, model or credentials; seconds after the build.

#[path = "support/mod.rs"]
mod support;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use support::owner_token;

#[path = "support/time_scale.rs"]
mod time_scale;
use time_scale::scaled;

use serde_json::{Value, json};

const EVENTS_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const KEY_ID: &str = "owner-key";
const RUN: &str = "flows-surfaces";
const AGENT_CREDENTIAL: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
/// #1130: a second scoped lane, the blind reviewer.
const REVIEWER_CREDENTIAL: &str =
    "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
/// #1130 follow-up: a third lane that tries to claim another lane's draft.
const SQUATTER_CREDENTIAL: &str =
    "aaaabbbbccccddddaaaabbbbccccddddaaaabbbbccccddddaaaabbbbccccdddd";
const EXAMPLE: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");

fn graphhelm() -> Command {
    let mut command = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env("GRAPHHELM_EVENTS_KEY", EVENTS_KEY);
    command
}

fn git(project: &Path, args: &[&str]) {
    let output = graphhelm_process::command("git")
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
    // #1309 ruling A: the owner approves only what a preview showed passing.
    for id in ["checkout", "basket"] {
        support::passing_preview(&project, id);
    }
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
        self.serve_with_audit(events, project, None)
    }

    /// The Runtime, optionally with `--read-audit` writing every response body to `audit`.
    fn serve_with_audit(
        &self,
        events: &Path,
        project: bool,
        audit: Option<&Path>,
    ) -> (ServerGuard, String, String) {
        let binding = format!(
            "{AGENT_CREDENTIAL}=agent-planner|project-local|{RUN};{REVIEWER_CREDENTIAL}=agent-reviewer|project-local|{RUN};{SQUATTER_CREDENTIAL}=agent-squatter|project-local|{RUN}"
        );
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
        if let Some(audit) = audit {
            command.arg("--read-audit").arg(audit);
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
        let output = assert_cmd::Command::from_std(graphhelm_process::command(
            assert_cmd::cargo::cargo_bin!("graphhelm"),
        ))
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

/// #1835: each journey's last run comes from what its runner recorded, identically on the CLI,
/// HTTP and MCP. Credible regressions: a journey never run read as passed or failed, a failed run
/// without the step that broke, and a run in flight hiding the last finished one.
#[test]
fn flows_serve_each_journeys_last_recorded_run_and_not_run_when_none() {
    let harness = prepared();
    let (_, read) = harness.cli(&["preview", "checkout", "--read"]);
    let digest = read["data"]["digest"].as_str().unwrap().to_owned();
    let previews = harness.project.join(".graphhelm/journey-previews");
    // checkout passed; a newer run is in flight, so the passed one is the newest archived run.
    let archived = previews.join("checkout/history/00000001792058400000");
    std::fs::create_dir_all(&archived).unwrap();
    std::fs::write(
        archived.join("state.json"),
        json!({"preview":true,"kind":"replay","state":"ready","result":"pass","digest":digest,
            "commit":"c1","startedAt":"2026-10-17T10:00:00+00:00",
            "ranAt":"2026-10-17T10:00:09+00:00",
            "screens":{"cart":{"result":"pass","frame":false},"pay":{"result":"pass","frame":false},
                "done":{"result":"pass","frame":false}},
            "edges":{"cart.checkout":{"result":"pass"},"pay.submit":{"result":"pass"}}})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        previews.join("checkout/state.json"),
        json!({"preview":true,"state":"running","digest":digest,"commit":"c1",
            "startedAt":"2026-10-18T10:00:00+00:00","screens":{},"edges":{}})
        .to_string(),
    )
    .unwrap();
    // basket failed at its second edge; `done` was never reached. Its flow changed since.
    std::fs::create_dir_all(previews.join("basket")).unwrap();
    std::fs::write(
        previews.join("basket/state.json"),
        json!({"preview":true,"kind":"replay","state":"ready","result":"fail","digest":"sha256:old",
            "commit":"c1","startedAt":"2026-10-17T11:00:00+00:00",
            "ranAt":"2026-10-17T11:00:07+00:00",
            "screens":{"cart":{"result":"pass","frame":false},"pay":{"result":"pass","frame":false},
                "done":{"frame":false,"reason":"not_reached"}},
            "edges":{"cart.checkout":{"result":"pass"},
                "pay.submit":{"result":"fail","reason":"act_failed"}}})
        .to_string(),
    )
    .unwrap();

    let cli = harness.flows();
    assert_eq!(
        flow(&cli, "checkout")["lastRun"],
        json!({"status":"passed","ranAt":"2026-10-17T10:00:09+00:00","kind":"replay",
            "reason":null,"failingStep":null,"flowChanged":false,
            // #1777: a run recorded before viewports proved web only.
            "viewports":{"web":"passed","mobile":"not_run"},"failsOnlyOn":null}),
        "{cli}"
    );
    assert_eq!(
        flow(&cli, "basket")["lastRun"],
        json!({"status":"failed","ranAt":"2026-10-17T11:00:07+00:00","kind":"replay",
            "reason":null,"failingStep":{"kind":"edge","id":"pay.submit","result":"fail",
                "reason":"act_failed","viewport":null},"flowChanged":true,
            "viewports":{"web":"failed","mobile":"not_run"},"failsOnlyOn":null}),
        "{cli}"
    );
    assert_eq!(
        flow(&cli, "broken")["lastRun"],
        json!({"status":"not_run","ranAt":null,"kind":null,"reason":null,"failingStep":null,
            "flowChanged":null,"viewports":{"web":"not_run","mobile":"not_run"},"failsOnlyOn":null}),
        "{cli}"
    );

    let (_server, base, token) = harness.serve(true);
    let (status, over_http) = http(&base, "GET", "/v1/journey-flows", Some(&token));
    assert_eq!(status, 200, "{over_http}");
    assert_eq!(over_http["data"], cli, "HTTP differs from the CLI");
    let (ok, over_mcp) = harness.mcp(&base, &token, "journey_flows", &json!({}));
    assert!(ok, "{over_mcp}");
    assert_eq!(over_mcp["data"], cli, "MCP differs from the CLI");
}

/// #1953: a journey whose run is in flight is listed `running` (with when it started and the step
/// it is on) next to its last finished run, on the Runtime route as on the CLI, so a reloaded
/// Studio shows it running. Credible regressions: the list hiding a run in flight (the card reads
/// `not run` until it ends), and a run abandoned long ago listed as running for ever.
#[test]
fn flows_list_a_run_in_flight_as_running_and_an_abandoned_one_as_not() {
    let harness = prepared();
    let previews = harness.project.join(".graphhelm/journey-previews");
    let started = chrono::Utc::now().to_rfc3339();
    std::fs::create_dir_all(previews.join("checkout")).unwrap();
    std::fs::write(
        previews.join("checkout/state.json"),
        json!({"preview":true,"kind":"replay","state":"running","digest":"sha256:aa",
            "commit":"c1","startedAt":started,"current":"cart.checkout",
            "screens":{},"edges":{}})
        .to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(previews.join("basket")).unwrap();
    std::fs::write(
        previews.join("basket/state.json"),
        json!({"preview":true,"kind":"replay","state":"running","digest":"sha256:aa",
            "commit":"c1","startedAt":"2020-01-01T00:00:00+00:00","current":null,
            "screens":{},"edges":{}})
        .to_string(),
    )
    .unwrap();

    let (_server, base, token) = harness.serve(true);
    let (status, over_http) = http(&base, "GET", "/v1/journey-flows", Some(&token));
    assert_eq!(status, 200, "{over_http}");
    let data = &over_http["data"];
    assert_eq!(
        flow(data, "checkout")["running"],
        json!({"startedAt":started,"current":"cart.checkout","kind":"replay"}),
        "{data}"
    );
    assert_eq!(
        flow(data, "checkout")["lastRun"]["status"],
        "not_run",
        "{data}"
    );
    assert_eq!(flow(data, "basket")["running"], Value::Null, "{data}");
    assert_eq!(flow(data, "broken")["running"], Value::Null, "{data}");
    assert_eq!(*data, harness.flows(), "HTTP differs from the CLI");
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
    // #1684: the edited flow is a new digest with no preview yet; approve would refuse it, so
    // it is not approvable until its preview is recorded.
    assert_eq!(stale["approvable"], false, "{stale}");
    support::passing_preview(&harness.project, "checkout");
    let previewed = flow(&harness.flows(), "checkout").clone();
    assert_eq!(previewed["approvable"], true, "{previewed}");
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

/// #1588: only the owner rejects a flow, over HTTP and MCP as on the CLI, and the review list
/// carries the reason. Credible regressions: the agent session token or an agent-typed MCP session
/// recording a rejection, a rejection with no note, and a reason the Runtime cannot show.
#[test]
fn only_the_owner_rejects_a_flow_and_the_review_list_shows_the_reason() {
    let harness = prepared();
    let (_server, base, owner) = harness.serve(true);
    let agent_token = std::fs::read_to_string(harness.events.with_extension("agent.token"))
        .expect("serve mints the agent session token beside the owner token");
    const ROUTE: &str = "/v1/journey-flows/checkout/reject";
    const NOTE: &str = r#"{"note": "the receipt never shows"}"#;

    let (status, refused) = http_body(&base, "POST", ROUTE, Some(agent_token.trim()), NOTE);
    assert_eq!(status, 403, "{refused}");
    let (ok, refused) = harness.mcp(
        &base,
        &owner,
        "journey_reject",
        &json!({"id": "checkout", "note": "x"}),
    );
    assert!(!ok, "{refused}");
    assert_eq!(
        refused["error"]["data"]["code"], "GHCLI040_JOURNEY_REJECT_OWNER_ONLY",
        "{refused}"
    );
    let (status, refused) = http_body(&base, "POST", ROUTE, Some(&owner), r#"{"note": " "}"#);
    assert_eq!(status, 400, "an empty note: {refused}");
    assert_eq!(flow(&harness.flows(), "checkout")["rejected"], Value::Null);

    let (status, rejected) = http_body(&base, "POST", ROUTE, Some(&owner), NOTE);
    assert_eq!(status, 200, "{rejected}");
    assert_eq!(rejected["command"], "journey.reject");
    assert_eq!(
        flow(&harness.flows(), "checkout")["rejected"]["note"],
        "the receipt never shows"
    );

    let (ok, over_mcp) = harness.mcp_as(
        "owner",
        &base,
        &owner,
        "journey_reject",
        &json!({"id": "basket", "note": "the basket loses items"}),
    );
    assert!(ok, "{over_mcp}");
    assert_eq!(
        flow(&harness.flows(), "basket")["rejected"]["note"],
        "the basket loses items"
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

/// #1130 (`keel.invariant.permissions`): a reviewer agent approves a draft only as the actor its
/// scoped credential names, and only when the store's `drafted` record names a different author
/// for that exact digest. Defects named: an author approving its own flow by declaring another
/// reviewer name, an approval with no recorded author, the shared agent session token reaching
/// the routes, and the generic signal route writing the reserved kinds. Cost: one Runtime, a few
/// requests.
#[test]
fn a_reviewer_agent_reviews_only_with_its_own_credential_and_never_approves() {
    let harness = prepared();
    let owner = owner_token(&harness.project);
    let events = harness.project.join(".graphhelm/events");
    let (_server, base, _) = harness.serve_with(&events, true);
    let _ = owner;
    let before = harness.flow_text("checkout");
    let review = |bearer: &str| {
        http_body(
            &base,
            "POST",
            "/v1/journey-flows/checkout/review",
            Some(bearer),
            r#"{"checked":"docs/PROJECT.md checkout screens"}"#,
        )
    };
    let draft = |bearer: &str, yaml: &str| {
        http_body(
            &base,
            "POST",
            "/v1/journey-flows/checkout/draft",
            Some(bearer),
            &json!({"yaml": yaml}).to_string(),
        )
    };
    // A draft no lane wrote through the Runtime has no author: fail closed.
    let (status, refused) = review(REVIEWER_CREDENTIAL);
    assert_ne!(status, 200, "{refused}");
    assert_eq!(harness.flow_text("checkout"), before);
    // The shared agent session token reaches neither route.
    let shared = std::fs::read_to_string(events.with_extension("agent.token")).unwrap();
    for path in ["draft", "review"] {
        let (status, refused) = http(
            &base,
            "POST",
            &format!("/v1/journey-flows/checkout/{path}"),
            Some(shared.trim()),
        );
        assert_ne!(status, 200, "{path}: {refused}");
    }
    // #1178: a non-canonical draft is refused to its author, not later to its reviewer, and
    // leaves the file as it was.
    let (status, refused) = draft(AGENT_CREDENTIAL, &format!("{before}# stray comment\n"));
    assert_ne!(status, 200, "a non-canonical draft was accepted: {refused}");
    assert!(refused.to_string().contains("not canonical"), "{refused}");
    assert_eq!(harness.flow_text("checkout"), before);
    // The author writes its draft through the Runtime.
    let authored = before.replace(
        "title: Shopper pays for the cart",
        "title: Shopper pays for the whole cart",
    );
    let (status, written) = draft(AGENT_CREDENTIAL, &authored);
    assert_eq!(status, 200, "{written}");
    assert_eq!(written["data"]["writer"], "agent-planner", "{written}");
    assert_eq!(harness.flow_text("checkout"), authored);
    // Another lane cannot claim it by re-posting the same bytes ...
    let (status, refused) = draft(SQUATTER_CREDENTIAL, &authored);
    assert_ne!(
        status, 200,
        "a lane re-claimed another lane's draft: {refused}"
    );
    // ... and the author cannot approve its own draft.
    let (status, refused) = review(AGENT_CREDENTIAL);
    assert_ne!(status, 200, "the author approved its own draft: {refused}");
    // A lane that changed the flow becomes one of its writers, and no writer of any version
    // reviews it: not the squatter, and not the first author after the squatter's edit.
    let squatted = authored.replace("whole cart", "entire cart");
    let (status, written) = draft(SQUATTER_CREDENTIAL, &squatted);
    assert_eq!(status, 200, "{written}");
    for writer in [SQUATTER_CREDENTIAL, AGENT_CREDENTIAL] {
        let (status, refused) = review(writer);
        assert_ne!(status, 200, "a writer of the flow approved it: {refused}");
    }
    assert_eq!(flow(&harness.flows(), "checkout")["status"], "draft");
    // A file edited outside the route has no writer at its digest: fail closed.
    let path = harness
        .project
        .join(".graphhelm/journeys/checkout.journey.yaml");
    std::fs::write(&path, squatted.replace("entire cart", "full cart")).unwrap();
    let (status, refused) = review(REVIEWER_CREDENTIAL);
    assert_ne!(status, 200, "an off-route edit was approved: {refused}");
    std::fs::write(&path, &squatted).unwrap();
    // The generic signal route cannot write the reserved kinds.
    let body = json!({"signal": {"id": "journey-agent-approved-00000000000000000000000000000000",
        "type": "journey_flow_agent_approved", "severity": "low",
        "source": {"type": "node", "id": "agent-planner"}, "description": "{}",
        "evidence": ["x"], "emittedAt": "2026-10-13T00:00:00Z"}})
    .to_string();
    let (status, refused) = signal_as_agent(&base, shared.trim(), "forged-review", &body);
    assert_ne!(status, 200, "{refused}");
    // #1309 ruling A: a different lane records its review; the review never approves. The flow
    // stays a draft until the owner approves it.
    let (status, reviewed) = review(REVIEWER_CREDENTIAL);
    assert_eq!(status, 200, "{reviewed}");
    assert_eq!(reviewed["data"]["reviewed"], true, "{reviewed}");
    assert_eq!(reviewed["data"]["status"], "draft", "{reviewed}");
    assert_eq!(flow(&harness.flows(), "checkout")["status"], "draft");
    // An agent's review record never signs an approval: a flow file edited to `approved` at the
    // reviewed digest still reads unsigned.
    let edited = squatted.replace("status: draft", "status: approved");
    assert_ne!(edited, squatted, "the fixture flow carries `status: draft`");
    std::fs::write(&path, &edited).unwrap();
    let (_, validated) = harness.cli(&["validate", "--all"]);
    let checkout = validated["data"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| {
            file["file"]
                .as_str()
                .unwrap()
                .ends_with("checkout.journey.yaml")
        })
        .unwrap()
        .clone();
    assert!(
        checkout["ok"] == false && !checkout["findings"].as_array().unwrap().is_empty(),
        "an agent review signed an approval: {validated}"
    );
    std::fs::write(&path, &squatted).unwrap();
}

/// #1309 slice 1: one owner grant request, with the `Idempotency-Key` every mutation carries.
fn grant_call(base: &str, path: &str, bearer: &str, key: &str, body: &str) -> (u16, Value) {
    let address = base.strip_prefix("http://").unwrap();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAuthorization: Bearer {bearer}\r\nIdempotency-Key: {key}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(scaled(Duration::from_secs(60))))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut body = raw[split + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn first_code(reply: &Value) -> (&str, &str) {
    let diagnostic = &reply["diagnostics"][0];
    (
        diagnostic["code"].as_str().unwrap_or(""),
        diagnostic["path"].as_str().unwrap_or(""),
    )
}

/// #1309 slice 1: owner grants live in a private journal beside the events directory. They
/// survive a restart by replay, a reused key answers its receipt, no agent credential reaches the
/// routes, and a torn journal refuses rather than guessing.
#[test]
fn owner_journey_grants_survive_a_restart_and_refuse_agents_and_a_torn_journal() {
    let harness = prepared();
    let journal = harness
        .scratch
        .path()
        .join("credential-broker/journey-credentials.v1.log");
    let grants = format!("/v1/executions/{RUN}/journey-credential-grants");
    let (guard, base, token) = harness.serve(true);
    let project_id = http(&base, "GET", "/health", None).1["data"]["projectId"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = |purpose: &str| {
        json!({"projectId": project_id, "laneId": "agent-reviewer", "flowId": "checkout",
            "purpose": purpose, "expiresAt": "2030-01-01T00:00:00Z"})
        .to_string()
    };

    let (status, created) = grant_call(&base, &grants, &token, "grant-1", &body("reviewer"));
    assert_eq!(status, 200, "{created}");
    let grant = created["data"]["grantId"].as_str().unwrap().to_owned();
    assert_eq!(created["data"]["state"], "active");
    assert_eq!(created["data"]["laneId"], "agent-reviewer");
    let (status, again) = grant_call(&base, &grants, &token, "grant-1", &body("reviewer"));
    assert_eq!((status, &again["data"]), (200, &created["data"]));
    let (status, conflict) = grant_call(&base, &grants, &token, "grant-1", &body("author"));
    assert_eq!(status, 409, "{conflict}");
    assert_eq!(first_code(&conflict).1, "/idempotencyKey");
    let committed = std::fs::read(&journal).unwrap();

    // No agent credential reaches the routes, and none of them writes a frame.
    let agent_token =
        std::fs::read_to_string(harness.events.with_extension("agent.token")).unwrap();
    for bearer in [agent_token.trim(), AGENT_CREDENTIAL, REVIEWER_CREDENTIAL] {
        let (status, refused) = grant_call(&base, &grants, bearer, "grant-agent", &body("author"));
        assert!(matches!(status, 401 | 403), "{status} {refused}");
    }
    let (status, refused) = grant_call(&base, &grants, &token, "grant-2", &body("approve"));
    assert_eq!(
        (status, first_code(&refused).1),
        (400, "/purpose"),
        "{refused}"
    );
    let (status, refused) = grant_call(
        &base,
        "/v1/executions/no-such-run/journey-credential-grants",
        &token,
        "grant-3",
        &body("author"),
    );
    assert_eq!(status, 404, "{refused}");
    assert_eq!(std::fs::read(&journal).unwrap(), committed);

    let revoke = format!("{grants}/{grant}/revoke");
    let (status, revoked) = grant_call(&base, &revoke, &token, "revoke-1", "");
    assert_eq!(status, 200, "{revoked}");
    assert_eq!(revoked["data"]["state"], "revoked");
    drop(guard);

    // A restart replays both frames: the receipt still answers, and the grant is still revoked.
    let (guard, base, token) = harness.serve(true);
    let (status, replayed) = grant_call(&base, &grants, &token, "grant-1", &body("reviewer"));
    assert_eq!(
        (status, replayed["data"]["grantId"].as_str()),
        (200, Some(grant.as_str()))
    );
    let (status, refused) = grant_call(&base, &revoke, &token, "revoke-2", "");
    assert_eq!(
        (status, first_code(&refused).1),
        (409, "/grantId"),
        "{refused}"
    );
    drop(guard);

    // A torn last frame: both routes refuse, and the journal is left exactly as found.
    let mut torn = std::fs::read(&journal).unwrap();
    torn.pop();
    std::fs::write(&journal, &torn).unwrap();
    let (_guard, base, token) = harness.serve(true);
    for (path, key) in [(grants.as_str(), "grant-4"), (revoke.as_str(), "revoke-3")] {
        let (status, refused) = grant_call(&base, path, &token, key, &body("author"));
        assert_eq!(
            (status, first_code(&refused).1),
            (503, "/journal"),
            "{refused}"
        );
    }
    assert_eq!(std::fs::read(&journal).unwrap(), torn);
}

/// #1309 slice 2: one request with arbitrary headers. Returns status, the response head (so a test
/// can read `Cache-Control`), and the parsed body.
fn lane_call(base: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, String, Value) {
    let address = base.strip_prefix("http://").unwrap();
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(scaled(Duration::from_secs(60))))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let mut body = raw[split + 4..].to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        body = dechunk(&body);
    }
    (
        status,
        head,
        serde_json::from_slice(&body).unwrap_or(Value::Null),
    )
}

/// The #1148 lane credential a lane holds for `(execution, lane)`: the documented derivation the
/// Runtime recomputes to verify the declared lane.
fn lane_bearer(owner: &str, execution: &str, lane: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"graphhelm-lane-credential-v1\0");
    hasher.update(owner.as_bytes());
    hasher.update(b"\0");
    hasher.update(execution.as_bytes());
    hasher.update(b"\0");
    hasher.update(lane.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// #1309 slice 2 (RFC-1309 "Protected delivery channel", disposition on #1488): a lane holding
/// its own lane credential and an active exact-scope owner grant receives one journey credential,
/// once. The token reaches no journal frame, no `--read-audit` line and no replay; a same-key
/// retry answers 409 without it; every other principal and every scope mismatch is refused with
/// no frame written; the owner can revoke the credential, and the state survives a restart.
#[test]
fn a_lane_receives_its_journey_credential_once_and_the_token_is_written_nowhere() {
    let harness = prepared();
    let audit = harness.scratch.path().join("read-audit.jsonl");
    let journal = harness
        .scratch
        .path()
        .join("credential-broker/journey-credentials.v1.log");
    let grants = format!("/v1/executions/{RUN}/journey-credential-grants");
    let issue = format!("/v1/executions/{RUN}/journey-credentials");
    let (guard, base, owner) = harness.serve_with_audit(&harness.events, true, Some(&audit));
    let project_id = http(&base, "GET", "/health", None).1["data"]["projectId"]
        .as_str()
        .unwrap()
        .to_owned();
    let grant_body = |expires: &str| {
        json!({"projectId": project_id, "laneId": "agent-reviewer", "flowId": "checkout",
            "purpose": "reviewer", "expiresAt": expires})
        .to_string()
    };
    let (status, created) = grant_call(
        &base,
        &grants,
        &owner,
        "grant-live",
        &grant_body("2030-01-01T00:00:00Z"),
    );
    assert_eq!(status, 200, "{created}");
    let grant = created["data"]["grantId"].as_str().unwrap().to_owned();
    let (status, stale) = grant_call(
        &base,
        &grants,
        &owner,
        "grant-stale",
        &grant_body("2000-01-01T00:00:00Z"),
    );
    assert_eq!(status, 200, "{stale}");
    let stale = stale["data"]["grantId"].as_str().unwrap().to_owned();

    let reviewer = lane_bearer(&owner, RUN, "agent-reviewer");
    let planner = lane_bearer(&owner, RUN, "agent-planner");
    let ask = |bearer: &str, actor: &str, key: &str, body: &Value| {
        let authorization = format!("Bearer {bearer}");
        lane_call(
            &base,
            &issue,
            &[
                ("Authorization", &authorization),
                ("X-GraphHelm-Actor", actor),
                ("Idempotency-Key", key),
            ],
            &body.to_string(),
        )
    };
    let request = |grant_id: &str, flow: &str, purpose: &str| {
        json!({"grantId": grant_id, "projectId": project_id, "executionId": RUN,
            "flowId": flow, "purpose": purpose})
    };

    let (status, head, issued) = ask(
        &reviewer,
        "agent-reviewer",
        "cred-1",
        &request(&grant, "checkout", "reviewer"),
    );
    assert_eq!(status, 200, "{issued}");
    assert!(
        head.to_ascii_lowercase()
            .contains("cache-control: no-store"),
        "{head}"
    );
    let token = issued["data"]["token"].as_str().unwrap().to_owned();
    assert_eq!(token.len(), 64, "32 random bytes, hex");
    let credential = issued["data"]["credentialId"].as_str().unwrap().to_owned();
    assert_eq!(issued["data"]["grantId"], grant.as_str());
    assert_eq!(issued["data"]["state"], "active");

    // The token is sent once: the same key answers 409 naming the credential, without the token.
    let (status, _, again) = ask(
        &reviewer,
        "agent-reviewer",
        "cred-1",
        &request(&grant, "checkout", "reviewer"),
    );
    assert_eq!(
        (status, first_code(&again).1),
        (409, "/credentialId"),
        "{again}"
    );
    assert!(again.to_string().contains(&credential), "{again}");
    assert!(!again.to_string().contains(&token), "{again}");
    let committed = std::fs::read(&journal).unwrap();

    // Every other principal and every scope mismatch is refused before a token is drawn, with no
    // frame written.
    let agent_session =
        std::fs::read_to_string(harness.events.with_extension("agent.token")).unwrap();
    let cases = [
        // Another lane presenting the reviewer grant.
        (
            planner.clone(),
            "agent-planner",
            request(&grant, "checkout", "reviewer"),
        ),
        // The reviewer credential declared under another lane name verifies as no lane.
        (
            reviewer.clone(),
            "agent-planner",
            request(&grant, "checkout", "reviewer"),
        ),
        // The shared agent session token proves no lane.
        (
            agent_session.trim().to_owned(),
            "agent-reviewer",
            request(&grant, "checkout", "reviewer"),
        ),
        // The owner token is not a lane.
        (
            owner.clone(),
            "agent-reviewer",
            request(&grant, "checkout", "reviewer"),
        ),
        // Wrong flow, wrong purpose, an expired grant, an unknown grant.
        (
            reviewer.clone(),
            "agent-reviewer",
            request(&grant, "basket", "reviewer"),
        ),
        (
            reviewer.clone(),
            "agent-reviewer",
            request(&grant, "checkout", "author"),
        ),
        (
            reviewer.clone(),
            "agent-reviewer",
            request(&stale, "checkout", "reviewer"),
        ),
        (
            reviewer.clone(),
            "agent-reviewer",
            request("grant-none", "checkout", "reviewer"),
        ),
    ];
    for (bearer, actor, body) in &cases {
        let (status, _, refused) = ask(bearer, actor, "cred-refused", body);
        assert!(matches!(status, 401 | 403), "{actor} {status} {refused}");
        assert!(refused["data"]["token"].is_null(), "{refused}");
    }
    assert_eq!(std::fs::read(&journal).unwrap(), committed);

    // The owner revokes the credential.
    let revoke = format!("{issue}/{credential}/revoke");
    let (status, revoked) = grant_call(&base, &revoke, &owner, "rev-1", "");
    assert_eq!(status, 200, "{revoked}");
    assert_eq!(revoked["data"]["state"], "revoked");
    // A lane cannot revoke, even its own credential.
    let authorization = format!("Bearer {reviewer}");
    let (status, _, refused) = lane_call(
        &base,
        &revoke,
        &[
            ("Authorization", &authorization),
            ("X-GraphHelm-Actor", "agent-reviewer"),
            ("Idempotency-Key", "rev-lane"),
        ],
        "",
    );
    assert!(matches!(status, 401 | 403), "{status} {refused}");
    // A revoked grant issues nothing new.
    let (status, _) = grant_call(
        &base,
        &format!("{grants}/{grant}/revoke"),
        &owner,
        "rev-2",
        "",
    );
    assert_eq!(status, 200);
    let (status, _, refused) = ask(
        &reviewer,
        "agent-reviewer",
        "cred-2",
        &request(&grant, "checkout", "reviewer"),
    );
    assert_eq!(status, 403, "{refused}");
    drop(guard);

    // The token is written nowhere: not in the journal, and not in the read audit, which did
    // record the issuing response, the 409 and the refusals.
    let audit_text = std::fs::read_to_string(&audit).unwrap();
    assert!(audit_text.contains("journey credential"), "{audit_text}");
    assert!(
        !audit_text.contains(&token),
        "the read audit holds the token"
    );
    let journal_bytes = std::fs::read(&journal).unwrap();
    assert!(
        !journal_bytes
            .windows(token.len())
            .any(|window| window == token.as_bytes()),
        "the journal holds the token"
    );

    // A restart replays the credential, and the same key still never returns the token.
    let (_guard, base, _) = harness.serve_with_audit(&harness.events, true, Some(&audit));
    let (status, _, replayed) = lane_call(
        &base,
        &issue,
        &[
            ("Authorization", &authorization),
            ("X-GraphHelm-Actor", "agent-reviewer"),
            ("Idempotency-Key", "cred-1"),
        ],
        &request(&grant, "checkout", "reviewer").to_string(),
    );
    assert_eq!(
        (status, first_code(&replayed).1),
        (409, "/credentialId"),
        "{replayed}"
    );
    assert!(!replayed.to_string().contains(&token));
}

/// #1309 enforcement slice (scope ruled 2026-10-16: the draft route only): a lane writes a journey
/// draft with its journey credential only while the credential and its grant are both active and
/// unexpired, for that exact flow, with purpose `author`. Revocation and expiry are checked on
/// every use. A reviewer credential, a credential for another flow, a revoked or expired one, and
/// a wrong token are refused with one stable 403 and leave the flow file as it was; the token
/// never appears in a reply or the read audit. The review route is untouched (approval authority
/// is still BLOCKED in RFC-1309).
#[test]
fn a_journey_credential_writes_a_draft_only_while_it_and_its_grant_are_live() {
    let harness = prepared();
    let audit = harness.scratch.path().join("read-audit.jsonl");
    let grants = format!("/v1/executions/{RUN}/journey-credential-grants");
    let issue = format!("/v1/executions/{RUN}/journey-credentials");
    let (_guard, base, owner) = harness.serve_with_audit(&harness.events, true, Some(&audit));
    let project_id = http(&base, "GET", "/health", None).1["data"]["projectId"]
        .as_str()
        .unwrap()
        .to_owned();
    let lane = lane_bearer(&owner, RUN, "agent-planner");
    let mut keys = 0;
    let mut grant = |flow: &str, purpose: &str, expires: &str| {
        keys += 1;
        let body = json!({"projectId": project_id, "laneId": "agent-planner", "flowId": flow,
            "purpose": purpose, "expiresAt": expires})
        .to_string();
        let (status, created) = grant_call(&base, &grants, &owner, &format!("g-{keys}"), &body);
        assert_eq!(status, 200, "{created}");
        created["data"]["grantId"].as_str().unwrap().to_owned()
    };
    let credential = |grant_id: &str, flow: &str, purpose: &str, key: &str| {
        let authorization = format!("Bearer {lane}");
        let (status, _, issued) = lane_call(
            &base,
            &issue,
            &[
                ("Authorization", &authorization),
                ("X-GraphHelm-Actor", "agent-planner"),
                ("Idempotency-Key", key),
            ],
            &json!({"grantId": grant_id, "projectId": project_id, "executionId": RUN,
                "flowId": flow, "purpose": purpose})
            .to_string(),
        );
        assert_eq!(status, 200, "{issued}");
        (
            issued["data"]["credentialId"].as_str().unwrap().to_owned(),
            issued["data"]["token"].as_str().unwrap().to_owned(),
        )
    };
    let before = harness.flow_text("checkout");
    let mut edits = 0;
    let mut draft = |flow: &str, id: &str, token: &str| {
        edits += 1;
        let yaml = harness
            .flow_text(flow)
            .replacen("title: ", &format!("title: Edit {edits} "), 1);
        let authorization = format!("Bearer {token}");
        let (status, _, reply) = lane_call(
            &base,
            &format!("/v1/journey-flows/{flow}/draft"),
            &[
                ("Authorization", &authorization),
                ("X-GraphHelm-Journey-Credential", id),
            ],
            &json!({"yaml": yaml}).to_string(),
        );
        assert!(
            !reply.to_string().contains(token),
            "a reply carries the token"
        );
        (status, reply)
    };

    // A live author credential for this flow writes the draft, as its lane.
    let author_grant = grant("checkout", "author", "2030-01-01T00:00:00Z");
    let (author_id, author_token) = credential(&author_grant, "checkout", "author", "c-1");
    let (status, written) = draft("checkout", &author_id, &author_token);
    assert_eq!(status, 200, "{written}");
    assert_ne!(harness.flow_text("checkout"), before);

    // Refusals leave the flow file as it was.
    let refuse = |status: u16, reply: &Value, flow_before: &str, flow_now: &str| {
        assert_eq!(status, 403, "{reply}");
        assert_eq!(flow_before, flow_now);
    };
    // The same credential on another flow.
    let basket = harness.flow_text("basket");
    let (status, reply) = draft("basket", &author_id, &author_token);
    refuse(status, &reply, &basket, &harness.flow_text("basket"));
    // A wrong token under a real credential id.
    let checkout = harness.flow_text("checkout");
    let (status, reply) = draft("checkout", &author_id, &"0".repeat(64));
    refuse(status, &reply, &checkout, &harness.flow_text("checkout"));
    // A reviewer credential never writes a draft.
    let reviewer_grant = grant("checkout", "reviewer", "2030-01-01T00:00:00Z");
    let (reviewer_id, reviewer_token) = credential(&reviewer_grant, "checkout", "reviewer", "c-2");
    let (status, reply) = draft("checkout", &reviewer_id, &reviewer_token);
    refuse(status, &reply, &checkout, &harness.flow_text("checkout"));

    // Revocation is checked on every use: the owner revokes, and the same token stops working.
    let (status, _) = grant_call(
        &base,
        &format!("{issue}/{author_id}/revoke"),
        &owner,
        "r-1",
        "",
    );
    assert_eq!(status, 200);
    let (status, reply) = draft("checkout", &author_id, &author_token);
    refuse(status, &reply, &checkout, &harness.flow_text("checkout"));

    // Revoking the grant stops a credential that is itself still active.
    let second = grant("checkout", "author", "2030-01-01T00:00:00Z");
    let (second_id, second_token) = credential(&second, "checkout", "author", "c-3");
    let (status, _) = grant_call(
        &base,
        &format!("{grants}/{second}/revoke"),
        &owner,
        "r-2",
        "",
    );
    assert_eq!(status, 200);
    let (status, reply) = draft("checkout", &second_id, &second_token);
    refuse(status, &reply, &checkout, &harness.flow_text("checkout"));

    // Expiry is checked on every use: a grant that ends in a few seconds bounds its credential.
    let soon = (chrono::Utc::now() + chrono::Duration::seconds(3))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let short = grant("checkout", "author", &soon);
    let (short_id, short_token) = credential(&short, "checkout", "author", "c-4");
    std::thread::sleep(Duration::from_secs(5));
    let (status, reply) = draft("checkout", &short_id, &short_token);
    refuse(status, &reply, &checkout, &harness.flow_text("checkout"));

    // No token reached the read audit.
    let audit_text = std::fs::read_to_string(&audit).unwrap();
    for token in [&author_token, &reviewer_token, &second_token, &short_token] {
        assert!(
            !audit_text.contains(token.as_str()),
            "the read audit holds a token"
        );
    }
}
