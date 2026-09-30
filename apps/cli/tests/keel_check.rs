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
    assert_eq!(reply["data"]["policyVersion"], "1.3.0");
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
    assert!(String::from_utf8_lossy(&output.stdout).contains("GHCLI031_KEEL_CHECK_INPUT"));

    let scratch = tempfile::tempdir().unwrap();
    let bad = scratch.path().join("card.json");
    fs::write(&bad, br#"{"scopePaths":["src"],"unknown":1}"#).unwrap();
    let (code, reply) = run(repo.path(), Some(&bad));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(codes(&reply), vec!["GHS002_SCHEMA"]);
}

#[test]
fn a_schema_invalid_card_is_rejected_with_the_failing_field_path() {
    let repo = repository(&[]);
    let scratch = tempfile::tempdir().unwrap();
    let bad = scratch.path().join("card.json");
    fs::write(
        &bad,
        br#"{"promise":"","scopePaths":["src"],"proof":"cargo test"}"#,
    )
    .unwrap();

    let (code, reply) = run(repo.path(), Some(&bad));
    assert_eq!(code, 3, "{reply}");
    assert_eq!(reply["ok"], false);
    assert_eq!(codes(&reply), vec!["GHS002_SCHEMA"]);
    assert_eq!(reply["diagnostics"][0]["path"], "/promise");
    assert_eq!(reply["diagnostics"][0]["source"], "keel-card");
}

/// #1333: `--prove-new-tests` on a fix whose inline `mod tests` adds one test that detects the bug
/// and one that asserts its own arithmetic. Only the second is a signal, the checkout is untouched,
/// and no proving worktree outlives the command.
#[test]
fn prove_new_tests_earns_the_regression_test_and_flags_the_one_green_on_the_parent() {
    let repo = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| {
        let full = repo.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    };
    write(
        "Cargo.toml",
        "[package]\nname = \"subject\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    write(".gitignore", "/target\nCargo.lock\n");
    write(
        "src/lib.rs",
        "pub fn double(x: u32) -> u32 {\nx + x + 1\n}\n",
    );
    git(repo.path(), &["init", "-q"]);
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    write(
        "src/lib.rs",
        "pub fn double(x: u32) -> u32 {\nx + x\n}\n\n#[cfg(test)]\nmod tests {\nuse super::*;\n\n#[test]\nfn double_of_two_is_four() {\nassert_eq!(double(2), 4);\n}\n\n#[test]\nfn arithmetic_still_works() {\nassert_eq!(2 + 2, 4);\n}\n}\n",
    );
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "fix"]);
    let target = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "--json",
            "keel",
            "check",
            "--diff",
            "HEAD~1..HEAD",
            "--prove-new-tests",
            "--prove-timeout-secs",
            "300",
            "--repo",
        ])
        .arg(repo.path())
        .arg("--prove-target-dir")
        .arg(target.path())
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{reply}");
    let proofs = reply["data"]["testProof"]["proofs"].as_array().unwrap();
    let verdict_of = |name: &str| {
        proofs
            .iter()
            .find(|proof| proof["name"] == name)
            .map(|proof| {
                (
                    proof["parent"]["outcome"].clone(),
                    proof["head"]["outcome"].clone(),
                    proof["verdict"].clone(),
                )
            })
            .unwrap_or_else(|| panic!("{name} not proven: {reply}"))
    };
    assert_eq!(
        verdict_of("double_of_two_is_four"),
        ("failed".into(), "passed".into(), "earned".into()),
        "{reply}"
    );
    assert_eq!(
        verdict_of("arithmetic_still_works"),
        ("passed".into(), "passed".into(), "green_on_parent".into()),
        "{reply}"
    );
    assert_eq!(codes(&reply), vec!["keel.test.green_on_parent"], "{reply}");

    let status = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert!(status.stdout.is_empty(), "the checkout was written");
    let worktrees = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.path())
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let listed = String::from_utf8_lossy(&worktrees.stdout);
    assert_eq!(listed.matches("worktree ").count(), 1, "{listed}");
}

#[test]
fn an_incomplete_inline_graft_does_not_claim_the_subject_is_new() {
    let repo = tempfile::tempdir().unwrap();
    let write = |text: &str| {
        fs::create_dir_all(repo.path().join("src")).unwrap();
        fs::write(repo.path().join("src/lib.rs"), text).unwrap();
    };
    fs::write(
        repo.path().join("Cargo.toml"),
        "[package]\nname = \"keel_graft_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    git(repo.path(), &["init", "-q"]);
    write("pub fn fixed() -> u32 { 1 }\n");
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "base"]);
    write(
        "pub fn fixed() -> u32 { 1 }\n\n#[cfg(test)]\nmod tests {\nuse super::*;\nfn expected_value() -> u32 { 1 }\n#[test]\nfn fixed_is_one() { assert_eq!(fixed(), expected_value()); }\n}\n",
    );
    git(repo.path(), &["add", "."]);
    git(repo.path(), &["commit", "-q", "-m", "test"]);
    let target = tempfile::tempdir().unwrap();
    let output = Command::cargo_bin("graphhelm")
        .unwrap()
        .args(["--json", "keel", "check", "--repo"])
        .arg(repo.path())
        .args([
            "--diff",
            "HEAD~1..HEAD",
            "--prove-new-tests",
            "--prove-timeout-secs",
            "30",
            "--prove-target-dir",
        ])
        .arg(target.path())
        .output()
        .unwrap();
    let reply: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{reply}");
    let proof = &reply["data"]["testProof"]["proofs"][0];
    assert_eq!(proof["parent"]["outcome"], "did_not_compile");
    assert!(
        proof["parent"]["detail"]
            .as_str()
            .unwrap()
            .contains("expected_value")
    );
    assert_eq!(proof["head"]["outcome"], "passed");
    assert_eq!(proof["verdict"], "unproven");
    assert_eq!(
        reply["data"]["testProof"]["findings"][0]["rule"],
        "keel.test.unproven"
    );
}
