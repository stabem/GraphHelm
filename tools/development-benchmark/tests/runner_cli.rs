//! The runner binary: its most important output is the word "no" (#225).
//!
//! The issue's validation command is `cargo run -p graphhelm-development-benchmark -- --manifest
//! <path>`, so the binary's contract is testable from here. Until #222 fills the accounting
//! receipts, the ONLY honest outputs are refusals -- and each cell below names which one.
//!
//! Exit codes: 2 for a typed refusal, 64 for a usage error (the sysexits convention for bad
//! invocation), 0 only when a comparison was actually produced -- which nothing can produce yet.

use std::path::PathBuf;
use std::process::Command;

fn runner() -> Command {
    Command::new(env!("CARGO_BIN_EXE_graphhelm-development-benchmark"))
}

fn shipped_manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/manifest.json")
}

/// No arguments is a usage error, not a refusal: nothing was asked, so nothing was refused.
#[test]
fn no_arguments_is_a_usage_error_that_names_the_flag() {
    let output = runner().output().expect("the runner binary exists");

    assert_eq!(output.status.code(), Some(64), "usage errors exit 64");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--manifest"),
        "the usage error must name the missing flag, got: {stderr}"
    );
}

/// The shipped manifest loads -- and the run still refuses, because the quantities it needs have
/// no receipt yet. The refusal names the FIELD and the ARM, per the blueprint: a missing number
/// is never a zero and never an interpolation.
#[test]
fn the_shipped_manifest_loads_and_the_run_refuses_on_the_missing_receipts() {
    let output = runner()
        .arg("--manifest")
        .arg(shipped_manifest())
        .output()
        .expect("the runner binary exists");

    assert_eq!(
        output.status.code(),
        Some(2),
        "a typed refusal exits 2; stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("CostUnavailable"),
        "the refusal is typed, got: {stdout}"
    );
    assert!(
        stdout.contains("compiled_input_tokens"),
        "the refusal names the quantity, got: {stdout}"
    );
    assert!(
        stdout.contains("baseline"),
        "the refusal names the arm, got: {stdout}"
    );
}

/// A corpus edited after the freeze cannot be read at all. The digest mismatch is a refusal, and
/// it carries the declared and actual digests so the operator sees WHICH kind of edit happened.
#[test]
fn a_tampered_corpus_refuses_on_the_digest_before_anything_else() {
    let text = std::fs::read_to_string(shipped_manifest()).expect("shipped manifest is readable");
    let tampered = text.replace("events-open-verdict", "events-open-verdict-edited");
    assert_ne!(text, tampered, "arrangement: the tamper must actually land");
    let dir = std::env::temp_dir().join("gh-225-tampered-manifest");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("manifest.json");
    std::fs::write(&path, tampered).expect("tampered manifest written");

    let output = runner()
        .arg("--manifest")
        .arg(&path)
        .output()
        .expect("the runner binary exists");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("CorpusDigestMismatch"), "got: {stdout}");
}

/// A path that does not exist is unreadable, not a digest problem.
#[test]
fn a_missing_manifest_refuses_as_unreadable() {
    let output = runner()
        .arg("--manifest")
        .arg("does/not/exist/manifest.json")
        .output()
        .expect("the runner binary exists");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Unreadable"), "got: {stdout}");
}

/// K's #504 finding 1, at the binary's grain: an oracle softened after the freeze must refuse
/// even though the manifest itself is byte-identical. The corpus travels as one tree, so the
/// fixture copies the shipped tree and bends one oracle byte.
#[test]
fn a_softened_oracle_refuses_even_with_the_manifest_untouched() {
    let root = std::env::temp_dir().join("gh-225-softened-oracle");
    let _ = std::fs::remove_dir_all(&root);
    for kind in ["oracle", "objectives"] {
        std::fs::create_dir_all(root.join(kind)).expect("fixture tree");
    }
    let shipped = shipped_manifest();
    let shipped_root = shipped.parent().expect("manifest has a directory");
    std::fs::copy(&shipped, root.join("manifest.json")).expect("manifest copied");
    for kind in ["oracle", "objectives"] {
        for entry in std::fs::read_dir(shipped_root.join(kind)).expect("shipped dir") {
            let entry = entry.expect("entry");
            std::fs::copy(entry.path(), root.join(kind).join(entry.file_name()))
                .expect("file copied");
        }
    }
    let target = root.join("oracle/schema-ceiling.json");
    let mut bytes = std::fs::read(&target).expect("oracle readable");
    bytes.push(b' ');
    std::fs::write(&target, bytes).expect("oracle softened");

    let output = runner()
        .arg("--manifest")
        .arg(root.join("manifest.json"))
        .output()
        .expect("the runner binary exists");

    assert_eq!(
        output.status.code(),
        Some(2),
        "stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("FrozenFilesMismatch") && stdout.contains("oracle"),
        "the refusal names the freeze that moved, got: {stdout}"
    );
}

/// The receipts argument the module doc promised. A COMPLETE dry-run directory produces the one
/// measured bar plus NAMED refusals for the bars receipts cannot carry -- and still exits 2,
/// because a one-bar report is not a verdict. Partial truth prints; nothing is promoted.
#[test]
fn a_complete_receipts_directory_reports_the_input_ratio_and_refuses_the_rest_by_name() {
    let root = std::env::temp_dir().join("gh-225-dryrun-receipts");
    let _ = std::fs::remove_dir_all(&root);
    write_dry_run(&root);

    let output = runner()
        .arg("--manifest")
        .arg(shipped_manifest())
        .arg("--receipts")
        .arg(&root)
        .output()
        .expect("the runner binary exists");

    assert_eq!(
        output.status.code(),
        Some(2),
        "one bar out of three is not a verdict; stdout: {} stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("input_ratio") && stdout.contains("0.4"),
        "the measured bar prints, got: {stdout}"
    );
    assert!(
        stdout.contains("worst_case_ratio"),
        "the tail travels WITH the median, got: {stdout}"
    );
    assert!(
        stdout.contains("session") && stdout.contains("quality"),
        "the unmeasured bars are refused BY NAME, got: {stdout}"
    );
}

/// A receipts path that does not exist refuses as unreadable -- before any number.
#[test]
fn a_missing_receipts_directory_refuses_as_unreadable() {
    let output = runner()
        .arg("--manifest")
        .arg(shipped_manifest())
        .arg("--receipts")
        .arg("does/not/exist")
        .output()
        .expect("the runner binary exists");

    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Unreadable"), "got: {stdout}");
}

/// Writes a symmetric, complete run over the SHIPPED twelve cases: every baseline 1000, every
/// compiled 400, so the ratio of medians is exactly 0.4.
fn write_dry_run(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("cases")).expect("run tree");
    let ids = [
        "events-open-verdict",
        "events-held-file",
        "schema-ceiling",
        "protocols-canonical-order",
        "cli-gate-workspace",
        "governor-validator-identity",
        "runtime-capsule-bytes",
        "events-idempotency",
        "mcp-refusal-identity",
        "benchmark-recall-order",
        "accounting-unavailable",
        "extension-inventory",
    ];
    let order: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    let arm = serde_json::json!({
        "snapshot": "snap-dry",
        "indexSnapshot": "index-dry",
        "objective": "dry run",
        "permissions": ["read"],
        "modelRoute": "fake",
        "modelSettings": "temperature=0",
        "cleanState": true,
        "order": order,
        "seed": 7,
        "clock": "2026-08-31T02:00:00Z",
        "budget": 100000,
        "acceptanceContract": "contract-dry",
        "binaryDigest": "sha256:build-dry",
        "environment": "env-dry",
        "cacheDiscipline": "cold",
    });
    std::fs::write(
        root.join("arms.json"),
        serde_json::to_vec_pretty(&serde_json::json!({ "baseline": arm, "compiled": arm }))
            .expect("arms encode"),
    )
    .expect("arms written");
    for id in ids {
        let record = serde_json::json!({
            "caseId": id,
            "baseline": { "provider_reported_input_tokens":
                { "value": 1000, "provenance": "measured", "producer": "provider", "note": "" },
                "requiredEvidenceFound": true },
            "compiled": { "provider_reported_input_tokens":
                { "value": 400, "provenance": "measured", "producer": "provider", "note": "" },
                "requiredEvidenceFound": true },
        });
        std::fs::write(
            root.join("cases").join(format!("{id}.json")),
            serde_json::to_vec_pretty(&record).expect("case encodes"),
        )
        .expect("case written");
    }
}
