//! #519: `journey preview` answers the kept result for the flow and commit it ran on, else starts
//! one run in the background and answers `running`; the run ends `ready` or `failed` with a reason
//! from the closed list, and `--force` runs anew. Credible defects: a run that never leaves
//! `running` when it cannot start a browser, a kept result served for a changed flow, and a second
//! run started on every read. Real CLI/tempdir/Git boundary, the installed observer is a tripwire
//! that must never start; cost: seconds after build, offline, no Node/browser/provider.
use std::path::Path;
use std::time::{Duration, Instant};

#[path = "support/time_scale.rs"]
mod time_scale;
use time_scale::scaled;

use serde_json::Value;

const FLOW: &str = include_str!("fixtures/journey_flow/checkout.journey.yaml");
const BACKEND_FLOW: &str = include_str!("fixtures/journey_backend/backend.journey.yaml");

fn cli(project: &Path, args: &[&str]) -> (i32, Value) {
    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
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
            graphhelm_process::command("git")
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
    // #677: the closed reason must not replace the original refusal. No page text is kept.
    assert_eq!(
        first["detail"],
        serde_json::json!({"code":"replay.observer_missing","pointer":"/observer"})
    );
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

#[test]
fn a_missing_launcher_shell_is_recorded_as_a_spawn_failure() {
    let dir = draft();
    std::fs::write(
        dir.path().join(".graphhelm/journeys/backend.journey.yaml"),
        BACKEND_FLOW,
    )
    .unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/journey-fixture.json"),
        r#"{"schema":"graphhelm-journey-fixture/1","script":"launcher.sh","isolated":true}"#,
    )
    .unwrap();
    std::fs::write(dir.path().join("launcher.sh"), "#!/bin/sh\nexit 0\n").unwrap();
    assert!(
        graphhelm_process::command("git")
            .arg("-C")
            .arg(dir.path())
            .args(["add", "."])
            .status()
            .unwrap()
            .success()
    );
    assert!(
        graphhelm_process::command("git")
            .arg("-C")
            .arg(dir.path())
            .args(["commit", "--quiet", "--no-verify", "-m", "launcher fixture"])
            .status()
            .unwrap()
            .success()
    );

    let out = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args(["--json", "journey", "preview", "backend"])
        .arg("--project")
        .arg(dir.path())
        .env("GRAPHHELM_SECRET_shopper_password", "preview_canary_519")
        .env("GRAPHHELM_POSIX_SHELL", dir.path().join("missing-shell"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reply["data"]["state"], "running", "{reply}");
    let result = loop {
        let (code, reply) = cli(dir.path(), &["preview", "backend", "--read"]);
        assert_eq!(code, 0, "{reply}");
        if reply["data"]["state"] != "running" {
            break reply["data"].clone();
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(result["state"], "failed", "{result}");
    assert_eq!(result["reason"], "watch.launch_failed", "{result}");
    let log = result["launchLog"]["tail"].as_str().unwrap_or_default();
    assert!(log.contains("could not start"), "{result}");
    assert!(!log.contains("did not finish within"), "{result}");
    assert!(!dir.path().join("DRIVER_STARTED").exists());
}

/// A 1x1 PNG: what a kept frame is, at the smallest size.
const TINY_PNG: &str = "89504e470d0a1a0a0000000d4948445200000001000000010802000000907753de0000000c49444154789c63f8cfc0000003010100c9fe92ef0000000049454e44ae426082";

/// #1448: `journey report` writes one self-contained page from the kept preview: an inlined frame
/// for each screen that has one, the action into each step, the checks with seen/not seen, the
/// overall result, and every name escaped. It refuses when no preview is kept, and starts no run
/// (the installed observer is the tripwire).
#[test]
fn a_report_writes_one_page_with_the_frames_actions_checks_and_result() {
    let dir = draft();
    let out = dir.path().join("report.html");
    let report = |dir: &Path| {
        cli(
            dir,
            &["report", "checkout", "--html", out.to_str().unwrap()],
        )
    };

    let (code, reply) = report(dir.path());
    assert_eq!(code, 3, "{reply}");
    assert_eq!(reply["diagnostics"][0]["code"], "report.preview_missing");
    assert!(!out.exists());

    let (_, none) = cli(dir.path(), &["preview", "checkout", "--read"]);
    let (digest, commit) = (&none["data"]["digest"], &none["data"]["commit"]);
    let check =
        |name: &str, seen: bool| serde_json::json!({"role":"button","name":name,"seen":seen});
    let state = serde_json::json!({
        "preview": true, "kind": "preview", "state": "ready", "result": "drift",
        "digest": digest, "commit": commit,
        "startedAt": "2026-10-17T10:00:00+00:00", "ranAt": "2026-10-17T10:00:09+00:00",
        "screens": {
            "cart": {"result":"pass","frame":true,"file":"cart.png","width":1,"height":1,
                "checks":[check("Checkout", true)]},
            "pay": {"result":"drift","reason":"expect_missing","frame":true,"file":"pay.png",
                "width":1,"height":1,"checks":[check("Pay <now>", false)]},
            "done": {"reason":"not_reached","frame":false}
        },
        "edges": {"cart.checkout": {"result":"pass"},
            "pay.submit": {"result": null, "reason": "not_reached"}}
    });
    let kept = dir.path().join(".graphhelm/journey-previews/checkout");
    std::fs::create_dir_all(kept.join("frames")).unwrap();
    std::fs::write(kept.join("state.json"), state.to_string()).unwrap();
    for screen in ["cart", "pay"] {
        std::fs::write(
            kept.join(format!("frames/{screen}.png")),
            hex::decode(TINY_PNG).unwrap(),
        )
        .unwrap();
    }

    let (code, reply) = report(dir.path());
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["frames"], 2, "{reply}");
    let page = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        page.matches("src=\"data:image/png;base64,").count(),
        2,
        "{page}"
    );
    assert!(page.contains("Shopper pays for the cart"), "{page}");
    assert!(
        page.contains("Action: activate &quot;Checkout&quot;"),
        "{page}"
    );
    assert!(page.contains("&#10003; button \"Checkout\""), "{page}");
    assert!(
        page.contains("&#10007; button \"Pay &lt;now&gt;\""),
        "{page}"
    );
    assert!(
        page.contains("No screenshot recorded for this step"),
        "{page}"
    );
    assert!(
        page.contains("Overall: <b style=\"color:#BA7517\">drift</b>"),
        "{page}"
    );
    assert!(
        !page.contains("Pay <now>"),
        "an unescaped name reached the page"
    );
    assert!(!dir.path().join("DRIVER_STARTED").exists());
}
