//! #519: `journey preview` answers the kept result for the flow and commit it ran on, else starts
//! one run in the background and answers `running`; the run ends `ready` or `failed` with a reason
//! from the closed list, and `--force` runs anew. Credible defects: a run that never leaves
//! `running` when it cannot start a browser, a kept result served for a changed flow, and a second
//! run started on every read. Real CLI/tempdir/Git boundary, the installed observer is a tripwire
//! that must never start; cost: seconds after build, offline, no Node/browser/provider.
#[path = "support/time_scale.rs"]
mod time_scale;
use time_scale::scaled;

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");

fn cli(project: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "journey"])
        .args(args)
        .arg("--project")
        .arg(project)
        .env("GRAPHHELM_SECRET_shopper_password", "preview_canary_519")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap_or(Value::Null),
    )
}

/// A DRAFT `checkout` flow in a committed project, with a tripwire observer installed.
fn draft() -> tempfile::TempDir {
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
        vec!["config", "user.email", "preview@example.invalid"],
        vec!["config", "user.name", "Preview observer"],
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
    let observer = dir.path().join(".graphhelm/observers");
    std::fs::create_dir_all(&observer).unwrap();
    std::fs::write(
        observer.join("journey_driver.mjs"),
        "import {writeFileSync} from 'node:fs';writeFileSync('DRIVER_STARTED','unsafe');",
    )
    .unwrap();
    dir
}

/// Reads until the background run leaves `running`, within a bound.
fn settled(project: &Path) -> Value {
    let deadline = Instant::now() + scaled(Duration::from_secs(60));
    loop {
        let (code, reply) = cli(project, &["preview", "checkout", "--read"]);
        assert_eq!(code, 0, "{reply}");
        if reply["data"]["state"] != "running" {
            return reply["data"].clone();
        }
        assert!(
            Instant::now() < deadline,
            "the run never left running: {reply}"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn a_preview_runs_once_keeps_its_result_and_runs_anew_only_when_asked() {
    let dir = draft();
    let (code, reply) = cli(dir.path(), &["preview", "nowhere"]);
    assert_eq!(code, 2, "{reply}");
    assert!(
        reply["diagnostics"][0]["code"] == "preview.flow_unknown",
        "{reply}"
    );

    // Nothing kept yet: a read answers `none` and starts nothing.
    let (code, reply) = cli(dir.path(), &["preview", "checkout", "--read"]);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["state"], "none", "{reply}");
    assert_eq!(reply["data"]["preview"], true, "{reply}");
    assert_eq!(reply["data"]["kind"], "preview", "{reply}");
    assert!(!dir.path().join(".graphhelm/journey-previews").exists());

    // A start answers `running` at once, and the run ends with a reason, not `running` forever.
    let (code, reply) = cli(dir.path(), &["preview", "checkout"]);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["state"], "running", "{reply}");
    assert!(reply["data"].get("pid").is_none(), "{reply}");
    let first = settled(dir.path());
    assert_eq!(first["state"], "failed", "{first}");
    assert_eq!(first["reason"], "driver.observer_missing", "{first}");
    assert!(first["ranAt"].is_string(), "{first}");

    // Kept: a second start answers the same run instead of running again.
    let (_, again) = cli(dir.path(), &["preview", "checkout"]);
    assert_eq!(again["data"]["state"], "failed", "{again}");
    assert_eq!(again["data"]["ranAt"], first["ranAt"], "{again}");

    // Run again runs anew.
    let (_, forced) = cli(dir.path(), &["preview", "checkout", "--force"]);
    assert_eq!(forced["data"]["state"], "running", "{forced}");
    let second = settled(dir.path());
    assert_ne!(second["ranAt"], first["ranAt"], "{second}");

    // A changed flow makes the kept result stale.
    let flow = dir.path().join(".graphhelm/journeys/checkout.journey.yaml");
    let text = std::fs::read_to_string(&flow).unwrap();
    let changed = text.replacen("title: ", "title: Changed ", 1);
    assert_ne!(changed, text, "ARRANGEMENT: the flow title was edited");
    std::fs::write(&flow, changed).unwrap();
    let (_, stale) = cli(dir.path(), &["preview", "checkout", "--read"]);
    assert_eq!(stale["data"]["state"], "none", "{stale}");

    assert!(
        !dir.path().join("DRIVER_STARTED").exists(),
        "the tripwire observer never starts"
    );
}
