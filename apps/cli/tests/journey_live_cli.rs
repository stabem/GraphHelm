//! #398: `journey open` / `journey act` / `journey close` refuse before any browser I/O when the
//! walk could not be deterministic or the session does not exist. Credible defects: opening a
//! flow with no replay cache (or a void one) and walking it some other way, a step no path
//! reaches, and `act` reaching a session that is not there. Real CLI/tempdir/Git boundary, the
//! installed observer is a tripwire that must never start; cost: seconds after build, offline,
//! no Node/browser/provider.
use std::path::Path;
use std::process::Command;

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");
const CACHE: &str = include_str!("fixtures/journey_replay/checkout-cache.json");

fn cli(project: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "journey"])
        .args(args)
        .arg("--project")
        .arg(project)
        .env("GRAPHHELM_SECRET_shopper_password", "live_canary_398")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

/// An approved `checkout` flow in a committed project, with a tripwire observer installed.
fn approved() -> tempfile::TempDir {
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
    for args in [
        vec!["init", "--quiet", "--object-format=sha1"],
        vec!["config", "user.email", "live@example.invalid"],
        vec!["config", "user.name", "Live observer"],
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
    let observer = dir.path().join(".graphhelm/observers");
    std::fs::create_dir_all(&observer).unwrap();
    std::fs::write(
        observer.join("journey_driver.mjs"),
        "import {writeFileSync} from 'node:fs';writeFileSync('DRIVER_STARTED','unsafe');",
    )
    .unwrap();
    dir
}

fn refuses(project: &Path, args: &[&str], command: &str, exit: i32, code: &str) -> Value {
    let (actual, reply) = cli(project, args);
    assert_eq!(actual, exit, "{reply}");
    assert_eq!(reply["command"], command, "{reply}");
    assert!(
        reply["diagnostics"]
            .as_array()
            .is_some_and(|ds| ds.iter().any(|d| d["code"] == code)),
        "expected {code}: {reply}"
    );
    assert!(!project.join("DRIVER_STARTED").exists());
    reply
}

#[test]
fn open_without_a_current_replay_cache_is_refused_naming_replay() {
    let dir = approved();
    let reply = refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay"],
        "journey.open",
        2,
        "replay.cache_missing",
    );
    assert!(
        reply.to_string().contains("journey replay"),
        "the refusal names the remedy: {reply}"
    );

    // A cache whose flow digest is not the approved flow's is void: still refused.
    let mut cache: Value = serde_json::from_str(CACHE).unwrap();
    cache["flowDigest"] = format!("sha256:{}", "0".repeat(64)).into();
    let directory = dir.path().join(".graphhelm/journey-cache");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("checkout.json"), cache.to_string()).unwrap();
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay"],
        "journey.open",
        2,
        "replay.cache_missing",
    );
}

#[test]
fn a_step_no_path_reaches_and_a_partial_recording_bundle_are_refused() {
    let dir = approved();
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "nowhere"],
        "journey.open",
        2,
        "live.step_unreachable",
    );
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay", "--path", "express"],
        "journey.open",
        2,
        "live.step_unreachable",
    );
    refuses(
        dir.path(),
        &["open", "checkout", "--step", "pay", "--execution", "x"],
        "journey.open",
        3,
        "live.recording_incomplete",
    );
}

#[test]
fn act_and_close_without_a_live_session_are_refused() {
    let dir = approved();
    let (code, listed) = cli(dir.path(), &["sessions"]);
    assert_eq!(code, 0, "{listed}");
    assert_eq!(listed["command"], "journey.sessions");
    assert_eq!(
        listed["data"]["sessions"],
        serde_json::json!([]),
        "{listed}"
    );
    refuses(
        dir.path(),
        &[
            "act",
            "live-0123456789abcdef",
            "--kind",
            "activate",
            "--role",
            "button",
            "--name",
            "Checkout",
        ],
        "journey.act",
        2,
        "live.session_gone",
    );
    refuses(
        dir.path(),
        &["close", "../outside"],
        "journey.close",
        3,
        "live.request_invalid",
    );
}
