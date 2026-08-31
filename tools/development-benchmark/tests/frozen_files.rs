//! The oracle binds by RAW BYTES, and so do the objectives (K's #504 finding 1).
//!
//! `corpusDigest` freezes the manifest's `{id, oracleId}` pairs and NOTHING froze the files they
//! point at: Bar 2 (recall = 100%) is computed against `requiredEvidence`, so an oracle could be
//! softened without moving the freeze -- the blueprint's SS9 predicted exactly this attack ("the
//! oracle gets weak exactly when retrieval is optimised"). Raw bytes, not canonical JSON, per the
//! folded SS7b decision: a corpus exists to be REPLAYED, and a canonical digest is deliberately
//! blind to bytes that differ.

use graphhelm_development_benchmark::{BenchmarkRefusal, Case, frozen_files_digest};

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) {
    std::fs::write(dir.join(name), bytes).expect("fixture file written");
}

fn cases() -> Vec<Case> {
    vec![
        Case {
            id: "alpha".to_owned(),
            oracle_id: "oracle-alpha".to_owned(),
        },
        Case {
            id: "beta".to_owned(),
            oracle_id: "oracle-beta".to_owned(),
        },
    ]
}

fn arrange(root: &std::path::Path) {
    let oracle = root.join("oracle");
    let objectives = root.join("objectives");
    std::fs::create_dir_all(&oracle).unwrap();
    std::fs::create_dir_all(&objectives).unwrap();
    write(&oracle, "alpha.json", b"{\"answer\":\"a\"}");
    write(&oracle, "beta.json", b"{\"answer\":\"b\"}");
    write(&objectives, "alpha.json", b"{\"objective\":\"a\"}");
    write(&objectives, "beta.json", b"{\"objective\":\"b\"}");
}

/// POSITIVE CONTROL, first: untouched files digest to the same value twice.
#[test]
fn untouched_files_digest_stably() {
    let directory = tempfile::tempdir().unwrap();
    arrange(directory.path());

    let first = frozen_files_digest(directory.path(), "oracle", &cases())
        .expect("readable oracle files digest");
    let second = frozen_files_digest(directory.path(), "oracle", &cases())
        .expect("readable oracle files digest");

    assert_eq!(
        first, second,
        "the digest must be a pure function of the bytes"
    );
    assert!(first.starts_with("sha256:"));
}

/// The attack itself: soften one oracle byte, the digest MUST move. This is what nothing
/// guaranteed before -- requiredEvidence could change with the freeze intact.
#[test]
fn a_softened_oracle_moves_the_digest() {
    let directory = tempfile::tempdir().unwrap();
    arrange(directory.path());
    let frozen = frozen_files_digest(directory.path(), "oracle", &cases()).unwrap();

    write(
        directory.path().join("oracle").as_path(),
        "beta.json",
        b"{\"answer\":\"B\"}",
    );

    let softened = frozen_files_digest(directory.path(), "oracle", &cases()).unwrap();
    assert_ne!(
        frozen, softened,
        "one byte in an oracle changed and the freeze did not move: recall is judged against \
         bytes nothing binds"
    );
}

/// Order and identity are part of the freeze: the SAME files under swapped case ids must not
/// collide with the original, or two corpora that grade differently share one digest.
#[test]
fn the_digest_binds_file_bytes_to_their_case_ids() {
    let directory = tempfile::tempdir().unwrap();
    arrange(directory.path());
    let frozen = frozen_files_digest(directory.path(), "oracle", &cases()).unwrap();

    // Swap the two oracle files' CONTENT while keeping both names present.
    write(
        directory.path().join("oracle").as_path(),
        "alpha.json",
        b"{\"answer\":\"b\"}",
    );
    write(
        directory.path().join("oracle").as_path(),
        "beta.json",
        b"{\"answer\":\"a\"}",
    );

    let swapped = frozen_files_digest(directory.path(), "oracle", &cases()).unwrap();
    assert_ne!(
        frozen, swapped,
        "which bytes belong to which case is part of the freeze"
    );
}

/// A case whose file is missing is a refusal naming the path, never a digest over the subset --
/// a partial corpus is a DIFFERENT corpus.
#[test]
fn a_missing_file_refuses_instead_of_digesting_the_subset() {
    let directory = tempfile::tempdir().unwrap();
    arrange(directory.path());
    std::fs::remove_file(directory.path().join("oracle/beta.json")).unwrap();

    let refusal = frozen_files_digest(directory.path(), "oracle", &cases())
        .expect_err("a case with no oracle file was digested anyway");

    match refusal {
        BenchmarkRefusal::Unreadable { detail } => {
            assert!(
                detail.contains("beta"),
                "the refusal must name what is missing, got: {detail}"
            );
        }
        other => panic!("readability did not decide this: {other:?}"),
    }
}
