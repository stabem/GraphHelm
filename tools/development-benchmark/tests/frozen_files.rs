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
    // The retrieval artifacts carry the objective VERBATIM as their query, which is the
    // producer's own mechanical rule and what `verify_frozen_files` now cross-checks.
    let retrieval = root.join("retrieval");
    std::fs::create_dir_all(&retrieval).unwrap();
    write(
        &retrieval,
        "alpha.json",
        b"{\"caseId\":\"alpha\",\"query\":\"a\"}",
    );
    write(
        &retrieval,
        "beta.json",
        b"{\"caseId\":\"beta\",\"query\":\"b\"}",
    );
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

/// The FOURTH freeze (#225): the retrieval artifacts are inputs to the compiled arm the same way
/// the objectives are inputs to both -- an artifact regenerated after the freeze changes what
/// the run measures just as silently as a rewritten objective. The manifest carries
/// `retrievalDigest`, `verify_frozen_files` checks it, and a moved artifact refuses naming its
/// own kind so the operator lands on the right directory.
#[test]
fn a_moved_retrieval_artifact_refuses_with_its_own_kind() {
    use graphhelm_development_benchmark::{corpus_digest, load_manifest, verify_frozen_files};

    let directory = tempfile::tempdir().unwrap();
    arrange(directory.path());
    let retrieval = directory.path().join("retrieval");

    let case_values: Vec<serde_json::Value> = cases()
        .iter()
        .map(|case| serde_json::to_value(case).unwrap())
        .collect();
    let manifest_json = serde_json::json!({
        "manifestVersion": 2,
        "corpusDigest": corpus_digest(&case_values),
        "oracleDigest": frozen_files_digest(directory.path(), "oracle", &cases()).unwrap(),
        "objectivesDigest":
            frozen_files_digest(directory.path(), "objectives", &cases()).unwrap(),
        "retrievalDigest":
            frozen_files_digest(directory.path(), "retrieval", &cases()).unwrap(),
        "cases": case_values,
    });
    let manifest = load_manifest(&serde_json::to_string(&manifest_json).unwrap())
        .expect("a manifest with a retrieval freeze loads");

    // POSITIVE CONTROL: untouched, all four freezes verify.
    verify_frozen_files(&manifest, directory.path()).expect("untouched artifacts verify");

    // The query stays TRUE to the objective; only other bytes move, so this cell still tests the
    // digest rather than the new objective/query coupling.
    write(
        &retrieval,
        "beta.json",
        b"{\"caseId\":\"beta\",\"query\":\"b\",\"hits\":[]}",
    );

    let refusal = verify_frozen_files(&manifest, directory.path())
        .expect_err("a regenerated retrieval artifact verified anyway");
    match refusal {
        BenchmarkRefusal::FrozenFilesMismatch { kind, .. } => {
            assert_eq!(
                kind, "retrieval",
                "the refusal names the directory that moved"
            );
        }
        other => panic!("the freeze did not decide this: {other:?}"),
    }
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

/// THE GAP BETWEEN THE DIGESTS (Codex P2): three intact freezes, one incoherent corpus.
///
/// Edit an objective, refresh its digest, leave retrieval alone — every file-digest check still
/// passes, because each binds its own directory and nothing binds them to each other. The drive
/// then sends the NEW objective to the model while compiling hits selected for the OLD one.
///
/// The producer's mechanical rule is what makes this checkable: the query IS the objective,
/// verbatim. So this is the existing convention enforced, not a new one invented.
#[test]
fn an_objective_edited_without_regenerating_retrieval_refuses() {
    use graphhelm_development_benchmark::{corpus_digest, load_manifest, verify_frozen_files};

    let directory = tempfile::tempdir().unwrap();
    arrange(directory.path());

    // The edit, done PROPERLY: the objective changes and its digest is refreshed with it, so all
    // three freezes are internally valid. Only the coupling is broken.
    write(
        directory.path().join("objectives").as_path(),
        "beta.json",
        b"{\"objective\":\"a different question entirely\"}",
    );

    let case_values: Vec<serde_json::Value> = cases()
        .iter()
        .map(|case| serde_json::to_value(case).unwrap())
        .collect();
    let manifest = load_manifest(
        &serde_json::json!({
            "manifestVersion": 2,
            "corpusDigest": corpus_digest(&case_values),
            "oracleDigest": frozen_files_digest(directory.path(), "oracle", &cases()).unwrap(),
            "objectivesDigest":
                frozen_files_digest(directory.path(), "objectives", &cases()).unwrap(),
            "retrievalDigest":
                frozen_files_digest(directory.path(), "retrieval", &cases()).unwrap(),
            "cases": case_values,
        })
        .to_string(),
    )
    .expect("every digest is internally valid");

    let refusal = verify_frozen_files(&manifest, directory.path())
        .expect_err("three intact freezes let an incoherent corpus through");

    match refusal {
        BenchmarkRefusal::FrozenFilesMismatch {
            kind,
            declared,
            actual,
        } => {
            assert_eq!(kind, "retrieval");
            assert!(
                declared.contains('b') && actual.contains("different question"),
                "the refusal must show BOTH sides so the operator sees which moved: \
                 declared={declared}, actual={actual}"
            );
        }
        other => panic!("the coupling did not decide this: {other:?}"),
    }
}
