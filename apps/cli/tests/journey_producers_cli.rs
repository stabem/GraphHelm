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
        // #319: the Governor's "rejected" is the normal verdict for evidence; the reply says
        // "recorded" in words beside it.
        assert_eq!(reply["data"]["outcome"], "recorded", "{reply}");
        assert_eq!(reply["data"]["rejectionReason"], "signal_not_actionable");
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
    assert_eq!(reply["data"]["outcome"], "recorded", "{reply}");
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

/// #347: explicit citations must survive a newer unrelated observation. The existing
/// newest-pair test misses that race. Public producers + fresh reader, no new seam;
/// cost: ~2 seconds, offline Git/event-store processes.
#[test]
fn walked_pins_the_selected_pair_and_refuses_partial_foreign_or_wrong_step_ids() {
    assert!(git_available(), "Git is required by this observer");
    let harness = prepared();
    // Real capture with the same contract/step but a different execution, not an unknown id.
    let graph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/graphs/manual-override-deploy.yaml");
    let started = graphhelm()
        .args(["execution", "start", "--events"])
        .arg(&harness.events)
        .args(["--execution", "foreign-producer-run", "--file"])
        .arg(&graph)
        .arg("--fixtures")
        .arg(harness.scratch.path().join("fixtures.json"))
        .args(["--mode", "manual", "--held"])
        .output()
        .unwrap();
    assert!(started.status.success(), "{}", envelope(&started));
    let foreign = graphhelm()
        .args(["journey", "capture", "--events"])
        .arg(&harness.events)
        .args(["--execution", "foreign-producer-run", "--keyring"])
        .arg(&harness.keyring)
        .args(["--key-id", KEY_ID, "--project"])
        .arg(&harness.project)
        .args(["--contract", "cart", "--step", "open-cart", "--image"])
        .arg(&harness.image)
        .output()
        .unwrap();
    let foreign_reply = envelope(&foreign);
    assert!(foreign.status.success(), "{foreign_reply}");
    assert_eq!(foreign_reply["data"]["outcome"], "recorded");
    let foreign_id = foreign_reply["data"]["signalId"].as_str().unwrap();
    let source = harness.project.join("web/cart/Line.tsx");
    std::fs::write(&source, "dirty observation").unwrap();
    let selected = harness.capture("open-cart", &[]);
    std::fs::write(&source, "line").unwrap();
    let newest = harness.capture("open-cart", &[]);
    let review = harness.capture("review", &[]);
    let before = snapshot(&harness.events);
    for (from, to) in [
        (Some(selected.as_str()), None),
        (None, Some(review.as_str())),
        (Some("unknown-capture-id"), Some(review.as_str())),
        (Some(foreign_id), Some(review.as_str())),
        (Some(review.as_str()), Some(newest.as_str())),
    ] {
        let mut command = harness.record("walked");
        command.args([
            "--contract",
            "cart",
            "--from",
            "open-cart",
            "--to",
            "review",
        ]);
        if let Some(id) = from {
            command.args(["--from-capture", id]);
        }
        if let Some(id) = to {
            command.args(["--to-capture", id]);
        }
        let out = command.output().unwrap();
        let reply = harness.refuse_unchanged(&out, &before);
        assert_eq!(reply["command"], "journey.walked", "{reply}");
    }
    let out = harness
        .record("walked")
        .args([
            "--contract",
            "cart",
            "--from",
            "open-cart",
            "--to",
            "review",
            "--from-capture",
            &selected,
            "--to-capture",
            &review,
        ])
        .output()
        .unwrap();
    let reply = envelope(&out);
    assert!(out.status.success(), "{reply}");
    assert_eq!(reply["data"]["outcome"], "recorded");
    // The newest capture is fresh, while the explicitly cited dirty capture is not.
    // If the producer substitutes newest, the independent reader reports walked.
    let reader = harness.journeys();
    assert_eq!(
        harness.step_view("open-cart")["capture"]["signalId"],
        newest
    );
    assert_eq!(
        reader["journeys"][0]["arrows"][0]["state"], "stale",
        "{reader}"
    );
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

impl Harness {
    /// `keel check` over `HEAD~1..HEAD` of the project with a card naming the `cart` journey.
    fn keel_check(&self, scope: &[&str], with_records: bool) -> (i32, Value) {
        let card = self.scratch.path().join("card.json");
        std::fs::write(
            &card,
            serde_json::to_vec(&json!({
                "promise": "the cart renders", "scopePaths": scope,
                "proof": "npx playwright test", "journeys": ["cart"],
            }))
            .unwrap(),
        )
        .unwrap();
        let mut command = graphhelm();
        command
            .args([
                "--json",
                "keel",
                "check",
                "--diff",
                "HEAD~1..HEAD",
                "--repo",
            ])
            .arg(&self.project)
            .arg("--card")
            .arg(&card);
        if with_records {
            command
                .arg("--events")
                .arg(&self.events)
                .args(["--execution", RUN, "--keyring"])
                .arg(&self.keyring)
                .args(["--key-id", KEY_ID]);
        }
        let output = command.output().unwrap();
        (output.status.code().unwrap(), envelope(&output))
    }
}

fn diagnostics_with(reply: &Value, code: &str) -> Vec<String> {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == code)
        .map(|d| d["message"].as_str().unwrap().to_owned())
        .collect()
}

/// #382 phase A: a diff touching a journey's screen must name that journey in its card, and the
/// journey must have been replayed green at the head. Credible regression: a screen change ships
/// with a card that never mentions its journey, or with a stale capture, and nothing says so.
/// Existing coverage only checks freshness for journeys the card already names. Cost: the shared
/// temp project, a few CLI runs.
#[test]
fn keel_check_names_a_touched_journey_the_card_omits_and_a_replay_missing_at_the_head() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    std::fs::write(harness.project.join("web/cart/Line.tsx"), "line 2").unwrap();
    git(&harness.project, &["commit", "-qam", "edit cart"]);

    let card = harness.scratch.path().join("plain-card.json");
    std::fs::write(
        &card,
        serde_json::to_vec(&json!({"promise": "the cart renders",
            "scopePaths": ["web/cart/Line.tsx"], "proof": "npx playwright test"}))
        .unwrap(),
    )
    .unwrap();
    let output = graphhelm()
        .args([
            "--json",
            "keel",
            "check",
            "--diff",
            "HEAD~1..HEAD",
            "--repo",
        ])
        .arg(&harness.project)
        .arg("--card")
        .arg(&card)
        .output()
        .unwrap();
    let reply = envelope(&output);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a signal, not a gate: {reply}"
    );
    let missing = diagnostics_with(&reply, "keel.journey.card_missing_journey");
    assert_eq!(missing.len(), 1, "{reply}");
    assert!(missing[0].contains("cart/open-cart"), "{reply}");

    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert_eq!(code, 0, "{reply}");
    assert!(
        diagnostics_with(&reply, "keel.journey.card_missing_journey").is_empty(),
        "{reply}"
    );
    let not_green = diagnostics_with(&reply, "keel.journey.replay_not_green");
    assert_eq!(not_green.len(), 1, "{reply}");
    assert!(
        not_green[0].contains("no clean capture taken at the head"),
        "{reply}"
    );

    harness.capture("open-cart", &[]);
    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert_eq!(code, 0, "{reply}");
    assert!(
        diagnostics_with(&reply, "keel.journey.replay_not_green").is_empty(),
        "a clean capture at the head is green: {reply}"
    );
}

fn journey_warnings(reply: &Value) -> Vec<String> {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["code"] == "keel.journey.no_fresh_capture")
        .map(|d| d["message"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn keel_check_warns_for_a_touched_screen_until_it_has_a_fresh_capture_at_the_head() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    std::fs::write(harness.project.join("web/cart/Line.tsx"), "line 2").unwrap();
    git(&harness.project, &["commit", "-qam", "edit cart"]);

    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], false);
    assert_eq!(code, 0, "{reply}");
    let warnings = journey_warnings(&reply);
    assert_eq!(warnings.len(), 1, "{reply}");
    assert!(warnings[0].contains("cart/open-cart"), "{reply}");
    assert!(
        warnings[0].contains("--events"),
        "the warning says captures were not read: {reply}"
    );

    let (_, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert!(
        journey_warnings(&reply)[0].contains("no capture"),
        "{reply}"
    );

    harness.capture("open-cart", &[]);
    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert_eq!(code, 0, "{reply}");
    assert!(
        journey_warnings(&reply).is_empty(),
        "a fresh capture at the head silences it: {reply}"
    );

    std::fs::write(harness.project.join("web/cart/Line.tsx"), "line 3").unwrap();
    git(&harness.project, &["commit", "-qam", "edit cart again"]);
    let (code, reply) = harness.keel_check(&["web/cart/Line.tsx"], true);
    assert_eq!(code, 0, "{reply}");
    let warnings = journey_warnings(&reply);
    assert!(
        warnings[0].contains("web/cart/Line.tsx"),
        "a stale capture names the changed file: {reply}"
    );
}

#[test]
fn keel_check_warns_on_a_missing_contract_and_ignores_untouched_screens() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    std::fs::write(harness.project.join("README.md"), "x").unwrap();
    git(&harness.project, &["add", "-A"]);
    git(&harness.project, &["commit", "-qm", "readme"]);
    let (code, reply) = harness.keel_check(&["README.md"], false);
    assert_eq!(code, 0, "{reply}");
    assert!(
        journey_warnings(&reply).is_empty(),
        "README.md touches no screen: {reply}"
    );
    std::fs::remove_file(harness.project.join(".graphhelm/journeys/cart.json")).unwrap();
    let (code, reply) = harness.keel_check(&["README.md"], false);
    assert_eq!(code, 0, "{reply}");
    assert!(
        reply["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "keel.journey.contract_unreadable" && d["severity"] == "warning"),
        "{reply}"
    );
}

/// #382 phase B: a plan recorded on a run is what the briefing hands the next agent. Credible
/// regression: the briefing drops or recomputes the plan, or serves a plan it cannot open. No
/// existing test reads a signal back into the briefing. Cost: the shared temp project, three CLI
/// runs.
#[test]
fn a_recorded_keel_plan_reaches_the_briefing() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    let output = graphhelm()
        .args([
            "--json",
            "keel",
            "plan",
            "--task",
            "issue-7",
            "--paths",
            "web/cart/Line.tsx",
            "--repo",
        ])
        .arg(&harness.project)
        .arg("--events")
        .arg(&harness.events)
        .args(["--execution", RUN, "--keyring"])
        .arg(&harness.keyring)
        .args(["--key-id", KEY_ID])
        .output()
        .unwrap();
    let recorded = envelope(&output);
    assert_eq!(output.status.code(), Some(0), "{recorded}");
    assert_eq!(
        recorded["data"]["plan"]["journeys"],
        json!(["cart"]),
        "{recorded}"
    );
    assert_eq!(recorded["data"]["plan"]["proof"], "journey");

    let briefing = |with_keyring: bool| {
        let mut command = graphhelm();
        command
            .args(["--json", "execution", "briefing", "--events"])
            .arg(&harness.events)
            .args(["--execution", RUN]);
        if with_keyring {
            command
                .arg("--keyring")
                .arg(&harness.keyring)
                .args(["--key-id", KEY_ID]);
        }
        envelope(&command.output().unwrap())
    };
    let read = briefing(true);
    assert_eq!(read["data"]["plan"], recorded["data"]["plan"], "{read}");
    assert!(
        briefing(false)["data"].get("plan").is_none(),
        "without a keyring the briefing omits the plan rather than guessing"
    );
}

/// #561: the CLI door of the same promise. `keel plan --events` records the plan on the run as
/// the owner; `graph synthesize --execution` on that run, with no `--critic` and no `--plan`,
/// puts the graded design in front and names the run as the source. Before the plan is recorded
/// the same command is the draft alone. Credible regression: the flags parsed and never read (the
/// run's plan ignored on the door a lane's terminal uses). Cost: the shared temp project, one
/// `keel plan`, two synthesize runs.
#[test]
fn graph_synthesize_inside_a_run_reads_the_plan_keel_plan_recorded_there() {
    if !git_available() {
        return;
    }
    let harness = prepared();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../core/architect/fixtures");
    let goal = std::fs::read_to_string(fixtures.join("first-compile/GOAL.txt")).unwrap();
    let synthesize = |name: &str| {
        let output = graphhelm()
            .args([
                "--json",
                "graph",
                "synthesize",
                "--goal",
                goal.trim_end(),
                "--out",
            ])
            .arg(harness.scratch.path().join(name))
            .args(["--allow-program", "cargo", "--fixture"])
            .arg(fixtures.join("first-compile/replies.json"))
            .arg("--events")
            .arg(&harness.events)
            .args(["--execution", RUN, "--keyring"])
            .arg(&harness.keyring)
            .args(["--key-id", KEY_ID])
            .output()
            .unwrap();
        let reply = envelope(&output);
        assert_eq!(reply["ok"], true, "{reply}");
        reply
    };
    let before = synthesize("before.json");
    assert!(before["data"].get("critic").is_none(), "{before}");
    assert_ne!(
        before["data"]["document"]["spec"]["entrypoints"],
        json!(["critic_design"])
    );

    let output = graphhelm()
        .args(["--json", "keel", "plan", "--task", "issue-561", "--paths"])
        .arg("core/events/src/journal.rs")
        .arg("--repo")
        .arg(&harness.project)
        .arg("--events")
        .arg(&harness.events)
        .args(["--execution", RUN, "--keyring"])
        .arg(&harness.keyring)
        .args(["--key-id", KEY_ID])
        .output()
        .unwrap();
    let recorded = envelope(&output);
    assert_eq!(output.status.code(), Some(0), "{recorded}");
    assert_eq!(
        recorded["data"]["plan"]["critic"]["mode"], "design",
        "{recorded}"
    );

    let after = synthesize("after.json");
    assert_eq!(
        after["data"]["document"]["spec"]["entrypoints"],
        json!(["critic_design"]),
        "{after}"
    );
    assert_eq!(
        after["data"]["critic"],
        json!({"mode": "design", "source": "run", "passScore": 8, "maxRounds": 3, "taskId": "issue-561"}),
        "{after}"
    );
}
