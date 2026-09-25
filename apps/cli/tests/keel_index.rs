//! Contract evidence for the public Keel index surface.
//!
//! The observable promise is that setup users can invoke one GraphHelm binary to create,
//! verify, and query the existing source-bound index. This catches a wiring defect where the
//! standalone scanner works but the installed CLI is missing the command or emits a non-envelope
//! response. The stale mutation also covers the safety boundary: query must not use an index
//! after the source snapshot changes.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

fn repository() -> TempDir {
    let repo = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-q", repo.path().to_str().unwrap()])
        .assert()
        .success();
    fs::write(repo.path().join("lib.rs"), "pub fn hello() {}\n").unwrap();
    Command::new("git")
        .args(["-C", repo.path().to_str().unwrap(), "add", "."])
        .assert()
        .success();
    repo
}

#[test]
fn public_surface_indexes_verifies_queries_and_refuses_stale_source() {
    let repo = repository();
    let output = tempfile::tempdir().unwrap();
    let index = output.path().join("index.json");

    Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "keel",
            "index",
            "--repo",
            repo.path().to_str().unwrap(),
            "--out",
            index.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"))
        .stdout(predicate::str::contains("\"command\":\"keel\""));

    Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "keel",
            "verify",
            "--repo",
            repo.path().to_str().unwrap(),
            "--index",
            index.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\":true"));

    Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "keel",
            "query",
            "--repo",
            repo.path().to_str().unwrap(),
            "--index",
            index.to_str().unwrap(),
            "--term",
            "hello",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("lib.rs"));

    fs::write(repo.path().join("lib.rs"), "pub fn changed() {}\n").unwrap();
    Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "keel",
            "query",
            "--repo",
            repo.path().to_str().unwrap(),
            "--index",
            index.to_str().unwrap(),
            "--term",
            "hello",
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("GHKEEL002_STALE"));
}

#[test]
fn malformed_index_data_never_appears_in_public_errors() {
    let repo = repository();
    let output = tempfile::tempdir().unwrap();
    let index = output.path().join("index.json");
    Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "keel",
            "index",
            "--repo",
            repo.path().to_str().unwrap(),
            "--out",
            index.to_str().unwrap(),
        ])
        .assert()
        .success();

    let mut document: serde_json::Value =
        serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    let sensitive = "C:\\Users\\private\\secret-token";
    document["files"][0]["parse"] = sensitive.into();
    fs::write(&index, serde_json::to_vec(&document).unwrap()).unwrap();

    Command::cargo_bin("graphhelm")
        .unwrap()
        .args([
            "keel",
            "verify",
            "--repo",
            repo.path().to_str().unwrap(),
            "--index",
            index.to_str().unwrap(),
        ])
        .assert()
        .code(2)
        .stdout(predicate::str::contains("GHKEEL001_INDEX"))
        .stdout(predicate::str::contains("secret-token").not())
        .stderr(predicate::str::contains("secret-token").not());
}
