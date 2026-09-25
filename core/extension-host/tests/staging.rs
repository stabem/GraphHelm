//! Verify-then-activate over a staged copy (#212), G4.
//!
//! The fixture is the repository's own shipped package, copied into a temp directory. Using the
//! real package means the digest under test is the one `validate_extension_package` produces --
//! this task does not compute a second package digest, because a second digest is a second ORACLE
//! and two oracles diverge in silence.

use graphhelm_extension_host::{ActivationClaim, StagingRefusal, activate_staged, verify_staged};

const SHIPPED: &str = "../../extensions/builtin/graphhelm-development-contracts";

fn sha256_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("create the staging directory");
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

/// The production change this catches: activating on the digest RECORDED at verification without
/// re-deriving it from the staged bytes. Verify-then-use over a mutable path is a TOCTOU by
/// construction, and "we verified it" is worthless if the verified bytes are not the used bytes.
#[test]
fn activation_refuses_a_staged_copy_whose_bytes_changed_after_verification() {
    let staging = tempfile::tempdir().expect("a temp dir");
    let package = staging.path().join("package");
    copy_tree(std::path::Path::new(SHIPPED), &package);

    let verified = verify_staged(&package)
        .expect("HARNESS-BROKE: the repository's own shipped package failed verification");

    // Landmark: untouched bytes DO activate. Without this the refusal below could be an activation
    // path that refuses everything, which is safe and useless.
    let first_claim = ActivationClaim::acquire(staging.path()).expect("the claim must be granted");
    activate_staged(&first_claim, &verified, &package)
        .expect("HARNESS-BROKE: an untouched staged copy did not activate");
    drop(first_claim);

    // The substitution is a package that STILL VALIDATES. An attacker who can write into staging
    // can write a well-formed package, so the interesting swap is not a corrupt one -- the
    // validator already refuses those, and a fixture that leans on it tests the validator rather
    // than this guard. Here the file changes AND its declared digest is updated to match, so the
    // package stays internally consistent and only its IDENTITY has moved.
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
        "HARNESS-BROKE: the manifest does not declare the digest this test replaces, so the \
         substitute would not validate and the refusal would come from the wrong check"
    );
    std::fs::write(&manifest_path, manifest.replace(&before, &after)).expect("write the manifest");

    // The substitute is well-formed on its own terms.
    verify_staged(&package).expect(
        "HARNESS-BROKE: the substituted package does not validate, so this fixture exercises the \
         validator rather than the verified-bytes check",
    );

    let second_claim = ActivationClaim::acquire(staging.path()).expect("the claim must be granted");
    let refusal = activate_staged(&second_claim, &verified, &package)
        .expect_err("activation accepted a staged copy whose bytes changed after verification");
    assert_eq!(refusal, StagingRefusal::StagedBytesChanged);
}
