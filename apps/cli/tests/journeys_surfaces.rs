//! #315 Task 3: the proven-journey map is the same `data` on the CLI, over HTTP and over MCP;
//! refused contracts and ignored records are reported, never folded; the route is owner-only
//! and needs the Runtime's `--project`.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

const EVENTS_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const KEY_ID: &str = "owner-key";
const RUN: &str = "journeys-surfaces";
/// A second run of the same project that records nothing itself (#332).
const OTHER_RUN: &str = "journeys-other";
const AGENT_CREDENTIAL: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3];

fn graphhelm() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env("GRAPHHELM_EVENTS_KEY", EVENTS_KEY);
    command
}

fn git(project: &Path, args: &[&str]) -> String {
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
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn step(id: &str, scope: &str) -> Value {
    json!({
        "stepId": id,
        "actorId": "shopper",
        "semanticAction": {
            "kind": "navigate",
            "target": {"strategy": "visible_text", "value": "Cart", "geometryClaim": false},
            "input": "typed-sentinel-379"
        },
        "expectedStates": ["stable"],
        "failureContract": {
            "timeoutSeconds": 30,
            "visibleError": "Cart did not open",
            "safeStop": "Stay on the page",
            "recoveryAction": null,
            "prohibitedSideEffects": []
        },
        "screen": {"screenId": format!("{id}-screen"), "title": id, "scopePaths": [scope]}
    })
}

fn contract(id: &str) -> Value {
    json!({
        "contractId": id,
        "version": 1,
        "title": "Cart",
        "taskScope": "Buy from the cart",
        "actors": [{"actorId": "shopper", "name": "Shopper", "goal": "Buy"}],
        "preconditions": [],
        "steps": [step("open-cart", "web/cart/"), step("review", "web/review/"), step("pay", "web/pay/")],
        "promises": [{
            "promiseId": "cart-renders",
            "stepId": "open-cart",
            "statement": "The cart renders",
            "requiredFact": "content_rendered",
            "requiredEvidenceKinds": ["visual_capture"],
            "requiredObserverCapability": "browser",
            "statesToObserve": ["stable"],
            "maxEvidenceAgeSeconds": 3600
        }],
        "riskSignals": [],
        "outOfScope": []
    })
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
    head: String,
}

/// A held manual run, an owner keyring, and a committed git project holding `cart.json` plus two
/// contract files that must be refused.
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
    for run in [RUN, OTHER_RUN] {
        let output = graphhelm()
            .args(["execution", "start", "--events"])
            .arg(&events)
            .args(["--execution", run, "--file"])
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
    }

    let project = scratch.path().join("project");
    let journeys = project.join(".graphhelm").join("journeys");
    std::fs::create_dir_all(&journeys).unwrap();
    std::fs::create_dir_all(project.join("web/cart")).unwrap();
    std::fs::write(project.join("web/cart/Line.tsx"), "line").unwrap();
    let write_contract = |name: &str, value: &Value| {
        std::fs::write(
            journeys.join(name),
            serde_json::to_vec_pretty(value).unwrap(),
        )
        .unwrap();
    };
    write_contract("cart.json", &contract("cart"));
    write_contract("bad..id.json", &contract("bad..id"));
    write_contract("wrong.json", &contract("other"));
    git(&project, &["init", "-q"]);
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "init"]);
    let head = git(&project, &["rev-parse", "HEAD"]);
    Harness {
        scratch,
        events,
        keyring,
        project,
        head,
    }
}

impl Harness {
    /// Records one signal through `execution signal`, with the PNG attached when `image`.
    fn signal(&self, id: &str, kind: &str, description: &Value, image: bool) {
        self.signal_in(RUN, id, kind, description, image);
    }

    fn signal_in(&self, run: &str, id: &str, kind: &str, description: &Value, image: bool) {
        let file = self.scratch.path().join(format!("{id}.json"));
        let envelope = json!({"id": id, "source": {"type": "test", "id": "journey-observer"},
            "type": kind, "severity": "low", "description": description.to_string(),
            "evidence": ["cart"], "emittedAt": "2026-10-06T12:00:00Z"});
        std::fs::write(&file, serde_json::to_vec(&envelope).unwrap()).unwrap();
        let png = self.scratch.path().join("shot.png");
        std::fs::write(&png, PNG).unwrap();
        let mut command = graphhelm();
        command
            .args(["execution", "signal", "--events"])
            .arg(&self.events)
            .args(["--execution", run, "--signal"])
            .arg(&file)
            .arg("--evidence-out")
            .arg(self.scratch.path().join(format!("{id}-evidence.json")))
            .arg("--keyring")
            .arg(&self.keyring)
            .args(["--key-id", KEY_ID]);
        if image {
            command.arg("--attach").arg(&png);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
    }

    fn capture(&self, id: &str, contract: &str, step: &str) {
        let description = json!({"protocol": "graphhelm-screen-capture-v1", "contractId": contract,
            "stepId": step, "revision": self.head, "dirty": false,
            "viewport": {"width": 1280, "height": 720}, "observer": "owner"});
        self.signal(id, "jpd.screen_captured", &description, true);
    }

    fn cli(&self) -> Value {
        self.cli_for(Some(RUN))
    }

    fn cli_for(&self, execution: Option<&str>) -> Value {
        let mut command = graphhelm();
        command.args(["journeys", "--events"]).arg(&self.events);
        if let Some(execution) = execution {
            command.args(["--execution", execution]);
        }
        let output = command
            .arg("--project")
            .arg(&self.project)
            .arg("--keyring")
            .arg(&self.keyring)
            .args(["--key-id", KEY_ID])
            .output()
            .unwrap();
        let envelope: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{envelope}");
        assert_eq!(envelope["command"], "journeys.read");
        envelope
    }

    /// Starts `serve`; `project` decides whether `--project` is passed.
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
        while get(&base, "/health", None).0 != 200 {
            assert!(Instant::now() < deadline, "the server never became healthy");
            std::thread::sleep(Duration::from_millis(50));
        }
        (guard, base, token)
    }
}

fn get(base: &str, path: &str, bearer: Option<&str>) -> (u16, Value) {
    let address = base.strip_prefix("http://").unwrap();
    let mut head = format!("GET {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
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

fn mcp(harness: &Harness, base: &str, token: &str) -> Value {
    mcp_with(harness, base, token, &json!({"executionId": RUN}))
}

fn mcp_with(harness: &Harness, base: &str, token: &str, arguments: &Value) -> Value {
    let token_file = harness.scratch.path().join("mcp-token");
    std::fs::write(&token_file, token).unwrap();
    let lines = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},
            "clientInfo":{"name":"conformance","version":"0"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"journeys","arguments": arguments}}),
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
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn cli_http_and_mcp_return_the_same_journey_map() {
    let harness = prepared();
    harness.capture("cap-open", "cart", "open-cart");
    harness.capture("cap-review", "cart", "review");
    harness.capture("cap-escape", "../etc", "open-cart");
    let walked = json!({"protocol": "graphhelm-transition-walked-v1", "contractId": "cart",
        "fromStepId": "open-cart", "toStepId": "review", "revision": harness.head,
        "observer": "owner", "fromCaptureId": "cap-open", "toCaptureId": "cap-review"});
    harness.signal("walk-1", "jpd.transition_walked", &walked, false);

    let cli = harness.cli()["data"].clone();
    assert_eq!(cli["head"], harness.head.as_str());
    assert_eq!(cli["ignoredRecords"], 1, "{cli}");
    assert_eq!(
        cli["refusedContracts"],
        json!([
            {"file": "bad..id.json", "reason": "invalid_file_name"},
            {"file": "wrong.json", "reason": "contract_id_mismatch"}
        ])
    );
    let journeys = cli["journeys"].as_array().unwrap();
    assert_eq!(journeys.len(), 1, "{cli}");
    let cart = &journeys[0];
    assert_eq!(cart["contractId"], "cart");
    let steps = cart["steps"].as_array().unwrap();
    assert_eq!(steps[0]["promises"], json!(["The cart renders"]), "{cli}");
    // #379: the map says what the user does and must reach on each step; the action's `input`
    // (typed text, possibly a secret) is never served.
    assert_eq!(
        steps[0]["action"],
        json!({"kind": "navigate", "target": "Cart", "strategy": "visible_text"}),
        "{cli}"
    );
    assert_eq!(steps[0]["expectedStates"], json!(["stable"]), "{cli}");
    assert!(!cli.to_string().contains("typed-sentinel-379"), "{cli}");
    assert!(steps[0]["capture"]["sequence"].is_u64(), "{cli}");
    assert_eq!(steps[0]["capture"]["signalId"], "cap-open");
    assert_eq!(steps[0]["capture"]["executionId"], RUN);
    assert!(steps[0]["capture"]["recordedAt"].is_string(), "{cli}");
    assert_eq!(cli["scope"], "project");
    assert_eq!(cli["requestedExecution"], RUN);
    assert_eq!(
        steps[0]["capture"]["imageEvidenceId"],
        "signal-cap-open-image-1"
    );
    assert_eq!(steps[0]["capture"]["freshness"], "fresh");
    assert_eq!(steps[0]["capture"]["changedFiles"], json!([]));
    assert_eq!(steps[1]["capture"]["signalId"], "cap-review");
    assert_eq!(steps[2]["capture"], Value::Null);
    assert_eq!(cart["arrows"][0]["state"], "walked");
    assert_eq!(cart["arrows"][0]["transitionSignalId"], "walk-1");
    assert_eq!(cart["arrows"][1]["state"], "never_walked");
    assert!(
        !cli.to_string().contains("../etc"),
        "an escaping id was folded: {cli}"
    );

    let (_server, base, token) = harness.serve(true);
    let path = format!("/v1/executions/{RUN}/journeys");
    let (status, http) = get(&base, &path, Some(&token));
    assert_eq!(status, 200, "{http}");
    assert_eq!(http["command"], "journeys.read");
    assert_eq!(http["data"], cli, "HTTP differs from the CLI");

    let over_mcp = mcp(&harness, &base, &token);
    assert_eq!(over_mcp["command"], "journeys.read");
    assert_eq!(over_mcp["data"], cli, "MCP differs from the CLI");

    // Owner-only: a scoped agent credential for this very run is refused.
    let (status, _) = get(&base, &path, Some(AGENT_CREDENTIAL));
    assert_eq!(status, 401);
}

#[test]
fn the_route_without_a_project_refuses_naming_project() {
    let harness = prepared();
    let (_server, base, token) = harness.serve(false);
    let (status, body) = get(
        &base,
        &format!("/v1/executions/{RUN}/journeys"),
        Some(&token),
    );
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["diagnostics"][0]["path"], "/project", "{body}");
}

/// #332: journeys are a project property. Captures recorded in one run show when another run of
/// the same project is asked for, and on the project-level CLI, route and MCP tool alike.
#[test]
fn captures_from_one_run_show_for_every_run_and_the_project() {
    let harness = prepared();
    harness.capture("cap-open", "cart", "open-cart");
    let description = json!({"protocol": "graphhelm-screen-capture-v1", "contractId": "cart",
        "stepId": "review", "revision": harness.head, "dirty": false,
        "viewport": {"width": 1280, "height": 720}, "observer": "owner"});
    harness.signal_in(
        OTHER_RUN,
        "cap-review",
        "jpd.screen_captured",
        &description,
        true,
    );

    let project = harness.cli_for(None)["data"].clone();
    let steps = &project["journeys"][0]["steps"];
    assert_eq!(steps[0]["capture"]["executionId"], RUN, "{project}");
    assert_eq!(steps[1]["capture"]["executionId"], OTHER_RUN, "{project}");
    assert_eq!(project["scope"], "project");
    assert!(project.get("requestedExecution").is_none(), "{project}");

    let mut other = harness.cli_for(Some(OTHER_RUN))["data"].clone();
    assert_eq!(other["requestedExecution"], OTHER_RUN);
    other.as_object_mut().unwrap().remove("requestedExecution");
    assert_eq!(other, project, "a run's map differs from the project's");

    let (_server, base, token) = harness.serve(true);
    let (status, http) = get(&base, "/v1/journeys", Some(&token));
    assert_eq!(status, 200, "{http}");
    assert_eq!(
        http["data"], project,
        "GET /v1/journeys differs from the CLI"
    );
    let (status, http) = get(
        &base,
        &format!("/v1/executions/{OTHER_RUN}/journeys"),
        Some(&token),
    );
    assert_eq!(status, 200, "{http}");
    assert_eq!(http["data"]["journeys"], project["journeys"]);
    let over_mcp = mcp_with(&harness, &base, &token, &json!({}));
    assert_eq!(over_mcp["data"], project, "MCP differs from the CLI");

    // A run that does not exist is still refused on the per-execution route.
    let (status, _) = get(&base, "/v1/executions/no-such-run/journeys", Some(&token));
    assert_eq!(status, 404);
    // Owner-only: a scoped agent credential is refused on the project route.
    let (status, _) = get(&base, "/v1/journeys", Some(AGENT_CREDENTIAL));
    assert_eq!(status, 401);
}
