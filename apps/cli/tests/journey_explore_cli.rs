//! Contract: malformed exploration input cannot reach model, browser or publication.
//! Regression: shared worker startup bypasses argument policy and performs effects first.
//! Gap: replay has no model/goal/permission input; architect has no browser proposal boundary.
//! Seams: real CLI and filesystem readback only. Cost: seconds after build, no network/browser.
use assert_cmd::Command;
use serde_json::Value;

#[test]
fn invalid_exploration_inputs_refuse_before_artifacts_or_model_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("model.json");
    std::fs::write(&fixture, r#"{"replies":{}}"#).unwrap();
    for (extra, code, pointer) in [
        (vec!["--id", "../outside"], "explore.id_invalid", "/id"),
        (
            vec!["--base", "https://example.com"],
            "driver.host_refused",
            "/base",
        ),
        (
            vec!["--max-steps", "0"],
            "explore.budget_invalid",
            "/maxSteps",
        ),
        (
            vec!["--allow-act", "["],
            "explore.permission_invalid",
            "/allowAct",
        ),
        (
            vec!["--secret", "missing_credential_356"],
            "driver.secret_missing",
            "/secrets",
        ),
        (
            vec!["--execution", "only-one"],
            "explore.recording_incomplete",
            "/recording",
        ),
    ] {
        let mut args = vec![
            "--json",
            "journey",
            "explore",
            "--base",
            "http://127.0.0.1:1",
            "--goal",
            "Visit checkout",
            "--id",
            "checkout",
            "--fixture",
        ];
        args.push(fixture.to_str().unwrap());
        args.extend(["--project", directory.path().to_str().unwrap()]);
        // Replace existing singleton values rather than relying on clap duplicate handling.
        let extra = if matches!(extra[0], "--id" | "--base") {
            let index = args.iter().position(|v| *v == extra[0]).unwrap();
            args[index + 1] = extra[1];
            Vec::new()
        } else {
            extra
        };
        args.extend(extra);
        let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
            .args(args)
            .env_remove("GRAPHHELM_SECRET_missing_credential_356")
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(3),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(reply["diagnostics"][0]["code"], code, "{reply}");
        assert_eq!(reply["diagnostics"][0]["path"], pointer, "{reply}");
        assert_eq!(reply["data"]["modelCalls"], 0);
        assert!(!directory.path().join(".graphhelm").exists());
    }
}

#[test]
fn fixture_and_live_route_conflict_and_hidden_worker_cannot_be_entered() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "--json",
            "journey",
            "explore",
            "--base",
            "http://127.0.0.1:1",
            "--goal",
            "Visit checkout",
            "--id",
            "checkout",
            "--fixture",
            "fixture.json",
            "--manifest",
            "manifest.yaml",
            "--route",
            "live",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let output = Command::new(assert_cmd::cargo::cargo_bin!("graphhelm"))
        .args([
            "--json",
            "journey",
            "explore",
            "--base",
            "http://127.0.0.1:1",
            "--goal",
            "Visit checkout",
            "--id",
            "checkout",
            "--fixture",
            "fixture.json",
            "--explore-worker",
        ])
        .env_remove("GRAPHHELM_EXPLORE_WORKER")
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(reply["diagnostics"][0]["code"], "explore.worker_invalid");
    assert_eq!(output.status.code(), Some(3));
}
