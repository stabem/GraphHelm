use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn repo() -> TempDir {
    let d = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-q", d.path().to_str().unwrap()])
        .assert()
        .success();
    fs::write(
        d.path().join("lib.rs"),
        "pub fn hello() {}\nfn private() {}\n",
    )
    .unwrap();
    fs::write(d.path().join("README.md"), "hello\n").unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "add", "."])
        .assert()
        .success();
    d
}

#[test]
fn scan_is_deterministic_and_reports_unsupported_without_source() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.contains("\"path\": \"README.md\""));
    assert!(text.contains("\"parse\": \"unsupported\""));
    assert!(text.contains("\"name\": \"hello\""));
    assert!(!text.contains("pub fn hello"));
    let first = text.clone();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert_eq!(first, fs::read_to_string(out).unwrap());
}

#[test]
fn verify_rejects_source_mutation() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::write(d.path().join("lib.rs"), "pub fn changed() {}\n").unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("KIDX2"));
}

#[test]
fn query_selects_by_symbol() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "query",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("lib.rs"));
}

#[test]
fn new_untracked_source_invalidates_the_coverage_snapshot() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::write(d.path().join("new.rs"), "pub fn added() {}\n").unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "query",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "hello",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("KIDX2"));
}

#[test]
fn scan_refuses_output_inside_the_repository_even_in_a_new_directory() {
    let d = repo();
    let out = d.path().join("new-directory").join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("KIDX5"));
    assert!(!out.exists());
}

#[test]
fn ignored_files_are_reported_as_omissions() {
    let d = repo();
    fs::write(d.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(d.path().join("ignored.txt"), "secret\n").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let text = fs::read_to_string(out).unwrap();
    assert!(text.contains("ignored.txt"));
    assert!(text.contains("ignored_excluded"));
}

#[test]
fn invalid_utf8_is_explicitly_unparseable() {
    let d = repo();
    fs::write(d.path().join("broken.rs"), [0xff, 0xfe]).unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "add", "broken.rs"])
        .assert()
        .success();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(fs::read_to_string(out).unwrap().contains("invalid_utf8"));
}

#[test]
fn proposal_is_source_bound_and_has_no_acceptance_promise() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "propose-card",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "lib.rs",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("needs_product_contract"))
        .stdout(predicate::str::contains("acceptanceCriteria").and(predicate::str::contains("[]")));
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "propose-card",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "missing",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("KIDX6"));
}

#[test]
fn verify_rejects_oversized_index_before_json_parsing() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    fs::write(&out, vec![b' '; 16 * 1024 * 1024 + 1]).unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "verify",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("KIDX4"));
}

#[test]
fn a_new_ignored_file_invalidates_the_coverage_snapshot() {
    let d = repo();
    fs::write(d.path().join(".gitignore"), "*.secret\n").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    fs::write(d.path().join("new.secret"), "secret\n").unwrap();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "query",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "lib.rs",
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("KIDX2"));
}

#[test]
fn candidate_discloses_unobserved_ignored_directory_contents() {
    let d = repo();
    fs::write(d.path().join(".gitignore"), "ignored/\n").unwrap();
    fs::create_dir(d.path().join("ignored")).unwrap();
    fs::write(
        d.path().join("ignored").join("hidden.rs"),
        "pub fn hidden() {}\n",
    )
    .unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "propose-card",
            "--repo",
            d.path().to_str().unwrap(),
            "--index",
            out.to_str().unwrap(),
            "--term",
            "lib.rs",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"ignoredDirectoryContentsUnobserved\":true",
        ));
}

#[test]
fn query_and_candidate_disclose_untracked_coverage_gaps() {
    let d = repo();
    fs::write(d.path().join("extra.rs"), "pub fn unseen() {}\n").unwrap();
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    for command in ["query", "propose-card"] {
        let output = Command::cargo_bin("keel-contract-index")
            .unwrap()
            .args([
                command,
                "--repo",
                d.path().to_str().unwrap(),
                "--index",
                out.to_str().unwrap(),
                "--term",
                "lib.rs",
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let result: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(result["coverageGaps"]["omittedPaths"], 1);
        assert_eq!(
            result["coverageGaps"]["omissionReasons"]["untracked_excluded"],
            1
        );
        assert_eq!(result["coverageGaps"]["unsupportedFiles"], 1);
    }
}

#[test]
fn scan_disables_repository_fsmonitor_commands() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let marker = out_dir.path().join("fsmonitor-ran");
    let hook = out_dir.path().join("fsmonitor.sh");
    let hook_path = hook.to_string_lossy().replace('\\', "/");
    let marker_path = marker.to_string_lossy().replace('\\', "/");
    fs::write(
        &hook,
        format!("#!/bin/sh\nprintf ran > \"{marker_path}\"\n"),
    )
    .unwrap();
    Command::new("git")
        .args([
            "-C",
            d.path().to_str().unwrap(),
            "config",
            "core.fsmonitor",
            &format!("sh \"{hook_path}\""),
        ])
        .assert()
        .success();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "status", "--porcelain"])
        .assert()
        .success();
    assert!(marker.exists(), "hostile fsmonitor fixture did not execute");
    fs::remove_file(&marker).unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(!marker.exists(), "scanner executed repository fsmonitor");
}

#[test]
fn scan_does_not_run_repository_clean_filters() {
    let d = repo();
    let out_dir = tempfile::tempdir().unwrap();
    let marker = out_dir.path().join("clean-filter-ran");
    let hook = out_dir.path().join("clean-filter.sh");
    let hook_path = hook.to_string_lossy().replace('\\', "/");
    let marker_path = marker.to_string_lossy().replace('\\', "/");
    fs::write(
        &hook,
        format!("#!/bin/sh\nprintf ran > \"{marker_path}\"\ncat\n"),
    )
    .unwrap();
    fs::write(
        d.path().join(".gitattributes"),
        "filtered.txt filter=hostile\n",
    )
    .unwrap();
    fs::write(d.path().join("filtered.txt"), "before\n").unwrap();
    Command::new("git")
        .args([
            "-C",
            d.path().to_str().unwrap(),
            "config",
            "filter.hostile.clean",
            &format!("sh \"{hook_path}\""),
        ])
        .assert()
        .success();
    Command::new("git")
        .args([
            "-C",
            d.path().to_str().unwrap(),
            "add",
            ".gitattributes",
            "filtered.txt",
        ])
        .assert()
        .success();
    assert!(
        marker.exists(),
        "hostile clean-filter fixture did not execute"
    );
    fs::remove_file(&marker).unwrap();
    let filtered = d.path().join("filtered.txt");
    fs::write(&filtered, "after!\n").unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&filtered)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_600_000_000))
        .unwrap();
    Command::new("git")
        .args(["-C", d.path().to_str().unwrap(), "status", "--porcelain"])
        .assert()
        .success();
    assert!(
        marker.exists(),
        "status did not reproduce the filter hazard"
    );
    fs::remove_file(&marker).unwrap();
    let out = out_dir.path().join("index.json");
    Command::cargo_bin("keel-contract-index")
        .unwrap()
        .args([
            "scan",
            "--repo",
            d.path().to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(!marker.exists(), "scanner executed repository clean filter");
}
