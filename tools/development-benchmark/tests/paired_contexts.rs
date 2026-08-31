//! The two arms' contexts, built by the harness under rules with the discretion squeezed out
//! (#506 reassigned: the compiled arm's SUBSTANCE, selector design on the issue).
//!
//! The compiled arm consumes a FROZEN retrieval artifact through the SHIPPED plan discipline
//! (`compile_plan_within`: stale refuses, escapes refuse, over-budget refuses) and lands in
//! `compile_capsule` with the section decision made EXPLICITLY -- `task` for the objective,
//! `evidence` for the surviving hits -- which is what the #404 seal said content wiring needs.
//! The naive arm is a declared lower bound: objective plus the full raw evidence files.
//!
//! And the oracle's ANSWER never enters either context: the driver reads the oracle file only to
//! learn the evidence list, through a struct that cannot see the answer field.

use std::path::Path;

use graphhelm_development_benchmark::{
    BenchmarkRefusal, RetrievalArtifact, capsule_for_case, naive_context, oracle_evidence_paths,
};

fn artifact(fresh: bool, hits: &[&str]) -> RetrievalArtifact {
    RetrievalArtifact {
        case_id: "alpha".to_owned(),
        query: "which function decides the verdict?".to_owned(),
        repo_snapshot: "sha256-snap-g".to_owned(),
        index_generation: if fresh {
            "sha256-snap-g".to_owned()
        } else {
            "sha256-snap-old".to_owned()
        },
        hits: hits
            .iter()
            .map(|hit| graphhelm_development_benchmark::RetrievalHit {
                path: (*hit).to_owned(),
                lines: None,
            })
            .collect(),
        coverage: "complete".to_owned(),
        pages: 1,
        max_results: 50,
        max_pages: 8,
        max_bytes: 1_000_000,
        max_tokens: 250_000,
    }
}

fn repo(root: &Path) {
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), b"pub fn decide() -> u8 { 42 }").unwrap();
    std::fs::write(root.join("src/other.rs"), b"pub fn unrelated() {}").unwrap();
}

/// POSITIVE CONTROL, first: a fresh artifact compiles a capsule that CARRIES the objective and
/// the hit's content, and reports which paths went in.
#[test]
fn a_fresh_artifact_compiles_the_objective_and_the_hits_into_the_capsule() {
    let directory = tempfile::tempdir().unwrap();
    repo(directory.path());

    let compiled = capsule_for_case(
        "which function decides the verdict?",
        &artifact(true, &["src/lib.rs"]),
        directory.path(),
    )
    .expect("a fresh, in-bounds artifact compiles");

    let text = String::from_utf8_lossy(&compiled.capsule);
    assert!(
        text.contains("which function decides the verdict?"),
        "the objective lands in the capsule (task section)"
    );
    assert!(
        text.contains("pub fn decide()"),
        "the hit's CONTENT lands in the capsule (evidence section), not just its name"
    );
    assert!(
        !text.contains("unrelated"),
        "a file the index did not select must not leak in"
    );
    assert_eq!(compiled.evidence_paths, vec!["src/lib.rs".to_owned()]);
}

/// A stale index refuses THROUGH the shipped discipline, and the refusal carries the wire code.
#[test]
fn a_stale_artifact_refuses_with_the_index_stale_code() {
    let directory = tempfile::tempdir().unwrap();
    repo(directory.path());

    let refusal = capsule_for_case(
        "which function decides the verdict?",
        &artifact(false, &["src/lib.rs"]),
        directory.path(),
    )
    .expect_err("coordinates from generation G resolved against other bytes");

    match refusal {
        BenchmarkRefusal::RetrievalRefused { code, case } => {
            assert_eq!(code, "index_stale");
            assert_eq!(case, "alpha");
        }
        other => panic!("the plan discipline did not decide this: {other:?}"),
    }
}

/// An escaping hit refuses -- the artifact is data from a provider, not a trusted path list.
#[test]
fn an_escaping_hit_refuses_before_any_read() {
    let directory = tempfile::tempdir().unwrap();
    repo(directory.path());

    let refusal = capsule_for_case(
        "which function decides the verdict?",
        &artifact(true, &["../outside.rs"]),
        directory.path(),
    )
    .expect_err("a traversal path from the index was handed to a reader");

    match refusal {
        BenchmarkRefusal::RetrievalRefused { code, .. } => {
            assert_eq!(code, "scope_mismatch");
        }
        other => panic!("the plan discipline did not decide this: {other:?}"),
    }
}

/// The naive arm: objective plus the FULL files, in evidence order -- a declared lower bound.
#[test]
fn the_naive_context_carries_the_objective_and_the_full_files() {
    let directory = tempfile::tempdir().unwrap();
    repo(directory.path());

    let context = naive_context(
        "which function decides the verdict?",
        &["src/lib.rs".to_owned(), "src/other.rs".to_owned()],
        directory.path(),
    )
    .expect("readable evidence builds a naive context");

    assert!(context.starts_with("which function decides the verdict?"));
    assert!(context.contains("pub fn decide()"));
    assert!(
        context.contains("unrelated"),
        "naive reads EVERYTHING it is pointed at"
    );
}

/// The oracle reader extracts the evidence list and CANNOT see the answer: a canary planted in
/// the answer field must be unreachable from anything the arms are built from.
#[test]
fn the_oracle_reader_is_blind_to_the_answer_field() {
    let directory = tempfile::tempdir().unwrap();
    let oracle = directory.path().join("alpha.json");
    std::fs::write(
        &oracle,
        br#"{
            "oracleId": "oracle-alpha",
            "requiredEvidence": ["src/lib.rs"],
            "answer": "CANARY-THE-ANSWER-IS-42-CANARY"
        }"#,
    )
    .unwrap();

    let (paths, raw) = oracle_evidence_paths(&oracle).expect("a well-formed oracle reads");

    assert_eq!(paths, vec!["src/lib.rs".to_owned()]);
    assert!(
        !raw.contains("CANARY"),
        "whatever the reader returns beside the paths must not carry the answer: the driver \
         builds contexts only from this function's output"
    );
}

/// The function grain (#506 selector, revised by measurement): a hit carrying a line range lands
/// ONLY those lines in the capsule. File-grain was the harness's own inflation -- the index
/// returns functions -- and the real-corpus dry run measured it at ratio 3.9, recall 0/12.
#[test]
fn a_ranged_hit_lands_only_its_lines_in_the_capsule() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("src")).unwrap();
    std::fs::write(
        directory.path().join("src/lib.rs"),
        b"line one
pub fn decide() -> u8 {
    42
}
line five is noise
",
    )
    .unwrap();

    let mut ranged = artifact(true, &[]);
    ranged.hits = vec![graphhelm_development_benchmark::RetrievalHit {
        path: "src/lib.rs".to_owned(),
        lines: Some("2-4".to_owned()),
    }];

    let compiled = capsule_for_case(
        "which function decides the verdict?",
        &ranged,
        directory.path(),
    )
    .expect("a fresh ranged artifact compiles");

    let text = String::from_utf8_lossy(&compiled.capsule);
    assert!(
        text.contains("pub fn decide()"),
        "the range's content lands"
    );
    assert!(
        !text.contains("line five is noise"),
        "content OUTSIDE the range must not ride in: the range is the whole point of the grain"
    );
    assert_eq!(compiled.evidence_paths, vec!["src/lib.rs".to_owned()]);
}

/// Codex r2 P1, K-confirmed shape: two hits from the SAME file with different ranges must land
/// BOTH slices -- the by-path lookup repeated the first range and silently dropped the rest,
/// which biased the fn-grain dry run downward on every case with duplicate paths.
#[test]
fn duplicate_path_hits_land_each_declared_range() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("src")).unwrap();
    std::fs::write(
        directory.path().join("src/lib.rs"),
        b"alpha-first\nalpha-second\nbeta-first\nbeta-second\n",
    )
    .unwrap();

    let mut ranged = artifact(true, &[]);
    ranged.hits = vec![
        graphhelm_development_benchmark::RetrievalHit {
            path: "src/lib.rs".to_owned(),
            lines: Some("1-2".to_owned()),
        },
        graphhelm_development_benchmark::RetrievalHit {
            path: "src/lib.rs".to_owned(),
            lines: Some("3-4".to_owned()),
        },
    ];

    let compiled = capsule_for_case("objective", &ranged, directory.path())
        .expect("two ranges of one file compile");

    let text = String::from_utf8_lossy(&compiled.capsule);
    assert!(text.contains("alpha-first"), "first range lands");
    assert!(
        text.contains("beta-first"),
        "the SECOND range must land too, not a repeat of the first"
    );
    assert_eq!(
        text.matches("alpha-first").count(),
        1,
        "the first range must not be duplicated in the second's place"
    );
}

/// Codex r2 P2: a range beyond the file is a lying artifact, refused by name -- never a silent
/// short slice and never an arithmetic panic.
#[test]
fn a_range_beyond_the_file_refuses_naming_it() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("src")).unwrap();
    std::fs::write(directory.path().join("src/lib.rs"), b"one\ntwo\n").unwrap();

    let mut ranged = artifact(true, &[]);
    ranged.hits = vec![graphhelm_development_benchmark::RetrievalHit {
        path: "src/lib.rs".to_owned(),
        lines: Some("1-999999".to_owned()),
    }];

    let refusal = capsule_for_case("objective", &ranged, directory.path())
        .expect_err("a range past the end of the file was sliced anyway");

    match refusal {
        BenchmarkRefusal::Unreadable { detail } => {
            assert!(
                detail.contains("999999") && detail.contains("src/lib.rs"),
                "the refusal names the range and the file, got: {detail}"
            );
        }
        other => panic!("readability did not decide this: {other:?}"),
    }
}
