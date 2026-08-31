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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../extensions/builtin/graphhelm-development-contracts/fixtures/benchmark/manifest.json",
    )
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
