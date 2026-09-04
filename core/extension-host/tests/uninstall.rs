//! Safe uninstall (#212): exactly one adopted tree goes, the pointer's two named versions are
//! untouchable, and nothing a user authored is ever reached.
//!
//! Fixture idiom as in `tests/install.rs` and `tests/staging.rs`: the repository's own shipped
//! package, identity moved by mutating a declared file AND its declared digest, so every refusal
//! below comes from the guard under test and never from the validator.

use std::path::Path;

use graphhelm_extension_host::{
    ActivationClaim, InstallRefusal, active_versions, install_package, switch_active,
    uninstall_version,
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
        "HARNESS-BROKE: the manifest does not declare the digest this fixture replaces"
    );
    std::fs::write(&manifest_path, manifest.replace(&before, &after)).expect("write the manifest");
}

/// Three distinct adopted versions, current = B, previous = A, C free.
fn three_versions(
    claim: &ActivationClaim,
    staged: &Path,
) -> (
    graphhelm_extension_host::InstalledVersion,
    graphhelm_extension_host::InstalledVersion,
    graphhelm_extension_host::InstalledVersion,
) {
    let package_a = staged.join("a");
    copy_tree(Path::new(SHIPPED), &package_a);
    let package_b = staged.join("b");
    copy_tree(Path::new(SHIPPED), &package_b);
    move_identity(&package_b);
    let package_c = staged.join("c");
    copy_tree(Path::new(SHIPPED), &package_c);
    move_identity(&package_c);
    move_identity(&package_c);

    let a = install_package(claim, &package_a).expect("A adopts");
    let b = install_package(claim, &package_b).expect("B adopts");
    let c = install_package(claim, &package_c).expect("C adopts");
    assert!(
        a.digest != b.digest && b.digest != c.digest && a.digest != c.digest,
        "HARNESS-BROKE: the three fixtures do not have three identities"
    );
    switch_active(claim, &a.digest).expect("A activates");
    switch_active(claim, &b.digest).expect("B activates");
    (a, b, c)
}

#[test]
fn a_version_that_is_neither_current_nor_previous_uninstalls_and_only_it() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let staged = tempfile::tempdir().expect("a temp dir");
    let (a, b, c) = three_versions(&claim, staged.path());

    // Trees are located by the roots the INSTALLER returned -- the digest-to-directory mapping
    // has one home in install.rs, and a re-spelling here would be the drift its comment warns of.
    assert!(c.root.is_dir(), "CONTROL: C's tree exists before");

    uninstall_version(&claim, &c.digest).expect("the free version uninstalls");

    assert!(!c.root.exists(), "C's tree is gone");
    assert!(a.root.is_dir(), "A (previous) is untouched");
    assert!(b.root.is_dir(), "B (current) is untouched");
    let pointer = active_versions(root.path())
        .expect("the pointer reads")
        .expect("a version is active");
    assert_eq!(
        (pointer.current.as_str(), pointer.previous.as_deref()),
        (b.digest.as_str(), Some(a.digest.as_str())),
        "uninstall of a free version must not move the pointer"
    );
}

#[test]
fn the_current_and_the_previous_version_refuse_to_uninstall() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let staged = tempfile::tempdir().expect("a temp dir");
    let (a, b, _c) = three_versions(&claim, staged.path());

    assert_eq!(
        uninstall_version(&claim, &b.digest).err(),
        Some(InstallRefusal::VersionRetained),
        "removing the CURRENT version is an outage, not an uninstall"
    );
    assert!(
        b.root.is_dir(),
        "the refusal must not have half-removed the tree"
    );

    assert_eq!(
        uninstall_version(&claim, &a.digest).err(),
        Some(InstallRefusal::VersionRetained),
        "removing the PREVIOUS version costs rollback its target"
    );
    assert!(
        a.root.is_dir(),
        "the refusal must not have half-removed the tree"
    );
}

#[test]
fn a_digest_that_was_never_adopted_refuses_as_unknown() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    // Full-length and well-formed, so this cell exercises "not adopted" and not the validator.
    let absent = format!("sha256:{}", "0".repeat(64));
    assert_eq!(
        uninstall_version(&claim, &absent).err(),
        Some(InstallRefusal::UnknownVersion),
        "uninstalling what was never adopted must say so, not succeed vacuously"
    );
}

/// D's blocking finding on this PR, his probe as the fixture: a digest whose hex is SHOUTED
/// bypasses retention and deletes the CURRENT version's tree.
///
/// The framing that matters is his: this is not "case-sensitive comparison" as a style nit. The
/// validator admitted any casing and any LENGTH -- it checked the alphabet, not the identity --
/// while retention compares exact strings. The lookup (a case-insensitive filesystem) therefore
/// accepted an identity retention did not recognize: `sha256:1ABB...` walked past the retention
/// check as a stranger and then FOUND the current version's directory. Case is an instance of
/// the gap, not the gap.
///
/// The cure lives in the validator -- exactly `sha256:` + 64 lowercase hex -- so every consumer
/// of the mapping (install, switch, rollback, uninstall) closes at once.
#[test]
fn a_shouted_spelling_of_the_current_digest_cannot_reach_its_tree() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let staged = tempfile::tempdir().expect("a temp dir");
    let (_a, b, _c) = three_versions(&claim, staged.path());

    let shouted = {
        let hex = b
            .digest
            .strip_prefix("sha256:")
            .expect("HARNESS-BROKE: the validator's digest lost its prefix");
        format!("sha256:{}", hex.to_uppercase())
    };
    assert_ne!(
        shouted, b.digest,
        "HARNESS-BROKE: shouting changed nothing, so this cell cannot distinguish spellings"
    );

    assert_eq!(
        uninstall_version(&claim, &shouted).err(),
        Some(InstallRefusal::UnknownVersion),
        "a non-canonical spelling must be refused by the validator, not resolved by the \
         filesystem"
    );
    assert!(
        b.root.is_dir(),
        "THE CURRENT VERSION'S TREE IS GONE: the lookup accepted an identity retention did not \
         recognize"
    );

    // The same gap's other face: a well-formed alphabet at the wrong LENGTH.
    assert_eq!(
        uninstall_version(&claim, "sha256:00").err(),
        Some(InstallRefusal::UnknownVersion),
        "a short digest is not an identity, and the validator is where that is decided"
    );
}

/// The deliverable's own sentence, measured: a link planted inside an adopted tree AFTER adoption
/// must not let uninstall reach through it. The removal leans on `remove_dir_all` deleting a
/// link itself rather than traversing it -- this cell MEASURES that property instead of trusting
/// the documentation, because the user's artifacts are what pay if it drifts.
///
/// Windows: a junction, created without privilege via `mklink /J`. The adopted tree was copied by
/// a copier that refuses links, so a link inside it is always a LATER plant -- which is exactly
/// the arrangement an uninstall meets on a machine where something else has been at the tree.
#[cfg(windows)]
#[test]
fn uninstall_removes_a_planted_junction_without_reaching_its_target() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let staged = tempfile::tempdir().expect("a temp dir");
    let package = staged.path().join("a");
    copy_tree(Path::new(SHIPPED), &package);
    let installed = install_package(&claim, &package).expect("the package adopts");
    // No switch: the pointer names nothing, so nothing is retained and the uninstall is legal.

    let user_directory = root.path().join("user-authored");
    std::fs::create_dir(&user_directory).expect("the user directory creates");
    let user_file = user_directory.join("keep-me.txt");
    std::fs::write(&user_file, b"authored by a person").expect("the user file writes");

    let junction = installed.root.join("planted-junction");
    let status = std::process::Command::new("cmd")
        .args([
            "/c",
            "mklink",
            "/J",
            junction.to_str().expect("junction path is unicode"),
            user_directory.to_str().expect("user path is unicode"),
        ])
        .status()
        .expect("mklink runs");
    assert!(
        status.success(),
        "ARRANGEMENT: the junction was not created"
    );
    assert!(
        junction.join("keep-me.txt").exists(),
        "CONTROL: the junction reaches the user file, or removing it proves nothing"
    );

    uninstall_version(&claim, &installed.digest)
        .expect("a version with a planted junction still uninstalls");

    assert!(!installed.root.exists(), "the adopted tree is gone");
    assert!(
        user_file.exists(),
        "UNINSTALL REACHED THROUGH THE JUNCTION: a user-authored file died with the version tree"
    );
    let content = std::fs::read(&user_file).expect("the user file still reads");
    assert_eq!(
        content, b"authored by a person",
        "the user file survived in name but not in content"
    );
}
