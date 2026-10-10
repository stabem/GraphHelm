//! #519 (coordinator's decision on #548): opening an APPROVED journey plays it through
//! `play_path` up to its first destructive act, holds there, and plays that act only on the run
//! the owner confirmed. Real CLI, real driver and Chromium, and an independent static app whose
//! server counts the destructive POST: the count, not the CLI's own report, says whether the act
//! reached the app. Catches the guard dropped or widened, a held run that still sends the act,
//! and a confirm that does not play it.
//! Cost: ~30s plus build, Node/Playwright/Chromium explicitly installed, local ports/Git; no
//! provider/account or network installation. Ordinary offline runs ignore this target.
#[path = "support/time_scale.rs"]
mod time_scale;
use time_scale::scaled;

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

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
                let Ok(line) = line else { break };
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

    /// How many times the app received the destructive POST.
    fn deletes(&mut self) -> u64 {
        writeln!(self.input, "counts").unwrap();
        self.input.flush().unwrap();
        self.replies.recv_timeout(Duration::from_secs(5)).unwrap()["deletes"]
            .as_u64()
            .unwrap()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = graphhelm_process_tree::terminate(self.child.id(), self.group);
        graphhelm_process_tree::close(&mut self.group);
        let _ = self.child.wait();
    }
}

fn cli(project: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .arg("--json")
        .args(args)
        .arg("--project")
        .arg(project)
        .output()
        .unwrap();
    let value = serde_json::from_slice(&out.stdout).expect("CLI must return its JSON envelope");
    (out.status.code().unwrap(), value)
}

fn git(project: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .arg("-C")
        .arg(project)
        .args([
            "-c",
            "user.name=Preview observer",
            "-c",
            "user.email=preview@example.invalid",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "Git fixture operation failed");
}

/// Reads the kept result until the background run leaves `running`, within a bound.
fn settled(project: &Path) -> Value {
    let deadline = Instant::now() + scaled(Duration::from_secs(180));
    loop {
        let (code, value) = cli(project, &["journey", "preview", "account", "--read"]);
        assert_eq!(code, 0, "{value}");
        if value["data"]["state"] != "running" {
            return value["data"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "the preview never ended: {value}"
        );
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[test]
#[ignore = "requires an explicitly observer-enabled validation project"]
fn an_approved_journey_holds_before_its_destructive_act_and_plays_it_only_when_confirmed() {
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
        "{\"name\":\"preview-fixture\",\"private\":true,\"type\":\"module\"}",
    )
    .unwrap();
    std::fs::write(project.join("fixture-server.mjs"), FIXTURE).unwrap();
    // Only this test-owned project links the explicitly declared installed toolchain.
    let linked = Command::new("node")
        .args(["-e", "require('node:fs').symlinkSync(process.argv[1],process.argv[2],process.platform==='win32'?'junction':'dir')"])
        .arg(toolchain.join("node_modules"))
        .arg(project.join("node_modules"))
        .status()
        .unwrap();
    assert!(linked.success(), "OBSERVER_MISSING: toolchain link");
    let mut command = Command::new("node");
    command.arg(project.join("fixture-server.mjs"));
    let mut app = Server::start(command);
    let base = app.started["base"].as_str().unwrap().to_owned();
    assert_eq!(app.deletes(), 0);

    // One edge, one destructive act: "Delete account" POSTs /account/delete, which the app counts.
    let flow = json!({
        "schema": "graphhelm.journey-flow/1",
        "id": "account",
        "title": "Owner deletes the account",
        "status": "draft",
        "approved": null,
        "base": base,
        "actors": ["owner"],
        "secrets": [],
        "risks": ["irreversible_effect"],
        "screens": [
            {"id":"account","url":"/account","state":"stable",
             "expect":[{"role":"heading","name":"Account"}],"scope":["fixture-server.mjs"]},
            {"id":"deleted","url":"/account/delete","state":"success",
             "expect":[{"role":"heading","name":"Account deleted"}],"scope":["fixture-server.mjs"]}
        ],
        "edges": [
            {"id":"account.delete","from":"account","to":"deleted",
             "acts":[{"kind":"submit","role":"button","name":"Delete account"}]}
        ],
        "paths": {"main": ["account.delete"]},
        "drift": []
    });
    let source = project.join(".graphhelm/journeys");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(
        source.join("account.journey.yaml"),
        serde_yaml_ng::to_string(&flow).unwrap(),
    )
    .unwrap();
    std::fs::write(
        project.join(".gitignore"),
        "node_modules/\n.graphhelm/observers/\n.graphhelm/journey-cache/\n.graphhelm/journey-previews/\n",
    )
    .unwrap();
    let observer = project.join(".graphhelm/observers");
    std::fs::create_dir(&observer).unwrap();
    std::fs::write(observer.join("journey_driver.mjs"), DRIVER).unwrap();
    let (code, value) = cli(
        &project,
        &["journey", "compile", "account", "--fmt", "--include-draft"],
    );
    assert_eq!(code, 0, "{value}");
    git(&project, &["init", "--quiet", "--object-format=sha1"]);
    git(&project, &["add", "."]);
    git(
        &project,
        &["commit", "--quiet", "--no-verify", "-m", "fixture"],
    );
    let (code, value) = cli(&project, &["journey", "approve", "account"]);
    assert_eq!(code, 0, "{value}");
    git(&project, &["add", "."]);
    git(
        &project,
        &["commit", "--quiet", "--no-verify", "-m", "approved flow"],
    );

    // Opening the approved journey plays it and holds before the destructive act.
    let (code, value) = cli(&project, &["journey", "preview", "account"]);
    assert_eq!(code, 0, "{value}");
    let held = settled(&project);
    assert_eq!(held["kind"], "replay", "{held}");
    assert_eq!(held["state"], "ready", "{held}");
    assert_eq!(held["screens"]["account"]["result"], "pass", "{held}");
    assert_eq!(
        held["edges"]["account.delete"]["result"], "skipped",
        "{held}"
    );
    assert_eq!(
        held["edges"]["account.delete"]["reason"], "confirm_needed",
        "{held}"
    );
    assert_eq!(held["held"]["edge"], "account.delete", "{held}");
    assert_eq!(held["held"]["act"], "Delete account", "{held}");
    assert_eq!(held["held"]["base"], base.as_str(), "{held}");
    assert_eq!(
        held["screens"]["deleted"]["reason"], "not_reached",
        "{held}"
    );
    assert_eq!(app.deletes(), 0, "a held act reached the app");

    // A later open without the click answers the kept result and sends nothing.
    let (code, value) = cli(&project, &["journey", "preview", "account"]);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["data"]["held"]["edge"], "account.delete", "{value}");
    assert_eq!(app.deletes(), 0, "reopening sent the held act");

    // The owner's click: this run plays the act, once.
    let (code, value) = cli(&project, &["journey", "preview", "account", "--confirm"]);
    assert_eq!(code, 0, "{value}");
    let played = settled(&project);
    assert_eq!(played["state"], "ready", "{played}");
    assert!(played.get("held").is_none_or(Value::is_null), "{played}");
    assert_eq!(
        played["edges"]["account.delete"]["result"], "pass",
        "{played}"
    );
    assert_eq!(played["screens"]["deleted"]["result"], "pass", "{played}");
    assert_eq!(
        app.deletes(),
        1,
        "the confirmed act did not reach the app once"
    );
}
