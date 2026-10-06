//! #315 Task 4: `graphhelm journey capture` and `graphhelm journey walked` record valid journey
//! documents that `graphhelm journeys` folds; every refusal happens before anything is appended.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::{Value, json};

const EVENTS_KEY: &str = "0101010101010101010101010101010101010101010101010101010101010101";
const KEY_ID: &str = "owner-key";
const RUN: &str = "journey-producers";

fn graphhelm() -> Command {
    let mut command = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env("GRAPHHELM_EVENTS_KEY", EVENTS_KEY);
    command
}

fn git_available() -> bool {
    match Command::new("git").arg("--version").output() {
        Ok(output) if output.status.success() => true,
        _ => {
            println!("skipped: `git --version` failed on this host");
            false
        }
    }
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

/// A PNG signature followed by an IHDR chunk declaring `width` x `height`.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(&13u32.to_be_bytes());
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

fn step(id: &str, scope: &str) -> Value {
    json!({
        "stepId": id,
        "actorId": "shopper",
        "semanticAction": {
            "kind": "navigate",
            "target": {"strategy": "visible_text", "value": "Cart", "geometryClaim": false}
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

fn contract() -> Value {
    json!({
        "contractId": "cart",
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

struct Harness {
    scratch: tempfile::TempDir,
    events: PathBuf,
    keyring: PathBuf,
    project: PathBuf,
    image: PathBuf,
    head: String,
}

/// A held manual run, an owner keyring, a 1280x720 PNG, and a committed project with `cart.json`.
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
    let image = scratch.path().join("shot.png");
    std::fs::write(&image, png(1280, 720)).unwrap();

    let project = scratch.path().join("project");
    let journeys = project.join(".graphhelm").join("journeys");
    std::fs::create_dir_all(&journeys).unwrap();
    std::fs::create_dir_all(project.join("web/cart")).unwrap();
    std::fs::write(project.join("web/cart/Line.tsx"), "line").unwrap();
    std::fs::write(
        journeys.join("cart.json"),
        serde_json::to_vec_pretty(&contract()).unwrap(),
    )
    .unwrap();
    git(&project, &["init", "-q"]);
    git(&project, &["add", "-A"]);
    git(&project, &["commit", "-q", "-m", "init"]);
    let head = git(&project, &["rev-parse", "HEAD"]);
    Harness {
        scratch,
        events,
        keyring,
        project,
        image,
        head,
    }
}

fn envelope(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
        panic!(
            "stdout is not JSON: {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

/// Every file under `dir`, path and bytes, sorted: the stream is unchanged when this is equal.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                out.push((path, bytes));
            }
        }
    }
    out.sort();
    out
}

impl Harness {
    fn record(&self, subcommand: &str) -> Command {
        let mut command = graphhelm();
        command
            .args(["journey", subcommand, "--events"])
            .arg(&self.events)
            .args(["--execution", RUN, "--keyring"])
            .arg(&self.keyring)
            .args(["--key-id", KEY_ID, "--project"])
            .arg(&self.project);
        command
    }

    fn capture_with(&self, contract: &str, step: &str, extra: &[&str]) -> Output {
        self.record("capture")
            .args(["--contract", contract, "--step", step, "--image"])
            .arg(&self.image)
            .args(extra)
            .output()
            .unwrap()
    }

    /// A successful capture; returns its signal id.
    fn capture(&self, step: &str, extra: &[&str]) -> String {
        let output = self.capture_with("cart", step, extra);
        let reply = envelope(&output);
        assert!(output.status.success(), "{reply}");
        assert_eq!(reply["command"], "journey.capture");
        assert_eq!(reply["data"]["kind"], "jpd.screen_captured");
        assert_eq!(reply["data"]["attachments"].as_array().unwrap().len(), 1);
        reply["data"]["signalId"].as_str().unwrap().to_owned()
    }

    fn walked(&self, from: &str, to: &str) -> Output {
        self.record("walked")
            .args(["--contract", "cart", "--from", from, "--to", to])
            .output()
            .unwrap()
    }

    fn journeys(&self) -> Value {
        let output = graphhelm()
            .args(["journeys", "--events"])
            .arg(&self.events)
            .args(["--execution", RUN, "--project"])
            .arg(&self.project)
            .arg("--keyring")
            .arg(&self.keyring)
            .args(["--key-id", KEY_ID])
            .output()
            .unwrap();
        let reply = envelope(&output);
        assert!(output.status.success(), "{reply}");
        reply["data"].clone()
    }

    fn step_view(&self, step: &str) -> Value {
        let data = self.journeys();
        assert_eq!(data["ignoredRecords"], 0, "{data}");
        data["journeys"][0]["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["stepId"] == step)
            .unwrap()
            .clone()
    }

    fn refuse_unchanged(&self, output: &Output, before: &[(PathBuf, Vec<u8>)]) -> Value {
        let reply = envelope(output);
        assert!(!output.status.success(), "{reply}");
        assert_eq!(reply["ok"], false, "{reply}");
        assert_eq!(snapshot(&self.events), before, "the stream changed");
        reply
    }
}

#[test]
fn a_clean_capture_records_head_the_png_viewport_and_one_image() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    let signal = harness.capture("open-cart", &["--pr", "7", "--phase", "after"]);
    let capture = &harness.step_view("open-cart")["capture"];
    assert_eq!(capture["signalId"], signal.as_str());
    assert_eq!(capture["revision"], harness.head.as_str());
    assert_eq!(capture["dirty"], false);
    assert_eq!(capture["viewport"], json!({"width": 1280, "height": 720}));
    assert_eq!(capture["pr"], 7);
    assert_eq!(capture["phase"], "after");
    assert_eq!(capture["observer"], "owner-cli");
    assert_eq!(
        capture["imageEvidenceId"],
        format!("signal-{signal}-image-1").as_str()
    );
    assert_eq!(capture["freshness"], "fresh");
}

#[test]
fn an_uncommitted_change_marks_the_capture_dirty() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    std::fs::write(harness.project.join("web/cart/Line.tsx"), "edited").unwrap();
    harness.capture("open-cart", &[]);
    let capture = &harness.step_view("open-cart")["capture"];
    assert_eq!(capture["dirty"], true);
    assert!(capture.get("pr").is_none());
}

#[test]
fn a_non_png_image_needs_an_explicit_viewport() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    let jpeg = harness.scratch.path().join("shot.jpg");
    std::fs::write(&jpeg, [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3]).unwrap();
    let before = snapshot(&harness.events);
    let output = harness
        .record("capture")
        .args(["--contract", "cart", "--step", "open-cart", "--image"])
        .arg(&jpeg)
        .output()
        .unwrap();
    harness.refuse_unchanged(&output, &before);
    let output = harness
        .record("capture")
        .args(["--contract", "cart", "--step", "open-cart", "--image"])
        .arg(&jpeg)
        .args(["--viewport", "390x844"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", envelope(&output));
    let capture = &harness.step_view("open-cart")["capture"];
    assert_eq!(capture["viewport"], json!({"width": 390, "height": 844}));
}

#[test]
fn path_like_ids_and_unknown_steps_are_refused_with_the_stream_unchanged() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    // A file the `../x` id would reach if it were ever used as a path.
    std::fs::write(
        harness.project.join(".graphhelm").join("x.json"),
        serde_json::to_vec(&contract()).unwrap(),
    )
    .unwrap();
    let before = snapshot(&harness.events);
    for bad in ["../x", "a/b"] {
        let output = harness.capture_with(bad, "open-cart", &[]);
        let reply = harness.refuse_unchanged(&output, &before);
        assert_eq!(reply["diagnostics"][0]["path"], "/contract", "{reply}");
        let output = harness.capture_with("cart", bad, &[]);
        let reply = harness.refuse_unchanged(&output, &before);
        assert_eq!(reply["diagnostics"][0]["path"], "/step", "{reply}");
        let output = harness.walked(bad, "review");
        let reply = harness.refuse_unchanged(&output, &before);
        assert_eq!(reply["diagnostics"][0]["path"], "/from", "{reply}");
    }
    let output = harness.capture_with("cart", "checkout", &[]);
    let reply = harness.refuse_unchanged(&output, &before);
    assert_eq!(reply["diagnostics"][0]["path"], "/step", "{reply}");
}

#[test]
fn walked_cites_the_newest_captures_and_the_fold_reports_it_walked() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    harness.capture("open-cart", &[]);
    let newest_open = harness.capture("open-cart", &[]);
    let review = harness.capture("review", &[]);
    let output = harness.walked("open-cart", "review");
    let reply = envelope(&output);
    assert!(output.status.success(), "{reply}");
    assert_eq!(reply["command"], "journey.walked");
    assert_eq!(reply["data"]["kind"], "jpd.transition_walked");
    let transition = reply["data"]["signalId"].as_str().unwrap().to_owned();

    let data = harness.journeys();
    assert_eq!(data["ignoredRecords"], 0, "{data}");
    let journey = &data["journeys"][0];
    let step = |id: &str| {
        journey["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|candidate| candidate["stepId"] == id)
            .unwrap()["capture"]["signalId"]
            .clone()
    };
    assert_eq!(step("open-cart"), newest_open.as_str());
    assert_eq!(step("review"), review.as_str());
    let arrow = &journey["arrows"][0];
    assert_eq!(arrow["fromStepId"], "open-cart");
    assert_eq!(arrow["toStepId"], "review");
    assert_eq!(arrow["state"], "walked", "{data}");
    assert_eq!(arrow["transitionSignalId"], transition.as_str());
}

#[test]
fn walked_refuses_a_missing_capture_and_non_consecutive_steps() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    harness.capture("open-cart", &[]);
    harness.capture("pay", &[]);
    let before = snapshot(&harness.events);
    let message = |reply: &Value| {
        reply["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let reply = harness.refuse_unchanged(&harness.walked("open-cart", "review"), &before);
    assert!(message(&reply).contains("no capture"), "{reply}");
    let reply = harness.refuse_unchanged(&harness.walked("open-cart", "pay"), &before);
    assert!(message(&reply).contains("consecutive"), "{reply}");
    let reply = harness.refuse_unchanged(&harness.walked("review", "open-cart"), &before);
    assert!(message(&reply).contains("consecutive"), "{reply}");
}

#[test]
fn a_committed_change_in_scope_makes_the_capture_stale_naming_the_file() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    harness.capture("open-cart", &[]);
    assert_eq!(
        harness.step_view("open-cart")["capture"]["freshness"],
        "fresh"
    );
    std::fs::write(harness.project.join("web/cart/Line.tsx"), "changed").unwrap();
    git(
        &harness.project,
        &["commit", "-q", "-am", "change the cart"],
    );
    let capture = &harness.step_view("open-cart")["capture"];
    assert_eq!(capture["freshness"], "stale", "{capture}");
    assert_eq!(capture["changedFiles"], json!(["web/cart/Line.tsx"]));
}
