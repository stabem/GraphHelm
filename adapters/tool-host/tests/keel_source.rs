//! Focused proof for the Keel Runtime source adapter.
//!
//! The observable contracts are: retained bytes are searched and read from one generation,
//! tracked unsupported text remains searchable, relevant untracked files force the live pair,
//! scoped factory state is absent, and unsafe reader paths are refused. Existing channel tests
//! cover the live walk; these tests cover the new selection and snapshot boundary.

use std::process::Command;

use graphhelm_runtime::context::SEARCH_BOUNDS;
use graphhelm_runtime::ports::{
    BoundedSourceReader, BoundedSourceSearch, SourceReadError, SourceSearchOrigin,
    SourceSearchReason,
};
use graphhelm_tool_host::keel_source::{
    KeelSnapshotPorts, KeelSnapshotSelection, LiveFallbackSourceSearch,
};
use graphhelm_tool_host::source_channel::WorkspaceSourceChannel;

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-C", root.to_str().unwrap()])
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(
        dir.path(),
        &["config", "user.email", "keel-tests@example.invalid"],
    );
    git(dir.path(), &["config", "user.name", "Keel Tests"]);
    dir
}

fn commit_all(root: &std::path::Path) {
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "fixture"]);
}

#[test]
fn tracked_unsupported_readme_is_searchable_and_read_from_snapshot() {
    let dir = repository();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("README.md"),
        "unsupported prose marker keel_readme_marker\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "fn live_marker() {}\n").unwrap();
    commit_all(dir.path());

    let KeelSnapshotSelection::Snapshot(ports) = KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("tracked source should admit a snapshot")
    };
    let result = ports
        .search()
        .search_with_provenance(&["keel_readme_marker".to_owned()], &SEARCH_BOUNDS)
        .unwrap();
    assert_eq!(result.paths, ["README.md"]);
    assert_eq!(result.provenance.origin, SourceSearchOrigin::Snapshot);
    assert_eq!(
        ports.reader().read_prefix("README.md", 4096).unwrap().bytes,
        b"unsupported prose marker keel_readme_marker\n"
    );
}

#[test]
fn uppercase_source_suffix_remains_in_the_snapshot_policy() {
    let dir = repository();
    std::fs::write(dir.path().join("UPPER.RS"), "uppercase_keel_marker\n").unwrap();
    commit_all(dir.path());

    let KeelSnapshotSelection::Snapshot(ports) = KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("tracked source should admit a snapshot")
    };
    let result = ports
        .search()
        .search_with_provenance(&["uppercase_keel_marker".to_owned()], &SEARCH_BOUNDS)
        .unwrap();
    assert_eq!(result.paths, ["UPPER.RS"]);
}

#[test]
fn process_notes_are_filtered_before_the_result_cap() {
    let dir = repository();
    std::fs::create_dir_all(dir.path().join(".superpowers/sdd")).unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join(".superpowers/sdd/note.md"),
        "snapshot_marker\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "snapshot_marker\n").unwrap();
    commit_all(dir.path());

    let KeelSnapshotSelection::Snapshot(ports) = KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("tracked source should admit a snapshot")
    };
    let mut bounds = SEARCH_BOUNDS;
    bounds.max_results = 1;
    let result = ports
        .search()
        .search_with_provenance(&["snapshot_marker".to_owned()], &bounds)
        .unwrap();
    assert_eq!(result.paths, ["src/lib.rs"]);
}

#[test]
fn snapshot_keeps_search_and_read_on_the_same_generation_after_mutation() {
    let dir = repository();
    std::fs::write(dir.path().join("README.md"), "old_generation_marker\n").unwrap();
    commit_all(dir.path());

    let KeelSnapshotSelection::Snapshot(ports) = KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("tracked source should admit a snapshot")
    };
    std::fs::write(dir.path().join("README.md"), "new_generation_marker\n").unwrap();

    let result = ports
        .search()
        .search_with_provenance(&["old_generation_marker".to_owned()], &SEARCH_BOUNDS)
        .unwrap();
    assert_eq!(result.paths, ["README.md"]);
    assert_eq!(
        ports.reader().read_prefix("README.md", 4096).unwrap().bytes,
        b"old_generation_marker\n"
    );
}

#[test]
fn relevant_untracked_source_selects_live_fallback() {
    let dir = repository();
    std::fs::write(dir.path().join("tracked.md"), "tracked\n").unwrap();
    commit_all(dir.path());
    std::fs::write(dir.path().join("untracked.md"), "untracked_keel_marker\n").unwrap();

    let KeelSnapshotSelection::LiveFallback { reason } =
        KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("relevant untracked source must select live fallback")
    };
    assert_eq!(reason, SourceSearchReason::SnapshotCoverageGap);
    let live = WorkspaceSourceChannel::open(dir.path()).unwrap();
    let result = LiveFallbackSourceSearch::with_reason(live, reason)
        .search_with_provenance(&["untracked_keel_marker".to_owned()], &SEARCH_BOUNDS)
        .unwrap();
    assert_eq!(result.paths, ["untracked.md"]);
    assert_eq!(result.provenance.origin, SourceSearchOrigin::Fallback);
    assert_eq!(
        result.provenance.reason,
        SourceSearchReason::SnapshotCoverageGap
    );
}

#[test]
fn ignored_directory_with_source_selects_live_fallback() {
    let dir = repository();
    std::fs::write(dir.path().join(".gitignore"), "ignored/\n").unwrap();
    std::fs::create_dir_all(dir.path().join("ignored")).unwrap();
    std::fs::write(
        dir.path().join("ignored/source.rs"),
        "ignored_keel_marker\n",
    )
    .unwrap();
    commit_all(dir.path());

    let KeelSnapshotSelection::LiveFallback { reason } =
        KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("ignored source must select live fallback")
    };
    assert_eq!(reason, SourceSearchReason::SnapshotCoverageGap);
}

#[test]
fn snapshot_build_failure_selects_live_fallback_without_error_details() {
    let dir = repository();
    std::fs::write(dir.path().join("tracked.md"), "tracked\n").unwrap();
    commit_all(dir.path());
    // Corrupt only Git's index after the live channel has a valid directory. The snapshot's
    // Git-backed admission then fails, while the live bounded walk remains usable.
    std::fs::write(dir.path().join(".git/index"), b"not a git index").unwrap();

    let KeelSnapshotSelection::LiveFallback { reason } =
        KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("snapshot build failure must select live fallback")
    };
    assert_eq!(reason, SourceSearchReason::SnapshotUnavailable);

    let live = WorkspaceSourceChannel::open(dir.path()).unwrap();
    let result = LiveFallbackSourceSearch::with_reason(live, reason)
        .search_with_provenance(&["tracked".to_owned()], &SEARCH_BOUNDS)
        .unwrap();
    assert_eq!(result.paths, ["tracked.md"]);
    assert_eq!(
        result.provenance.reason,
        SourceSearchReason::SnapshotUnavailable
    );
}

#[test]
fn scoped_factory_state_is_not_snapshot_evidence_and_unsafe_paths_are_refused() {
    let dir = repository();
    std::fs::create_dir_all(dir.path().join(".factory")).unwrap();
    std::fs::write(
        dir.path().join(".factory/board.md"),
        "keel_factory_marker\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("README.md"), "safe\n").unwrap();
    commit_all(dir.path());

    let KeelSnapshotSelection::Snapshot(ports) = KeelSnapshotPorts::try_open(dir.path()).unwrap()
    else {
        panic!("tracked source should admit a snapshot")
    };
    let result = ports
        .search()
        .search_with_provenance(&["keel_factory_marker".to_owned()], &SEARCH_BOUNDS)
        .unwrap();
    assert!(result.paths.is_empty());
    assert_eq!(
        ports.reader().read_prefix("../README.md", 4096),
        Err(SourceReadError::Escape)
    );
}
