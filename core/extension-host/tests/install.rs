//! Atomic version switch with rollback (#212): the pointer flips whole or not at all, a refusal
//! leaves the previous version active, and rollback returns to the previous known-good.
//!
//! The fixture is the repository's own shipped package, copied into a temp directory -- the same
//! choice `tests/staging.rs` made, for the same reason: the digest under test is then the one
//! `validate_extension_package` produces, and computing a second package digest would be a second
//! oracle, and two oracles diverge in silence. A second DISTINCT version is manufactured the way
//! staging's substitution fixture does it: mutate one declared file AND update its declared
//! digest, so the package stays internally consistent and only its identity moves.

use std::path::Path;

use graphhelm_extension_host::{
    ActivationClaim, InstallRefusal, active_versions, install_package, roll_back, switch_active,
};

const SHIPPED: &str = "../../extensions/builtin/graphhelm-development-contracts";

fn sha256_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

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

/// Move a package's IDENTITY: mutate one declared file and update the manifest digest so the
/// result still validates and only the package digest differs.
fn move_identity(package: &Path) {
    let declared = package.join("policies").join("memory-admission.yaml");
    let mut bytes = std::fs::read(&declared).expect("read a declared file");
    let before = sha256_of(&bytes);
    bytes.push(b'\n');
    let after = sha256_of(&bytes);
    std::fs::write(&declared, &bytes).expect("mutate a declared file");

    let manifest_path = package.join("extension.json");
    let manifest = std::fs::read_to_string(&manifest_path).expect("read the manifest");
    assert!(
        manifest.contains(&before),
        "HARNESS-BROKE: the manifest does not declare the digest this fixture replaces, so the \
         second version would not validate and every refusal below would come from the validator"
    );
    std::fs::write(&manifest_path, manifest.replace(&before, &after)).expect("write the manifest");
}

#[test]
fn an_installed_version_switches_atomically_and_the_previous_is_recorded() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    let staged = tempfile::tempdir().expect("a temp dir");
    let package_a = staged.path().join("a");
    copy_tree(Path::new(SHIPPED), &package_a);
    let package_b = staged.path().join("b");
    copy_tree(Path::new(SHIPPED), &package_b);
    move_identity(&package_b);

    assert_eq!(
        active_versions(root.path()).expect("a fresh root reads"),
        None,
        "CONTROL: a root that never activated must read as no active version, or every \
         assertion below is about leftovers"
    );

    let a = install_package(&claim, &package_a).expect("package A adopts");
    let b = install_package(&claim, &package_b).expect("package B adopts");
    assert_ne!(
        a.digest, b.digest,
        "HARNESS-BROKE: the two fixtures share a digest, so switching between them measures \
         nothing about the pointer"
    );

    let first = switch_active(&claim, &a.digest).expect("the first switch lands");
    assert_eq!(first.current, a.digest);
    assert_eq!(
        first.previous, None,
        "the first activation has nothing to retire"
    );

    let second = switch_active(&claim, &b.digest).expect("the second switch lands");
    assert_eq!(second.current, b.digest);
    assert_eq!(
        second.previous.as_deref(),
        Some(a.digest.as_str()),
        "the retired version must be recorded as previous, or rollback has no target"
    );

    let read_back = active_versions(root.path())
        .expect("the pointer reads")
        .expect("a version is active");
    assert_eq!(
        read_back, second,
        "what the switch returned and what the pointer persists must be one fact"
    );
}

#[test]
fn a_switch_to_an_unknown_digest_is_refused_with_the_pointer_untouched() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    let staged = tempfile::tempdir().expect("a temp dir");
    let package = staged.path().join("a");
    copy_tree(Path::new(SHIPPED), &package);
    let installed = install_package(&claim, &package).expect("the package adopts");
    switch_active(&claim, &installed.digest).expect("the switch lands");

    assert_eq!(
        switch_active(&claim, "sha256:0000000000000000").err(),
        Some(InstallRefusal::UnknownVersion),
        "a digest that was never adopted must refuse"
    );
    assert_eq!(
        active_versions(root.path())
            .expect("the pointer reads")
            .expect("a version is active")
            .current,
        installed.digest,
        "the refused switch must leave the previous version active -- the issue's own invariant"
    );
}

/// The issue's invariant, on the interesting arrangement: the target was adopted and its bytes
/// moved AFTERWARDS. The switch re-derives the digest at the moment of use; trusting the
/// directory name would be verify-then-use over a mutable tree.
#[test]
fn a_switch_whose_target_no_longer_matches_its_adopted_digest_refuses_and_the_old_version_stays() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    let staged = tempfile::tempdir().expect("a temp dir");
    let package_a = staged.path().join("a");
    copy_tree(Path::new(SHIPPED), &package_a);
    let package_b = staged.path().join("b");
    copy_tree(Path::new(SHIPPED), &package_b);
    move_identity(&package_b);

    let a = install_package(&claim, &package_a).expect("package A adopts");
    let b = install_package(&claim, &package_b).expect("package B adopts");
    switch_active(&claim, &a.digest).expect("A activates");

    // Corrupt B's ADOPTED tree the consistent way: the tree still validates, its identity moved.
    move_identity(&b.root);

    let refusal = switch_active(&claim, &b.digest)
        .expect_err("a tree whose bytes moved after adoption activated anyway");
    assert_eq!(
        refusal,
        InstallRefusal::AdoptedBytesChanged,
        "the refusal must name the moved bytes, not collapse into a generic failure"
    );
    assert_eq!(
        active_versions(root.path())
            .expect("the pointer reads")
            .expect("a version is active")
            .current,
        a.digest,
        "failed activation must leave the previous version active"
    );
}

#[test]
fn roll_back_returns_to_the_previous_version_and_refuses_without_one() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    assert_eq!(
        roll_back(&claim).err(),
        Some(InstallRefusal::NoPreviousVersion),
        "a root that never activated has nothing to roll back to"
    );

    let staged = tempfile::tempdir().expect("a temp dir");
    let package_a = staged.path().join("a");
    copy_tree(Path::new(SHIPPED), &package_a);
    let package_b = staged.path().join("b");
    copy_tree(Path::new(SHIPPED), &package_b);
    move_identity(&package_b);

    let a = install_package(&claim, &package_a).expect("package A adopts");
    let b = install_package(&claim, &package_b).expect("package B adopts");
    switch_active(&claim, &a.digest).expect("A activates");

    assert_eq!(
        roll_back(&claim).err(),
        Some(InstallRefusal::NoPreviousVersion),
        "one activation records no previous, and rolling back to nothing is an outage, not a \
         rollback"
    );

    switch_active(&claim, &b.digest).expect("B activates");
    let rolled = roll_back(&claim).expect("rollback lands");
    assert_eq!(rolled.current, a.digest, "rollback returns to the previous");
    assert_eq!(
        rolled.previous.as_deref(),
        Some(b.digest.as_str()),
        "the rolled-away version stays recorded, so the flip is reversible rather than lossy"
    );
}

#[test]
fn installing_the_same_package_twice_is_idempotent() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    let staged = tempfile::tempdir().expect("a temp dir");
    let package = staged.path().join("a");
    copy_tree(Path::new(SHIPPED), &package);

    let first = install_package(&claim, &package).expect("the first install adopts");
    let second = install_package(&claim, &package)
        .expect("the second install of identical bytes must adopt idempotently, not refuse");
    assert_eq!(first, second, "one package, one adopted version");
}

/// A torn pointer write must not cost the active version: leftovers beside the pointer are
/// ignored, and the pointer itself is only ever replaced whole.
#[test]
fn a_torn_pointer_write_leaves_the_old_version_active() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");

    let staged = tempfile::tempdir().expect("a temp dir");
    let package = staged.path().join("a");
    copy_tree(Path::new(SHIPPED), &package);
    let installed = install_package(&claim, &package).expect("the package adopts");
    switch_active(&claim, &installed.digest).expect("the switch lands");

    // The wreckage a crash mid-publication leaves behind: a partial temporary BESIDE the pointer.
    std::fs::write(root.path().join("active.json.tmp-crashed"), b"{\"curr")
        .expect("write the wreckage");

    assert_eq!(
        active_versions(root.path())
            .expect("wreckage beside the pointer must not make reading fail")
            .expect("a version is active")
            .current,
        installed.digest,
        "a torn temporary beside the pointer changed what is active"
    );

    // A corrupt POINTER is different from wreckage beside it: it must refuse loudly, never read
    // as "nothing active" -- silent None here would tell an operator the machine is fresh.
    std::fs::write(root.path().join("active.json"), b"{\"curr").expect("corrupt the pointer");
    assert_eq!(
        active_versions(root.path()).err(),
        Some(InstallRefusal::CorruptPointer),
        "a corrupt pointer must refuse as corrupt, not read as an empty machine"
    );
}
