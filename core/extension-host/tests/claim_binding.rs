//! #546: the claim NAMES the root, so a mutation cannot land under a root the claim never covered.
//!
//! Until #546 every mutating entry point took `&ActivationClaim` next to an `install_root: &Path`
//! and read only the second. Possession of a claim proved the caller had acquired the lock
//! somewhere; it did not prove the caller had acquired it over the tree about to change, and
//! `uninstall_version` is a `remove_dir_all`. The measured red, taken on the pre-fix signatures at
//! 361080f5, was `uninstall_version(&claim_for_A, root_B, digest) -> Ok(())` with B's adopted tree
//! gone.
//!
//! **What these cells are, stated plainly so a green is not read as more than it is.** The fix
//! deletes the parameter rather than checking it, so the mismatch has no spelling and the red
//! above cannot be re-expressed through the public API -- the old cell does not compile. That
//! property is held by the TYPE, and a runtime cell cannot observe it. What these cells observe is
//! the half a type cannot hold: that the root the entry points derive is the one the claim was
//! acquired for. They go red if `install_root()` starts naming a different path, or if a future
//! edit reintroduces a root argument and reads it. They would NOT go red if someone reintroduced
//! the parameter and left it unread.
//!
//! Discrimination comes from installing the SAME package under both roots: the two adopted trees
//! carry the same digest, so an assertion about which tree died is an assertion about which ROOT
//! was used, and cannot be satisfied by the digest lookup accidentally agreeing.
//!
//! Fixture idiom follows `tests/install.rs` and `tests/uninstall.rs` in this crate -- the shipped
//! package, copied -- rather than `tests/links.rs`'s synthetic one, because these cells need one
//! identity and no identity move. Every arrangement step says HARNESS-BROKE when it fails, so
//! #589's failure mode (undeclared files landing in the shipped tree) reads as a broken harness
//! and never as a verdict on the binding.

use std::path::Path;

use graphhelm_extension_host::{
    ActivationClaim, active_versions, install_package, switch_active, uninstall_version,
};

const SHIPPED: &str = "../../extensions/builtin/graphhelm-development-contracts";

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("create the target directory");
    for entry in std::fs::read_dir(from).expect("read the source directory") {
        let entry = entry.expect("a directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("an entry type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).expect("copy a package file");
        }
    }
}

#[test]
fn the_anchored_root_is_the_one_the_claim_was_acquired_for() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    assert_eq!(
        claim.install_root(),
        root.path(),
        "the anchored root must be the path acquire was given, or every entry point derives a \
         root the caller never named"
    );
}

/// A trailing separator is gone from the anchored root, and that is not cosmetic.
///
/// `--root /some/path/` reaches `acquire` straight off a CLI argument. The anchored root is built
/// from `Path::components`, which drops the separator before the first open, so the root the entry
/// points derive is the same one either spelling produces. `tests/links.rs` measures the security
/// face of the same walk; this cell measures only that the two spellings anchor to ONE path, which
/// is what makes the assertion above stable under an ordinary way of typing a directory.
#[test]
fn a_root_named_with_a_trailing_separator_anchors_to_the_same_path() {
    let root = tempfile::tempdir().expect("a temp dir");
    let mut trailing = root.path().to_path_buf().into_os_string();
    trailing.push(std::path::MAIN_SEPARATOR.to_string());
    let trailing = std::path::PathBuf::from(trailing);
    assert_ne!(
        trailing.as_os_str(),
        root.path().as_os_str(),
        "HARNESS-BROKE: the two spellings are the same string, so the cell compares nothing"
    );

    let claim = ActivationClaim::acquire(&trailing).expect("the claim must be granted");

    assert_eq!(
        claim.install_root(),
        root.path(),
        "a trailing separator must not produce a second anchored root for one directory"
    );
}

#[test]
fn a_write_lands_under_the_claims_own_root_and_leaves_the_other_alone() {
    let root_a = tempfile::tempdir().expect("a temp dir");
    let root_b = tempfile::tempdir().expect("a temp dir");
    let staged = tempfile::tempdir().expect("a temp dir");

    let package = staged.path().join("pkg");
    copy_tree(Path::new(SHIPPED), &package);

    // ARRANGEMENT: B holds an adopted tree of its own, under B's own claim.
    let adopted_b = {
        let claim_b = ActivationClaim::acquire(root_b.path())
            .expect("HARNESS-BROKE: B's claim was not granted");
        install_package(&claim_b, &package)
            .expect("HARNESS-BROKE: the package did not adopt under B")
            .root
    };
    assert!(
        adopted_b.is_dir(),
        "HARNESS-BROKE: B has no adopted tree, so the untouched-B assertions below hold vacuously"
    );
    assert_eq!(
        active_versions(root_b.path()).expect("B's pointer reads"),
        None,
        "CONTROL: B never switched, so a pointer under B afterwards can only have come from A's \
         claim"
    );

    let claim_a = ActivationClaim::acquire(root_a.path()).expect("A's claim must be granted");
    let installed_a = install_package(&claim_a, &package).expect("the package adopts under A");

    assert!(
        installed_a.root.starts_with(root_a.path()),
        "the adopted tree landed at {}, outside the claim's own root {}",
        installed_a.root.display(),
        root_a.path().display()
    );
    assert_ne!(
        installed_a.root, adopted_b,
        "HARNESS-BROKE: the two roots resolved to one directory, so nothing below discriminates"
    );
    assert_eq!(
        installed_a.root.file_name(),
        adopted_b.file_name(),
        "HARNESS-BROKE: the two roots hold different identities, so an assertion about WHICH tree \
         was written could be satisfied by the digest lookup instead of by the root"
    );

    let active = switch_active(&claim_a, &installed_a.digest).expect("the switch lands");
    assert_eq!(active.current, installed_a.digest);
    assert_eq!(
        active_versions(root_a.path()).expect("A's pointer reads"),
        Some(active),
        "the pointer write must land under the claim's own root"
    );
    assert_eq!(
        active_versions(root_b.path()).expect("B's pointer reads"),
        None,
        "a claim anchored to A wrote a pointer under B"
    );
}

#[test]
fn a_removal_takes_the_claims_own_tree_while_the_same_digest_survives_elsewhere() {
    let root_a = tempfile::tempdir().expect("a temp dir");
    let root_b = tempfile::tempdir().expect("a temp dir");
    let staged = tempfile::tempdir().expect("a temp dir");

    let package = staged.path().join("pkg");
    copy_tree(Path::new(SHIPPED), &package);

    let adopted_b = {
        let claim_b = ActivationClaim::acquire(root_b.path())
            .expect("HARNESS-BROKE: B's claim was not granted");
        install_package(&claim_b, &package)
            .expect("HARNESS-BROKE: the package did not adopt under B")
            .root
    };
    let sentinel = adopted_b.join("extension.json");
    let sentinel_bytes =
        std::fs::read(&sentinel).expect("HARNESS-BROKE: B's adopted tree has no manifest to watch");

    let claim_a = ActivationClaim::acquire(root_a.path()).expect("A's claim must be granted");
    let installed_a = install_package(&claim_a, &package).expect("the package adopts under A");
    assert_eq!(
        installed_a.root.file_name(),
        adopted_b.file_name(),
        "HARNESS-BROKE: the two adopted trees carry different digests, so the survival below could \
         be explained by the lookup rather than by the root"
    );
    assert!(
        installed_a.root.is_dir() && adopted_b.is_dir(),
        "HARNESS-BROKE: both trees must exist before the removal, or the survival assertion is \
         about an absence that was already there"
    );

    // Neither root switched, so the digest is retained by no pointer and the removal is allowed.
    uninstall_version(&claim_a, &installed_a.digest).expect("the free version uninstalls");

    assert!(
        !installed_a.root.exists(),
        "the removal must take the tree under the claim's own root"
    );
    assert!(
        adopted_b.is_dir(),
        "a claim anchored to {} removed the identically-digested tree at {} under an unrelated \
         root",
        root_a.path().display(),
        adopted_b.display()
    );
    assert_eq!(
        std::fs::read(&sentinel).expect("B's manifest still reads"),
        sentinel_bytes,
        "B's adopted tree survived as a directory but its contents were reached"
    );
}
