//! `graphhelm keel check` (#1330): the scope counter reached through the installed binary.
//!
//! Every case builds a real git repository with two commits and runs the binary on the range, so
//! the git invocation, the embedded policy and the envelope are all under test, not only the core
//! function. The oracle for each case is a literal: the rule id, the exit code, the counts.

use assert_cmd::Command;
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=keel",
            "-c",
            "user.email=keel@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

/// A repository whose `HEAD~1..HEAD` range adds `src/lib.rs` content and whatever `extra` names.
fn repository(extra: &[(&str, &str)]) -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q"]);
    fs::create_dir_all(repo.path().join("src")).unwrap();
    fs::write(repo.path().join("src/lib.rs"), "pub fn old() {}\n").unwrap();
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    fs::write(
        repo.path().join("src/lib.rs"),
        "pub fn old() {}\npub fn added() {}\n",
    )
    .unwrap();
    for (path, body) in extra {
        let full = repo.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, body).unwrap();
    }
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "change"]);
    repo
}

fn card(dir: &Path, scope: &[&str]) -> std::path::PathBuf {
    let path = dir.join("card.json");
    let card = serde_json::json!({
        "promise": "added() exists",
        "scopePaths": scope,
        "proof": "cargo test",
        "exportedSymbols": ["added"],
    });
    fs::write(&path, serde_json::to_vec(&card).unwrap()).unwrap();
    path
}

fn run(repo: &Path, card: Option<&Path>) -> (i32, Value) {
    let mut command = Command::cargo_bin("graphhelm").unwrap();
    command.args([
        "--json",
        "keel",
        "check",
        "--diff",
        "HEAD~1..HEAD",
        "--repo",
        repo.to_str().unwrap(),
    ]);
    if let Some(card) = card {
        command.args(["--card", card.to_str().unwrap()]);
    }
    let output = command.output().unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&output.stdout)));
    (output.status.code().unwrap(), reply)
}

fn codes(reply: &Value) -> Vec<String> {
    reply["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_diff_inside_its_card_passes_and_reports_its_surface() {
    let repo = repository(&[]);
    let scratch = tempfile::tempdir().unwrap();
    let (code, reply) = run(repo.path(), Some(&card(scratch.path(), &["src"])));
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["command"], "keel");
    assert_eq!(reply["data"]["policyVersion"], "1.2.0");
    assert_eq!(reply["data"]["cardDeclared"], true);
    assert_eq!(reply["data"]["surface"]["changedFiles"], 1);
    assert_eq!(reply["data"]["surface"]["newPublicSymbols"], 1);
    assert_eq!(reply["data"]["surface"]["cardExportedSymbols"], 1);
    assert_eq!(
        reply["data"]["surface"]["undeclaredPublicSymbols"],
        serde_json::json!([])
    );
    assert!(codes(&reply).is_empty(), "{reply}");
}

#[test]
fn a_path_outside_the_card_blocks_with_exit_2_and_keeps_the_report() {
    let repo = repository(&[("docs/notes.md", "an unplanned edit\n")]);
    let scratch = tempfile::tempdir().unwrap();
    let (code, reply) = run(repo.path(), Some(&card(scratch.path(), &["src/lib.rs"])));
    assert_eq!(code, 2, "{reply}");
    assert_eq!(reply["ok"], false);
    assert_eq!(codes(&reply), vec!["keel.scope.path_outside_card"]);
    assert_eq!(reply["diagnostics"][0]["severity"], "error");
    assert_eq!(reply["diagnostics"][0]["path"], "docs/notes.md");
    assert_eq!(reply["data"]["refused"], true);
    assert_eq!(reply["data"]["surface"]["changedFiles"], 2);
}

#[test]
fn a_sneaked_in_test_file_is_counted_and_blocked_when_outside_the_card() {
    let repo = repository(&[("tests/extra.rs", "#[test]\nfn sneaked() {}\n")]);
    let scratch = tempfile::tempdir().unwrap();
    let (code, reply) = run(repo.path(), Some(&card(scratch.path(), &["src"])));
    assert_eq!(code, 2, "{reply}");
    assert_eq!(codes(&reply), vec!["keel.scope.path_outside_card"]);
    assert_eq!(reply["data"]["surface"]["newTestFiles"], 1);
    assert_eq!(reply["data"]["surface"]["newTests"], 1);
}

#[test]
fn without_a_card_nothing_about_scope_blocks() {
    let repo = repository(&[("docs/notes.md", "an unplanned edit\n")]);
    let (code, reply) = run(repo.path(), None);
    assert_eq!(code, 0, "{reply}");
    assert_eq!(reply["data"]["cardDeclared"], false);
    assert!(
        !codes(&reply).contains(&"keel.scope.path_outside_card".to_owned()),
        "{reply}"
    );
}

#[test]
fn a_range_that_is_not_a_range_or_a_card_that_is_not_a_card_is_input_error_exit_3() {
    let repo = repository(&[]);
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "keel", "check", "--diff=--output=x", "--repo"])
        .arg(repo.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI030_KEEL_CHECK_INPUT"));

    let scratch = tempfile::tempdir().unwrap();
    let bad = scratch.path().join("card.json");
    fs::write(&bad, br#"{"scopePaths":["src"],"unknown":1}"#).unwrap();
    let (code, reply) = run(repo.path(), Some(&bad));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(codes(&reply), vec!["GHCLI030_KEEL_CHECK_INPUT"]);
}
