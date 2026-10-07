//! #347: replay refuses unsafe input before browser I/O. Credible defects are consuming
//! a draft/stale projection or discarding malformed cache bytes. Phase-1 observers never
//! consume a replay cache. Real CLI/tempdir/Git boundary, no test-only production seam;
//! cost: seconds after build, offline, no Node/browser/provider required.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");

fn cli(project: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "journey"])
        .args(args)
        .arg("--project")
        .arg(project)
        .env("GRAPHHELM_SECRET_shopper_password", "replay_canary_831597")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

fn project(approved: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for path in [
        "app/cart/page.tsx",
        "app/checkout/page.tsx",
        "app/api/pay/route.ts",
    ] {
        let file = dir.path().join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "export {}").unwrap();
    }
    std::fs::create_dir_all(dir.path().join(".graphhelm/journeys")).unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/journeys/checkout.journey.yaml"),
        FLOW,
    )
    .unwrap();
    let (code, reply) = cli(
        dir.path(),
        &["compile", "checkout", "--fmt", "--include-draft"],
    );
    assert_eq!(code, 0, "{reply}");
    if approved {
        for args in [
            vec!["init", "--quiet", "--object-format=sha1"],
            vec!["config", "user.email", "replay@example.invalid"],
            vec!["config", "user.name", "Replay observer"],
            vec!["add", "."],
            vec!["commit", "--quiet", "--no-verify", "-m", "fixture"],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let (code, reply) = cli(dir.path(), &["approve", "checkout"]);
        assert_eq!(code, 0, "{reply}");
    }
    // A driver I/O tripwire: invalid input must not execute even an installed program.
    let observer = dir.path().join(".graphhelm/observers");
    std::fs::create_dir_all(&observer).unwrap();
    std::fs::write(
        observer.join("journey_driver.mjs"),
        "import {writeFileSync} from 'node:fs';writeFileSync('DRIVER_STARTED','unsafe');",
    )
    .unwrap();
    dir
}

fn refuses(project: &Path, args: &[&str], exit: i32, code: &str) {
    let (actual, reply) = cli(project, args);
    assert_eq!(actual, exit, "{reply}");
    assert_eq!(reply["command"], "journey.replay");
    assert!(
        reply["diagnostics"]
            .as_array()
            .is_some_and(|ds| ds.iter().any(|d| d["code"] == code)),
        "{reply}"
    );
    assert!(!project.join("DRIVER_STARTED").exists());
}

#[test]
fn invalid_id_and_partial_recording_bundle_are_input_errors() {
    let dir = tempfile::tempdir().unwrap();
    refuses(
        dir.path(),
        &["replay", "../outside"],
        3,
        "replay.id_invalid",
    );
    refuses(
        dir.path(),
        &["replay", "checkout", "--execution", "uncreated"],
        3,
        "replay.recording_incomplete",
    );
    // An unauthenticated hidden worker refuses even while the caller keeps stdin open.
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "--json",
            "journey",
            "replay",
            "checkout",
            "--replay-worker",
            "--project",
        ])
        .arg(dir.path())
        .env_remove("GRAPHHELM_REPLAY_WORKER")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let held_input = child.stdin.take().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let timely = loop {
        if child.try_wait().unwrap().is_some() {
            break true;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            break false;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    drop(held_input);
    let out = child.wait_with_output().unwrap();
    assert!(
        timely,
        "unauthenticated worker blocked on stdin before refusing"
    );
    assert_eq!(out.status.code(), Some(3));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "replay.worker_invalid")
    );
    assert!(!dir.path().join(".graphhelm").exists());
}

#[test]
fn draft_noncanonical_stale_and_unreadable_projections_never_start_driver() {
    let draft = project(false);
    refuses(
        draft.path(),
        &["replay", "checkout"],
        2,
        "flow.not_approved",
    );
    let approved = project(true);
    let flow = approved
        .path()
        .join(".graphhelm/journeys/checkout.journey.yaml");
    let bytes = std::fs::read(&flow).unwrap();
    std::fs::write(&flow, [bytes.as_slice(), b"\n"].concat()).unwrap();
    refuses(
        approved.path(),
        &["replay", "checkout"],
        2,
        "flow.not_canonical",
    );
    std::fs::write(
        &flow,
        String::from_utf8(bytes.clone())
            .unwrap()
            .replace("name: Checkout", "name: Changed"),
    )
    .unwrap();
    refuses(
        approved.path(),
        &["replay", "checkout"],
        2,
        "flow.approval_stale",
    );
    std::fs::write(&flow, bytes).unwrap();
    let contract = approved.path().join(".graphhelm/journeys/checkout.json");
    std::fs::remove_file(&contract).unwrap();
    std::fs::create_dir(&contract).unwrap();
    refuses(
        approved.path(),
        &["replay", "checkout"],
        2,
        "flow.contract_stale",
    );
}

#[test]
fn malformed_oversized_and_directory_cache_are_preserved_and_refused() {
    let dir = project(true);
    let cache = dir.path().join(".graphhelm/journey-cache/checkout.json");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    for bytes in [
        b"{broken".to_vec(),
        vec![b'x'; 2 * 1024 * 1024 + 1],
        b"{\"secret\":\"forbidden\"}".to_vec(),
    ] {
        std::fs::write(&cache, &bytes).unwrap();
        refuses(
            dir.path(),
            &["replay", "checkout"],
            2,
            "replay.cache_invalid",
        );
        assert_eq!(std::fs::read(&cache).unwrap(), bytes);
    }
    // Independent wire fixture, bound to approved input rather than subject output.
    let flow: Value = serde_yaml_ng::from_slice(
        &std::fs::read(dir.path().join(".graphhelm/journeys/checkout.journey.yaml")).unwrap(),
    )
    .unwrap();
    let mut valid: Value =
        serde_json::from_str(include_str!("fixtures/journey_replay/checkout-cache.json")).unwrap();
    valid["flowDigest"] = flow["approved"]["digest"].clone();
    valid["screens"].as_object_mut().unwrap().remove("guest");
    valid["edges"].as_object_mut().unwrap().remove("cart.guest");
    valid["edges"]
        .as_object_mut()
        .unwrap()
        .remove("guest.submit");
    valid["edges"]["pay.submit"][1]["name"] = "Pay now".into();
    std::fs::write(&cache, serde_json::to_vec(&valid).unwrap()).unwrap();
    // The valid control must reach observer readiness, not an earlier refusal.
    refuses(
        dir.path(),
        &["replay", "checkout"],
        3,
        "replay.observer_missing",
    );
    // JSON Schema integer values include 1280.0: no panic after schema acceptance.
    let mut numeric = valid.clone();
    numeric["viewport"] = serde_json::json!({"width":1280.0,"height":720.0});
    std::fs::write(&cache, serde_json::to_vec(&numeric).unwrap()).unwrap();
    refuses(
        dir.path(),
        &["replay", "checkout"],
        3,
        "replay.observer_missing",
    );

    for mutation in 0..8 {
        let mut invalid = valid.clone();
        match mutation {
            0 => invalid["id"] = "foreign".into(),
            1 => {
                invalid["edges"]["pay.submit"].as_array_mut().unwrap().pop();
            }
            2 => invalid["edges"]["cart.checkout"][0]["exact"] = false.into(),
            3 => invalid["edges"]["cart.checkout"][0]["nth"] = 0.into(),
            4 => invalid["screens"]["cart"]["unknown"] = true.into(),
            5 => invalid["screens"]["cart"]["controls"][0]["name"] = "replay_canary_831597".into(),
            6 => invalid["screens"]["extra"] = invalid["screens"]["cart"].clone(),
            _ => {
                invalid["flowDigest"] = format!("sha256:{}", "0".repeat(64)).into();
                invalid["viewport"] = serde_json::json!({"width":16384,"height":16384});
            }
        }
        let bytes = serde_json::to_vec(&invalid).unwrap();
        std::fs::write(&cache, &bytes).unwrap();
        refuses(
            dir.path(),
            &["replay", "checkout"],
            2,
            "replay.cache_invalid",
        );
        assert_eq!(std::fs::read(&cache).unwrap(), bytes);
    }
    let escaped_secret = "quote\"canary\n831597";
    let mut escaped_cache = valid.clone();
    escaped_cache["screens"]["cart"]["controls"][0]["name"] = escaped_secret.into();
    let bytes = serde_json::to_vec(&escaped_cache).unwrap();
    std::fs::write(&cache, &bytes).unwrap();
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "journey", "replay", "checkout", "--project"])
        .arg(dir.path())
        .env("GRAPHHELM_SECRET_shopper_password", escaped_secret)
        .output()
        .unwrap();
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "escaped known secret passed cache preflight: {result}"
    );
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["code"] == "replay.cache_invalid")
    );
    assert_eq!(std::fs::read(&cache).unwrap(), bytes);
    std::fs::remove_file(&cache).unwrap();
    std::fs::create_dir(&cache).unwrap();
    refuses(
        dir.path(),
        &["replay", "checkout"],
        2,
        "replay.cache_invalid",
    );
    assert!(cache.is_dir());
}
