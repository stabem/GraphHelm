//! #543: the production `SourceReader` — nothing that ships decided what the workspace snapshot
//! was, and the port's own doc says the implementor "is caught by the guard, not by the type".
//! These are those guards, on the PRODUCER side for the first time.
//!
//! The key cell is the uncommitted edit: it is what separates a content digest from a commit
//! SHA. A reader answering with a ref would pass the mutate cell (commit moves the SHA) and the
//! negative control (no change, same SHA) — only an edit that no commit blessed tells the two
//! apart, and it is the exact silent slice G2b was written to describe.

use std::path::{Path, PathBuf};
use std::process::Command;

use graphhelm_runtime::ports::SourceReader;
use graphhelm_tool_host::process::HostError;
use graphhelm_tool_host::source_reader::{SourceReaderLimits, WorkspaceSourceReader};

fn scratch_repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), "// scratch v1\n").unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&project)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "scratch")
            .env("GIT_AUTHOR_EMAIL", "scratch@test.invalid")
            .env("GIT_COMMITTER_NAME", "scratch")
            .env("GIT_COMMITTER_EMAIL", "scratch@test.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    };
    git(&["init", "--quiet"]);
    git(&["add", "-A"]);
    git(&["commit", "--quiet", "-m", "scratch"]);
    (dir, project)
}

fn limits() -> SourceReaderLimits {
    SourceReaderLimits {
        max_files: 10_000,
        max_bytes: 64 * 1024 * 1024,
    }
}

/// NEGATIVE CONTROL, first: touching nothing, the id does not move. Without this cell an
/// implementation returning a fresh random id every call passes the mutation guard.
#[test]
fn touching_nothing_keeps_the_id_still() {
    let (_dir, project) = scratch_repo();
    let reader = WorkspaceSourceReader::open(&project, limits()).unwrap();

    let first = reader.current_snapshot();
    let second = reader.current_snapshot();

    assert_eq!(first, second, "an unchanged workspace has one identity");
}

/// Mutating one tracked file moves the id.
#[test]
fn mutating_one_tracked_file_moves_the_id() {
    let (_dir, project) = scratch_repo();
    let reader = WorkspaceSourceReader::open(&project, limits()).unwrap();
    let before = reader.current_snapshot();

    std::fs::write(project.join("src/lib.rs"), "// scratch v2 MUTATED\n").unwrap();

    let after = reader.current_snapshot();
    assert_ne!(
        before, after,
        "the bytes moved; the identity must move with them"
    );
}

/// THE CELL THE ISSUE EXISTS FOR: an UNCOMMITTED edit moves the id. A commit SHA survives an
/// uncommitted edit — a content digest cannot. No `git commit` happens in this test, and the
/// arrangement asserts exactly that.
#[test]
fn an_uncommitted_edit_moves_the_id() {
    let (_dir, project) = scratch_repo();
    let head_before = git_head(&project);
    let reader = WorkspaceSourceReader::open(&project, limits()).unwrap();
    let before = reader.current_snapshot();

    // Edit WITHOUT committing.
    std::fs::write(project.join("src/lib.rs"), "// edited, never committed\n").unwrap();
    assert_eq!(
        git_head(&project),
        head_before,
        "arrangement: HEAD must not have moved, or this cell degenerates into the mutate cell"
    );

    let after = reader.current_snapshot();
    assert_ne!(
        before, after,
        "an uncommitted edit changed the bytes; a reader whose id survived it is answering \
         with a ref, and the whole staleness mechanism would say fresh about bytes that moved"
    );
}

/// The walk is BOUNDED and the bound refuses at open, by name — cost stated, not discovered.
#[test]
fn a_workspace_beyond_the_bound_refuses_at_open() {
    let (_dir, project) = scratch_repo();
    for index in 0..5 {
        std::fs::write(project.join(format!("file-{index}.txt")), b"x").unwrap();
    }

    let refused = WorkspaceSourceReader::open(
        &project,
        SourceReaderLimits {
            max_files: 3,
            max_bytes: 64 * 1024 * 1024,
        },
    )
    .expect_err("a workspace past the declared bound was opened anyway");

    match refused {
        HostError::Config { rule } => {
            assert_eq!(rule, "the workspace exceeds the reader's declared bounds");
        }
        other => panic!("the bound did not decide this: {other:?}"),
    }
}

/// Fail-closed on unreadability: a workspace that vanishes under the reader yields identities
/// that NEVER read as fresh — each failure answer differs from the last, so the staleness gate
/// refuses instead of serving bytes nobody can name.
#[test]
fn an_unreadable_workspace_never_answers_fresh() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), b"// here then gone\n").unwrap();
    let reader = WorkspaceSourceReader::open(&project, limits()).unwrap();
    let healthy = reader.current_snapshot();

    std::fs::remove_dir_all(&project).unwrap();

    let first_failure = reader.current_snapshot();
    let second_failure = reader.current_snapshot();
    assert_ne!(
        healthy, first_failure,
        "unreadable must not impersonate the healthy identity"
    );
    assert_ne!(
        first_failure, second_failure,
        "two failure answers must differ, or a persistent failure reads as a stable, fresh \
         identity"
    );
}

/// The consumer integration: `compile_plan_against` with the REAL reader — fresh passes, a
/// mutation under an unchanged binding refuses `index_stale`. G2b's promise, now held against a
/// producer that ships.
#[test]
fn the_real_reader_drives_the_staleness_refusal_end_to_end() {
    use graphhelm_runtime::retrieval::{IndexResponse, RetrievalOutcome, compile_plan_against};

    let (_dir, project) = scratch_repo();
    let reader = WorkspaceSourceReader::open(&project, limits()).unwrap();
    let snapshot = reader.current_snapshot();
    let binding: graphhelm_protocols::SnapshotBinding = serde_json::from_value(serde_json::json!({
        "repoSnapshot": snapshot.as_str(),
        "indexGeneration": snapshot.as_str(),
    }))
    .unwrap();
    let response = IndexResponse::new(
        vec!["src/lib.rs".to_owned()],
        serde_json::from_value(serde_json::json!("complete")).unwrap(),
    );

    match compile_plan_against(&binding, &response, &reader) {
        RetrievalOutcome::Claim { .. } => {}
        other => panic!("a fresh binding over the real reader must compile: {other:?}"),
    }

    // The bytes move; the binding does not. The reader is what catches it.
    std::fs::write(project.join("src/lib.rs"), "// moved under the binding\n").unwrap();
    match compile_plan_against(&binding, &response, &reader) {
        RetrievalOutcome::Refused { code } => {
            assert_eq!(code.wire_name(), "index_stale");
        }
        other => panic!("moved bytes under an agreeing binding must refuse: {other:?}"),
    }
}

fn git_head(project: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// L's #556 finding, verbatim as a cell: the failure identity must not repeat ACROSS instances.
/// A per-instance counter salted only by the root repeats between readers (A and B, first
/// failure each, same id) -- and then a binding recorded during unreadability plus a NEW reader
/// that is also unreadable COMPARES EQUAL and reads FRESH: a fail-open inside the fail-closed
/// mechanism, in the exact scenario it exists for.
#[test]
fn two_readers_first_failures_never_share_an_identity() {
    let directory = tempfile::tempdir().unwrap();
    let project = directory.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src/lib.rs"), b"// here then gone\n").unwrap();
    let reader_a = WorkspaceSourceReader::open(&project, limits()).unwrap();
    let reader_b = WorkspaceSourceReader::open(&project, limits()).unwrap();

    std::fs::remove_dir_all(&project).unwrap();

    let failure_a = reader_a.current_snapshot();
    let failure_b = reader_b.current_snapshot();
    assert_ne!(
        failure_a, failure_b,
        "two INSTANCES' first failures shared an identity: a binding recorded during \
         unreadability would compare equal against a fresh reader's failure and read FRESH"
    );
}
