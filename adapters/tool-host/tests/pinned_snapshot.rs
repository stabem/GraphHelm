//! #539 (D-042's middle clause): the provider reads an immutable, digest-pinned index snapshot
//! COPIED into Tier 1 -- never the live index the host is still writing to.
//!
//! The three words are load-bearing separately. COPIED is the boundary (reading the host index
//! in place is the forbidden thing). IMMUTABLE is what makes the receipt's generation true a
//! moment after it is read. DIGEST-PINNED is the sharpest: the live index's `head_sha` and its
//! content disagreed by two hours and nine merges when #539 was filed -- a snapshot identified
//! by a ref can be stale and still answer with confidence, so the generation must derive from
//! the BYTES.

use std::path::Path;

use graphhelm_tool_host::process::HostError;
use graphhelm_tool_host::snapshot::{PinnedSnapshot, pin_snapshot, verify_pinned};

fn source_index(root: &Path) {
    std::fs::create_dir_all(root.join("graph")).unwrap();
    std::fs::write(root.join("graph/nodes.bin"), b"node bytes v1").unwrap();
    std::fs::write(root.join("graph/edges.bin"), b"edge bytes v1").unwrap();
    std::fs::write(root.join("meta.json"), b"{\"generation\":\"g1\"}").unwrap();
}

/// POSITIVE CONTROL, first: pinning the same source twice yields the SAME digest -- the
/// generation is a pure function of the bytes, which is what lets a receipt bind it.
#[test]
fn pinning_the_same_bytes_twice_yields_the_same_digest() {
    let source = tempfile::tempdir().unwrap();
    source_index(source.path());
    let tier1_a = tempfile::tempdir().unwrap();
    let tier1_b = tempfile::tempdir().unwrap();

    let first = pin_snapshot(source.path(), tier1_a.path()).expect("a readable index pins");
    let second = pin_snapshot(source.path(), tier1_b.path()).expect("same bytes pin again");

    assert_eq!(
        first.generation(),
        second.generation(),
        "the generation derives from CONTENT, not from when or where the copy happened"
    );
    assert!(first.generation().starts_with("sha256-"));
}

/// Different bytes, different generation -- otherwise two corpora share one identity.
#[test]
fn different_content_yields_a_different_generation() {
    let source_a = tempfile::tempdir().unwrap();
    source_index(source_a.path());
    let source_b = tempfile::tempdir().unwrap();
    source_index(source_b.path());
    std::fs::write(source_b.path().join("graph/nodes.bin"), b"node bytes v2").unwrap();

    let tier1_a = tempfile::tempdir().unwrap();
    let tier1_b = tempfile::tempdir().unwrap();
    let pin_a = pin_snapshot(source_a.path(), tier1_a.path()).unwrap();
    let pin_b = pin_snapshot(source_b.path(), tier1_b.path()).unwrap();

    assert_ne!(pin_a.generation(), pin_b.generation());
}

/// The copy lives INSIDE the Tier 1 root -- containment canonicalised, the #538 pattern.
#[test]
fn the_copy_lives_inside_the_tier1_root() {
    let source = tempfile::tempdir().unwrap();
    source_index(source.path());
    let tier1 = tempfile::tempdir().unwrap();

    let pinned = pin_snapshot(source.path(), tier1.path()).unwrap();

    let canonical_root = tier1.path().canonicalize().unwrap();
    let canonical_copy = pinned.root().canonicalize().unwrap();
    assert!(
        canonical_copy.starts_with(&canonical_root),
        "the snapshot copy must live inside the workspace: {}",
        pinned.root().display()
    );
    assert!(
        pinned.root().join("graph/nodes.bin").is_file(),
        "the copy carries the index's files"
    );
}

/// THE DEMONSTRATION THE ISSUE DEMANDS: mutating the SOURCE after the pin changes nothing about
/// what a retrieval against the copy reads -- the copy is the boundary, not an optimisation.
#[test]
fn mutating_the_source_during_a_retrieval_cannot_change_what_it_reads() {
    let source = tempfile::tempdir().unwrap();
    source_index(source.path());
    let tier1 = tempfile::tempdir().unwrap();
    let pinned = pin_snapshot(source.path(), tier1.path()).unwrap();

    // The background watcher writes: the live index moves mid-retrieval.
    std::fs::write(
        source.path().join("graph/nodes.bin"),
        b"node bytes v2-MOVED",
    )
    .unwrap();
    std::fs::write(source.path().join("meta.json"), b"{\"generation\":\"g2\"}").unwrap();

    let read_back = std::fs::read(pinned.root().join("graph/nodes.bin")).unwrap();
    assert_eq!(
        read_back, b"node bytes v1",
        "the retrieval reads the PINNED bytes, not the moved source"
    );
    verify_pinned(pinned.root(), pinned.generation())
        .expect("the copy still hashes to its recorded generation after the source moved");
}

/// A snapshot whose digest does not match its recorded value refuses BEFORE retrieval, with
/// both values -- re-pin or investigate, never read anyway.
#[test]
fn a_tampered_copy_refuses_verification_naming_both_digests() {
    let source = tempfile::tempdir().unwrap();
    source_index(source.path());
    let tier1 = tempfile::tempdir().unwrap();
    let pinned = pin_snapshot(source.path(), tier1.path()).unwrap();

    // The sabotage the acceptance names: one byte in the COPY.
    std::fs::write(pinned.root().join("graph/edges.bin"), b"edge bytes vX").unwrap();

    let refused = verify_pinned(pinned.root(), pinned.generation())
        .expect_err("a copy that no longer hashes to its pin was offered to retrieval anyway");

    match refused {
        HostError::SnapshotMismatch { expected, actual } => {
            assert_eq!(expected, pinned.generation());
            assert_ne!(actual, expected);
        }
        other => panic!("the pin did not decide this: {other:?}"),
    }
}

/// Rename detection: the digest binds file bytes to their PATHS -- two files swapping contents
/// must change the generation, or a reordered index reads as the same snapshot.
#[test]
fn swapping_two_files_contents_changes_the_generation() {
    let source_a = tempfile::tempdir().unwrap();
    source_index(source_a.path());
    let source_b = tempfile::tempdir().unwrap();
    source_index(source_b.path());
    // Same BYTES overall, different assignment: nodes<->edges swapped.
    std::fs::write(source_b.path().join("graph/nodes.bin"), b"edge bytes v1").unwrap();
    std::fs::write(source_b.path().join("graph/edges.bin"), b"node bytes v1").unwrap();

    let tier1_a = tempfile::tempdir().unwrap();
    let tier1_b = tempfile::tempdir().unwrap();
    let pin_a = pin_snapshot(source_a.path(), tier1_a.path()).unwrap();
    let pin_b = pin_snapshot(source_b.path(), tier1_b.path()).unwrap();

    assert_ne!(
        pin_a.generation(),
        pin_b.generation(),
        "which bytes belong to which path is part of the identity"
    );
}

/// The pinned type is the doorway's currency: it exposes root and generation, and only
/// `pin_snapshot` constructs it -- an unpinned path cannot impersonate a snapshot.
#[test]
fn the_pinned_type_carries_root_and_generation_together() {
    let source = tempfile::tempdir().unwrap();
    source_index(source.path());
    let tier1 = tempfile::tempdir().unwrap();

    let pinned: PinnedSnapshot = pin_snapshot(source.path(), tier1.path()).unwrap();
    assert!(pinned.root().is_absolute());
    assert!(!pinned.generation().is_empty());
}

/// L's #551 fold: an EMPTY tree must refuse to pin. The empty population's digest is the
/// sha256 of the empty string -- one identity shared by every empty snapshot in the world --
/// so a provision that silently copied NOTHING (wrong path, watcher race, permissions) would
/// pin, verify forever, and read as success. Empty is a refusal, not an identity.
#[test]
fn an_empty_tree_refuses_to_pin() {
    let source = tempfile::tempdir().unwrap();
    let tier1 = tempfile::tempdir().unwrap();

    let refused = pin_snapshot(source.path(), tier1.path())
        .expect_err("a snapshot of nothing was pinned as if it were an index");

    match refused {
        HostError::Config { rule } => {
            assert_eq!(rule, "an index snapshot cannot be empty");
        }
        other => panic!("the population rule did not decide this: {other:?}"),
    }
}
