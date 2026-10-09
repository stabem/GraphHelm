//! #398 (journey-first spec §5, phase C observer): an approved flow opened live at a step. The
//! walk replays the replay cache's locators of the earlier edges in a VISIBLE browser with no
//! model call and reports the step's state; the browser stays open for `journey act` until
//! `journey close`. Renaming the cart's button makes the walk stop at `cart.checkout/0` with
//! `drift.locator_missing`, and the session stays open on `cart`.
//! Cost: ~30s plus build, Node/Playwright/Chromium explicitly installed and a desktop for the
//! headed browser, local ports/Git; no provider/account or network installation. Ordinary
//! offline runs ignore this target.
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use serde_json::{Value, json};

const KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const SECRET: &str = "fixture_secret_831597";
const FLOW: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");
const FIXTURE: &str = include_str!("../../../tools/journey-driver/fixture-server.mjs");
const DRIVER: &[u8] = include_bytes!("../../../tools/journey-driver/driver.mjs");

struct Server {
    child: Child,
    group: graphhelm_process_tree::ProcessGroup,
    input: ChildStdin,
    replies: Receiver<Value>,
    started: Value,
}

impl Server {
    fn start(mut command: Command) -> Self {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        graphhelm_process_tree::configure(&mut command);
        let mut child = command
            .spawn()
            .expect("OBSERVER_MISSING: fixture/server executable");
        let group = match graphhelm_process_tree::create(&child) {
            Ok(group) => group,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("server containment: {error:?}")
            }
        };
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, replies) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.len() > 65536 {
                    break;
                }
                let Ok(value) = serde_json::from_str(&line) else {
                    break;
                };
                if tx.send(value).is_err() {
                    break;
                }
            }
        });
        let started = replies
            .recv_timeout(Duration::from_secs(30))
            .expect("OBSERVER_MISSING: fixture/server startup");
        Self {
            child,
            group,
            input,
            replies,
            started,
        }
    }
    fn control(&mut self, action: &str) -> Value {
        writeln!(self.input, "{action}").unwrap();
        self.input.flush().unwrap();
        self.replies.recv_timeout(Duration::from_secs(5)).unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = graphhelm_process_tree::terminate(self.child.id(), self.group);
        graphhelm_process_tree::close(&mut self.group);
        let _ = self.child.wait();
    }
}

fn cli() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.arg("--json").env("GRAPHHELM_EVENTS_KEY", KEY);
    command
}

fn reply(command: &mut Command) -> (i32, Value) {
    let out = command.output().unwrap();
    let value = serde_json::from_slice(&out.stdout).expect("CLI must return its JSON envelope");
    (out.status.code().unwrap(), value)
}

fn git(project: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .arg("-C")
        .arg(project)
        .args([
            "-c",
            "user.name=Replay observer",
            "-c",
            "user.email=replay@example.invalid",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "Git fixture operation failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn journey(project: &Path, args: &[&str]) -> (i32, Value) {
    let mut command = cli();
    command
        .arg("journey")
        .args(args)
        .arg("--project")
        .arg(project)
        .env("GRAPHHELM_SECRET_shopper_password", SECRET)
        .env("NODE_OPTIONS", "");
    reply(&mut command)
}

fn codes(value: &Value) -> Vec<String> {
    value["diagnostics"]
        .as_array()
        .map(|ds| {
            ds.iter()
                .map(|d| d["code"].as_str().unwrap_or("").to_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
#[ignore = "requires an explicitly observer-enabled validation project and a desktop"]
fn open_live_at_a_step_replays_cached_edges_headed_and_stops_open_at_a_drifted_edge() {
    let toolchain = PathBuf::from(
        std::env::var_os("GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT")
            .expect("OBSERVER_MISSING: GRAPHHELM_JOURNEY_TOOLCHAIN_PROJECT"),
    );
    assert!(
        toolchain
            .join("node_modules/@playwright/test/package.json")
            .is_file(),
        "OBSERVER_MISSING: project Playwright package"
    );
    let scratch = tempfile::tempdir().unwrap();
    let project = scratch.path().join("project");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(
        project.join("package.json"),
        "{\"name\":\"live-fixture\",\"private\":true,\"type\":\"module\"}",
    )
    .unwrap();
    std::fs::write(project.join("fixture-server.mjs"), FIXTURE).unwrap();
    let linked = Command::new("node")
        .args([
            "-e",
            "require('node:fs').symlinkSync(process.argv[1],process.argv[2],process.platform==='win32'?'junction':'dir')",
        ])
        .arg(toolchain.join("node_modules"))
        .arg(project.join("node_modules"))
        .status()
        .unwrap();
    assert!(linked.success(), "OBSERVER_MISSING: toolchain link");
    let mut command = Command::new("node");
    command.arg(project.join("fixture-server.mjs"));
    let mut app = Server::start(command);
    let base = app.started["base"].as_str().unwrap().to_owned();

    let source = project.join(".graphhelm/journeys");
    std::fs::create_dir_all(&source).unwrap();
    let mut flow: Value = serde_yaml_ng::from_str(FLOW).unwrap();
    flow["base"] = base.into();
    for screen in flow["screens"].as_array_mut().unwrap() {
        screen["scope"] = json!(["fixture-server.mjs"]);
    }
    // The cart's own expectation names only its heading, so a renamed button shows up where the
    // spec puts it: on the edge that clicks it (`cart.checkout/0`), not on the cart screen.
    flow["screens"][0]["expect"] = json!([{ "role":"heading","name":"Cart" }]);
    flow["screens"][1]["expect"] = json!([{ "role":"heading","name":"Order 42" }]);
    flow["edges"][1]["acts"][1]["name"] = "Submit order".into();
    std::fs::write(
        source.join("checkout.journey.yaml"),
        serde_yaml_ng::to_string(&flow).unwrap(),
    )
    .unwrap();
    std::fs::write(
        project.join(".gitignore"),
        "node_modules/\n.graphhelm/observers/\n.graphhelm/journey-cache/\n.graphhelm/journey-sessions/\n",
    )
    .unwrap();
    let observer = project.join(".graphhelm/observers");
    std::fs::create_dir(&observer).unwrap();
    std::fs::write(observer.join("journey_driver.mjs"), DRIVER).unwrap();
    let (code, value) = journey(
        &project,
        &["compile", "checkout", "--fmt", "--include-draft"],
    );
    assert_eq!(code, 0, "{value}");
    git(&project, &["init", "--quiet", "--object-format=sha1"]);
    git(&project, &["add", "."]);
    git(
        &project,
        &["commit", "--quiet", "--no-verify", "-m", "fixture"],
    );
    let token = owner_token(&project);
    let (code, value) = journey(&project, &["approve", "checkout", "--token-file", &token]);
    assert_eq!(code, 0, "{value}");

    // The replay cache is what makes the live walk deterministic.
    let (code, value) = journey(&project, &["replay", "checkout"]);
    assert_eq!(code, 0, "replay must pass first: {value}");

    // Open at step 2 (`pay`): one cached edge replayed headed, the step's expectations hold.
    let (code, opened) = journey(&project, &["open", "checkout", "--step", "pay"]);
    assert_eq!(code, 0, "{opened}");
    assert_eq!(opened["command"], "journey.open");
    assert_eq!(opened["data"]["state"], "pass", "{opened}");
    assert_eq!(opened["data"]["headed"], true);
    assert_eq!(opened["data"]["modelCalls"], 0);
    let session = opened["data"]["sessionId"].as_str().unwrap().to_owned();
    // The session's state is readable without contacting the host (the Studio chip's source).
    let (code, listed) = journey(&project, &["sessions"]);
    assert_eq!(code, 0, "{listed}");
    let row = &listed["data"]["sessions"][0];
    assert_eq!(row["sessionId"], session.as_str(), "{listed}");
    assert_eq!(row["contractId"], "checkout", "{listed}");
    assert_eq!(row["stepId"], "pay", "{listed}");
    assert_eq!(row["state"], "pass", "{listed}");
    assert!(
        row.get("port").is_none() && row.get("pid").is_none(),
        "{listed}"
    );
    // The owner or an agent continues from the open step through the same session.
    let (code, acted) = journey(
        &project,
        &[
            "act",
            &session,
            "--kind",
            "enter_text",
            "--role",
            "textbox",
            "--name",
            "Password",
            "--secret",
            "shopper_password",
        ],
    );
    assert_eq!(code, 0, "{acted}");
    assert_eq!(acted["data"]["screen"], "pay", "{acted}");
    assert_eq!(acted["data"]["state"], "pass", "{acted}");
    assert!(
        !acted.to_string().contains(SECRET),
        "the secret never echoes"
    );
    // A destructive-looking name the flow does not have on this edge is refused.
    let (code, refused) = journey(
        &project,
        &[
            "act",
            &session,
            "--kind",
            "activate",
            "--role",
            "button",
            "--name",
            "Delete account",
        ],
    );
    assert_ne!(code, 0, "{refused}");
    assert!(
        codes(&refused).contains(&"live.act_refused_destructive".to_owned()),
        "{refused}"
    );
    let (code, closed) = journey(&project, &["close", &session]);
    assert_eq!(code, 0, "{closed}");
    assert_eq!(closed["data"]["closed"], true);
    assert!(
        !project
            .join(format!(".graphhelm/journey-sessions/{session}.json"))
            .exists(),
        "close removes the session record"
    );
    let (_, listed) = journey(&project, &["sessions"]);
    assert_eq!(listed["data"]["sessions"], json!([]), "{listed}");

    // Rename the cart's button: the walk stops at the broken edge and the browser stays there.
    assert_eq!(app.control(r#"{"kind":"rename-checkout"}"#)["armed"], true);
    let (code, drifted) = journey(&project, &["open", "checkout", "--step", "pay"]);
    assert_eq!(code, 1, "{drifted}");
    assert_eq!(drifted["data"]["state"], "drift", "{drifted}");
    assert_eq!(drifted["data"]["at"], "cart.checkout/0", "{drifted}");
    assert!(
        codes(&drifted).contains(&"drift.locator_missing".to_owned()),
        "{drifted}"
    );
    let session = drifted["data"]["sessionId"].as_str().unwrap().to_owned();
    let (code, still) = journey(
        &project,
        &[
            "act", &session, "--kind", "inspect", "--role", "heading", "--name", "Cart",
        ],
    );
    assert_eq!(code, 0, "the session survives the drift: {still}");
    assert_eq!(still["data"]["screen"], "cart", "{still}");
    let (code, closed) = journey(&project, &["close", &session]);
    assert_eq!(code, 0, "{closed}");
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
