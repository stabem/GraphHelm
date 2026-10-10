//! #519 (coordinator's decision on #548): opening an APPROVED journey plays it through
//! `play_path` up to its first destructive act, holds there, and plays that act only on the run
//! the owner confirmed. Real CLI, real driver and Chromium, and an independent static app whose
//! server counts the destructive POST: the count, not the CLI's own report, says whether the act
//! reached the app. Catches the guard dropped or widened, a held run that still sends the act,
//! and a confirm that does not play it.
//! Cost: ~30s plus build, Node/Playwright/Chromium explicitly installed, local ports/Git; no
//! provider/account or network installation. Ordinary offline runs ignore this target.
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sha2::Digest;

const FIXTURE: &str = include_str!("../../../tools/journey-driver/fixture-server.mjs");
const DRIVER: &[u8] = include_bytes!("../../../tools/journey-driver/driver.mjs");

struct OwnedNodeModulesLink(PathBuf);

impl Drop for OwnedNodeModulesLink {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _ = std::fs::remove_dir(&self.0);
        #[cfg(not(windows))]
        let _ = std::fs::remove_file(&self.0);
    }
}

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
    let deadline = Instant::now() + Duration::from_secs(180);
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
    let node_modules_link = project.join("node_modules");
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
        .arg(&node_modules_link)
        .status()
        .unwrap();
    assert!(linked.success(), "OBSERVER_MISSING: toolchain link");
    let _owned_node_modules_link = OwnedNodeModulesLink(node_modules_link);
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

/// The validation command must observe a real browser script and preserve the distinction
/// between a function called by the page and one loaded but untouched.  A plausible regression
/// is treating a zero count in generated coverage as proof that the original source is dead, or
/// losing the source hash association when the launcher uses a fresh app port.  Existing preview
/// coverage has no report consumer and therefore cannot catch either classification.  Cost: one
/// isolated Node launcher, one Chromium page, and one CLI child; ignored unless the observer
/// toolchain is explicitly enabled.
#[test]
#[ignore = "requires an explicitly observer-enabled validation project"]
fn keel_validation_reports_called_and_untouched_fixture_functions_with_current_source_hash() {
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
    std::fs::create_dir_all(project.join(".graphhelm/journeys")).unwrap();
    std::fs::create_dir_all(project.join(".graphhelm/observers")).unwrap();
    std::fs::write(
        project.join("package.json"),
        "{\"name\":\"keel-validation-fixture\",\"private\":true,\"type\":\"module\"}",
    )
    .unwrap();
    let linked = Command::new("node")
        .args(["-e", "require('node:fs').symlinkSync(process.argv[1],process.argv[2],process.platform==='win32'?'junction':'dir')"])
        .arg(toolchain.join("node_modules"))
        .arg(project.join("node_modules"))
        .status()
        .unwrap();
    assert!(linked.success(), "OBSERVER_MISSING: toolchain link");
    std::fs::write(
        project.join(".graphhelm/observers/journey_driver.mjs"),
        DRIVER,
    )
    .unwrap();

    // The source is served byte-for-byte by the isolated launcher. Coverage can therefore match
    // its generated script to this current tracked file by SHA-256.
    std::fs::write(
        project.join("app.js"),
        "function calledFunction() { document.body.dataset.called = 'yes'; }\nfunction untouchedFunction() { return 'never'; }\ncalledFunction();\ndocument.querySelector('button').addEventListener('click', calledFunction);\n",
    )
    .unwrap();
    std::fs::write(
        project.join("fixture-server.mjs"),
        r#"import {createServer} from 'node:http';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
const [mode, dir, _runtimePort, appPort] = process.argv.slice(2);
if (mode === 'up') {
  mkdirSync(dir, {recursive:true});
  const server = createServer((req, res) => {
    if (req.url === '/app.js') { res.setHeader('content-type','application/javascript'); res.end(readFileSync('app.js')); return; }
    res.setHeader('content-type','text/html; charset=utf-8');
    res.end('<!doctype html><main><h1>Coverage fixture</h1><button>Run</button><script src="/app.js"></script></main>');
  });
  server.listen(Number(appPort), '127.0.0.1');
  writeFileSync(`${dir}/pid`, String(process.pid));
  process.on('SIGTERM', () => server.close(() => process.exit(0)));
} else if (mode === 'down') {
  try { process.kill(Number(readFileSync(`${dir}/pid`, 'utf8')), 'SIGTERM'); } catch {}
}
"#,
    )
    .unwrap();
    std::fs::write(
        project.join("fixture.sh"),
        "#!/bin/sh\nexec node fixture-server.mjs \"$@\"\n",
    )
    .unwrap();
    std::fs::write(
        project.join(".graphhelm/journey-fixture.json"),
        r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":true}"#,
    )
    .unwrap();
    let preview_sentinel = project.join(".graphhelm/journey-previews/coverage/state.json");
    std::fs::create_dir_all(preview_sentinel.parent().unwrap()).unwrap();
    std::fs::write(&preview_sentinel, br#"{"sentinel":"ordinary-preview"}"#).unwrap();
    std::fs::write(
        project.join(".gitignore"),
        "node_modules/\n.graphhelm/journey-previews/\n.graphhelm/observers/\n",
    )
    .unwrap();

    let flow = json!({
        "schema": "graphhelm.journey-flow/1",
        "id": "coverage",
        "title": "Coverage fixture",
        "status": "draft",
        "approved": null,
        "base": "http://127.0.0.1:3000",
        "actors": ["owner"],
        "secrets": [],
        "risks": [],
        "screens": [
            {"id":"start","url":"/","state":"stable",
             "expect":[{"role":"heading","name":"Coverage fixture"}],"scope":["app.js"]},
            {"id":"after","url":"/","state":"stable",
             "expect":[{"role":"heading","name":"Coverage fixture"}],"scope":["app.js"]}
        ],
        "edges": [
            {"id":"run","from":"start","to":"after",
             "acts":[{"kind":"activate","role":"button","name":"Run"}]}
        ],
        "paths": {"main": ["run"]},
        "drift": []
    });
    std::fs::write(
        project.join(".graphhelm/journeys/coverage.journey.yaml"),
        serde_yaml_ng::to_string(&flow).unwrap(),
    )
    .unwrap();
    let (compile_code, compiled) = cli(
        &project,
        &["journey", "compile", "coverage", "--fmt", "--include-draft"],
    );
    assert_eq!(
        compile_code, 0,
        "fixture must be a canonical valid flow: {compiled}"
    );
    git(&project, &["init", "--quiet", "--object-format=sha1"]);
    git(&project, &["add", "."]);
    git(
        &project,
        &["commit", "--quiet", "--no-verify", "-m", "coverage fixture"],
    );

    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "keel", "test", "validation", "--repo"])
        .arg(&project)
        .output()
        .unwrap();
    let envelope: Value =
        serde_json::from_slice(&output.stdout).expect("validation must return its JSON envelope");
    assert_eq!(output.status.code(), Some(0), "{envelope}");
    let data = &envelope["data"];
    assert_eq!(data["failures"], json!([]), "{data}");
    assert!(
        data["unknown"].as_array().unwrap().iter().any(|item| {
            item["source"] == "fixture-server.mjs"
                || item["path"] == "fixture-server.mjs"
                || item["reason"] == "backend_or_native_unobserved"
        }),
        "tracked backend launcher must remain unknown: {}",
        data["unknown"]
    );
    assert_eq!(data["deadConfirmed"], json!([]), "{data}");
    let used = data["used"].as_array().unwrap();
    let untouched = data["notObserved"].as_array().unwrap();
    let called = used
        .iter()
        .find(|item| item["name"] == "calledFunction")
        .expect("called generated function must be used");
    assert_eq!(called["source"], "app.js", "{called}");
    assert_eq!(
        called["sourceSha256"],
        format!(
            "{:x}",
            sha2::Sha256::digest(std::fs::read(project.join("app.js")).unwrap())
        ),
        "source association must use the current file hash"
    );
    assert!(
        untouched
            .iter()
            .any(|item| item["name"] == "untouchedFunction" && item["source"] == "app.js"),
        "an untouched generated function must be a review candidate: {untouched:?}"
    );

    assert_eq!(
        std::fs::read(&preview_sentinel).unwrap(),
        br#"{"sentinel":"ordinary-preview"}"#,
        "validation must not rewrite the ordinary preview cache"
    );

    // A non-isolated launcher is refused before browser execution and cannot consume the
    // ordinary preview cache. This is a separate bounded refusal check on the same fixture.
    std::fs::write(
        project.join(".graphhelm/journey-fixture.json"),
        r#"{"schema":"graphhelm-journey-fixture/1","script":"fixture.sh","isolated":false}"#,
    )
    .unwrap();
    let refused = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "keel", "test", "validation", "--repo"])
        .arg(&project)
        .output()
        .unwrap();
    let refused_envelope: Value = serde_json::from_slice(&refused.stdout)
        .expect("refused validation must return its JSON envelope");
    assert_ne!(
        refused.status.code(),
        Some(0),
        "non-isolated launcher must not pass: {refused_envelope}"
    );
    assert!(
        refused_envelope["data"]["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["reason"] == "execution_failed"
                    || item["reason"] == "launcher_not_isolated"
                    || item["code"] == "preview.launcher_not_isolated"
            }),
        "non-isolated launcher refusal must be reported: {}",
        refused_envelope["data"]
    );
    assert_eq!(
        std::fs::read(&preview_sentinel).unwrap(),
        br#"{"sentinel":"ordinary-preview"}"#,
        "refused validation must preserve the ordinary preview cache"
    );
}
