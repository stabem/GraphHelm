//! #2625: a scoped, revocable journey.approve grant, used through `journey approve-grant`.
//! `journey approve` stays owner-only (#1309 ruling A). Same project setup as the owner tests
//! (scope files, a flow under .graphhelm/journeys, a passing preview); approval still goes through
//! approve_owned, so the grant never bypasses the approval checks.

use std::path::Path;

#[path = "support/mod.rs"]
mod support;
use support::{owner_token, passing_preview};

fn graphhelm() -> std::process::Command {
    let mut command = graphhelm_process::command(assert_cmd::cargo::cargo_bin!("graphhelm"));
    command.env(
        "GRAPHHELM_EVENTS_KEY",
        "0101010101010101010101010101010101010101010101010101010101010101",
    );
    command
}

fn project_with_flow() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in ["app/cart/page.tsx", "app/checkout/page.tsx", "app/api/pay/route.ts"] {
        let path = dir.path().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "export {}").unwrap();
    }
    std::fs::create_dir_all(dir.path().join(".graphhelm/journeys")).unwrap();
    std::fs::write(
        dir.path().join(".graphhelm/journeys/checkout.journey.yaml"),
        include_str!("fixtures/journey_flow/checkout.journey.yaml"),
    )
    .unwrap();
    dir
}

fn commit_project(project: &Path) {
    for args in [
        vec!["init", "-q"],
        vec!["add", "-A"],
        vec!["-c", "user.email=owner@example.com", "-c", "user.name=owner", "commit", "-qm", "project"],
    ] {
        let status = std::process::Command::new("git")
            .args(&args)
            .current_dir(project)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }
}
fn write_grant(project: &Path) -> std::path::PathBuf {
    let grant = project.join("grant.json");
    std::fs::write(
        &grant,
        r#"{"grants":[{"id":"g-qa-1","actor":"keel-journey-qa","scope":["journey.approve"],"revoked":false}]}"#,
    )
    .unwrap();
    grant
}

fn approve_grant(project: &Path, grant: &Path, actor: &str) -> std::process::Output {
    graphhelm()
        .args(["--json", "journey", "approve-grant", "checkout", "--grant"])
        .arg(grant)
        .args(["--actor", actor, "--project"])
        .arg(project)
        .output()
        .unwrap()
}

/// Red (Keel case 2): a valid grant lets keel-journey-qa approve a journey contract with a passing
/// preview, and the approval record names the grant id. Fails on main: no approve-grant route.
#[test]
fn a_valid_grant_is_accepted_and_the_record_names_it() {
    let project = project_with_flow();
    let _ = owner_token(project.path());
    let grant = write_grant(project.path());
    commit_project(project.path());
    passing_preview(project.path(), "checkout");

    let output = approve_grant(project.path(), &grant, "keel-journey-qa");
    let record = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "a valid grant was refused: {record}");
    let audit = std::fs::read_to_string(project.path().join(".graphhelm/journey-grant-audit/checkout.json")).expect("grant-audit record written");
    assert!(audit.contains("g-qa-1"), "the audit record must name the grant id: {audit}");
    assert!(audit.contains("keel-journey-qa") && audit.contains("checkout") && audit.contains("sha256:"), "{audit}");
}

/// Red (Keel case 3, Shikamaru): a valid grant with a FAILING preview is still refused. The grant
/// does not bypass the preview gate that approve_owned enforces; the flow is untouched.
#[test]
fn a_valid_grant_with_a_failing_preview_is_still_refused() {
    let project = project_with_flow();
    let _ = owner_token(project.path());
    let grant = write_grant(project.path());
    commit_project(project.path());
    let before = std::fs::read(project.path().join(".graphhelm/journeys/checkout.journey.yaml")).unwrap();

    let output = approve_grant(project.path(), &grant, "keel-journey-qa");
    assert!(!output.status.success(), "approved with no passing preview: {}", String::from_utf8_lossy(&output.stdout));
    let after = std::fs::read(project.path().join(".graphhelm/journeys/checkout.journey.yaml")).unwrap();
    assert_eq!(before, after, "a refused approve must not change the flow");
}

/// The digest rule is unchanged (#2625, Shikamaru): an approval through a grant writes the same
/// `approved.digest` the owner approval would for the same flow, so existing approvals are untouched
/// and the grant adds only the separate audit record.
#[test]
fn a_grant_approval_writes_the_same_digest_as_the_owner_approval() {
    let granted = project_with_flow();
    let _ = owner_token(granted.path());
    let grant = write_grant(granted.path());
    commit_project(granted.path());
    passing_preview(granted.path(), "checkout");
    let output = approve_grant(granted.path(), &grant, "keel-journey-qa");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stdout));
    let granted_flow = std::fs::read_to_string(granted.path().join(".graphhelm/journeys/checkout.journey.yaml")).unwrap();
    let stdout_digest = serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["data"]["approved"]["digest"].as_str().unwrap_or_default().to_owned();

    let owned = project_with_flow();
    let token = owner_token(owned.path());
    commit_project(owned.path());
    passing_preview(owned.path(), "checkout");
    let owner_out = graphhelm()
        .args(["--json", "journey", "approve", "checkout", "--token-file", &token, "--project"])
        .arg(owned.path())
        .output()
        .unwrap();
    assert!(owner_out.status.success(), "{}", String::from_utf8_lossy(&owner_out.stdout));
    let owner_flow = std::fs::read_to_string(owned.path().join(".graphhelm/journeys/checkout.journey.yaml")).unwrap();

    let digest = |text: &str| {
        text.lines()
            .find(|line| line.trim_start().starts_with("digest:") && line.contains("sha256:"))
            .map(|line| line.trim().to_owned())
            .unwrap_or_default()
    };
    assert!(!stdout_digest.is_empty(), "the grant approval reported no digest");
    assert_eq!(digest(&granted_flow), digest(&owner_flow), "a grant must not change the digest rule");
}