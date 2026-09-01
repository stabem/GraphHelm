//! The link-refusal matrix (#212): junctions and symlinks, in the package, in the adopted tree,
//! and in the layout's own ancestors -- plus the LF/CRLF cell, because the digest binds bytes.
//!
//! Three of these cells pay debts declared in earlier slices rather than discovered here: the
//! copier's link refusal shipped in #526 with its cells assigned to this file; the uninstall
//! junction cell shipped in #535 with its Unix twin assigned here; and the ancestor-as-link
//! limit was declared in #535's review (found by D) with the wiring assigned here.
//!
//! The fixture is SYNTHETIC and valid by construction (#589 is why: suites pinned to the shipped
//! package all went red at once when undeclared files landed in its tree). Identities come from
//! a marker folded into the skill body -- the digest binds the bytes, one marker is one identity.

use std::path::{Path, PathBuf};

use graphhelm_extension_host::{
    ActivationClaim, InstallRefusal, active_versions, install_package, switch_active,
    uninstall_version,
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn digest(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    format!("sha256:{}", hex::encode(sha2::Sha256::digest(bytes)))
}

fn write_declared(root: &Path, relative: &str, bytes: &[u8]) -> Value {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, bytes).unwrap();
    json!({
        "id": relative.replace(['/', '.'], "-"),
        "kind": "fixture",
        "path": relative,
        "sha256": digest(bytes),
        "effects": [],
        "permissions": [],
        "requires": {"capabilities": [], "observers": []}
    })
}

/// A package that validates by construction; `marker` decides its identity, and
/// `extra_declared` lets a cell add one declared file with chosen bytes (the CRLF cell).
fn synthetic_package(marker: &str, extra_declared: Option<(&str, &[u8])>) -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let root = directory.path().to_path_buf();

    let skill = format!(
        "---\nname: journey-contract\ndescription: Compile one user journey into observable \
         promises and failure contracts.\n---\n\n# Journey contract ({marker})\n\nRead with \
         `tool:status`, then validate locally with `cli:graph validate`.\n"
    );
    let skill = skill.as_bytes();
    std::fs::create_dir_all(root.join("skills/journey-contract")).unwrap();
    std::fs::write(root.join("skills/journey-contract/SKILL.md"), skill).unwrap();
    let skill_contribution = json!({
        "id": "journey-contract",
        "kind": "skill",
        "path": "skills/journey-contract/SKILL.md",
        "sha256": digest(skill),
        "surfaces": ["tool:status", "cli:graph validate"],
        "effects": ["runtime.connect", "runtime.read"],
        "permissions": ["network.loopback", "runtime.read"],
        "requires": {"capabilities": [], "observers": []},
        "family": "journey"
    });

    let claude = br#"{"name":"graphhelm-jpd-test","version":"1.0.0"}"#;
    let mut claude_contribution = write_declared(&root, ".claude-plugin/plugin.json", claude);
    claude_contribution["id"] = json!("claude-host");
    claude_contribution["kind"] = json!("host-adapter");

    let codex = br#"{"id":"graphhelm-jpd-test","name":"graphhelm-jpd-test","version":"1.0.0"}"#;
    let mut codex_contribution = write_declared(&root, ".codex-plugin/plugin.json", codex);
    codex_contribution["id"] = json!("codex-host");
    codex_contribution["kind"] = json!("host-adapter");

    let mcp = br#"{"mcpServers":{"graphhelm":{"command":"${GRAPHHELM_CLI}","args":["mcp","--url","http://127.0.0.1:8080","--token-file","${GRAPHHELM_TOKEN_FILE}","--actor","${GRAPHHELM_ACTOR}"]}}}"#;
    let mut mcp_contribution = write_declared(&root, ".mcp.json", mcp);
    mcp_contribution["id"] = json!("graphhelm-mcp-host");
    mcp_contribution["kind"] = json!("host-adapter");
    mcp_contribution["surfaces"] = json!(["cli:mcp"]);
    mcp_contribution["effects"] = json!(["runtime.connect"]);
    mcp_contribution["permissions"] = json!(["network.loopback", "token.reference.read"]);

    let mut contributions = vec![
        skill_contribution,
        claude_contribution,
        codex_contribution,
        mcp_contribution,
    ];
    if let Some((relative, bytes)) = extra_declared {
        contributions.push(write_declared(&root, relative, bytes));
    }

    let manifest = json!({
        "apiVersion": "p50.dev/v1",
        "kind": "Extension",
        "metadata": {
            "id": "graphhelm-jpd-test",
            "version": "1.0.0",
            "publisher": "graphhelm"
        },
        "spec": {
            "type": "skill-package",
            "capabilities": ["journey_proof"],
            "permissions": {
                "filesystem": {"package": "read", "workspaceArtifacts": "proposal-write"},
                "network": {"external": false, "loopbackRuntimeApi": true},
                "runtime": {"read": true, "mutations": [], "ownerConfirmationRequired": []},
                "secrets": {"artifactValues": false, "tokenFile": "reference-only"}
            },
            "contracts": {"contributions": contributions},
            "runtime": {"kind": "data", "isolationMinimum": "tier_0"},
            "compatibility": {"framework": ">=0.1"}
        }
    });
    std::fs::write(
        root.join("extension.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    (directory, root)
}

#[cfg(windows)]
fn junction(link: &Path, target: &Path) {
    let status = std::process::Command::new("cmd")
        .args([
            "/c",
            "mklink",
            "/J",
            link.to_str().expect("link path is unicode"),
            target.to_str().expect("target path is unicode"),
        ])
        .status()
        .expect("mklink runs");
    assert!(
        status.success(),
        "ARRANGEMENT: the junction was not created"
    );
}

/// Debt cell from #526 (Windows face): the copier's link refusal, declared there without a cell.
/// A junction inside the PACKAGE must refuse -- following it would smuggle bytes from outside
/// the package into the tree that later re-derivation trusts.
#[cfg(windows)]
#[test]
fn a_junction_inside_a_package_refuses_to_install() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("junction-package", None);

    let outside = tempfile::tempdir().expect("a temp dir");
    std::fs::write(outside.path().join("smuggled.txt"), b"outside bytes").unwrap();
    junction(&package.join("planted"), outside.path());
    assert!(
        package.join("planted").join("smuggled.txt").exists(),
        "CONTROL: the junction reaches outside bytes, or the refusal below proves nothing"
    );

    // The refusal layer is the VALIDATOR, measured by probe: GHEX004, "extension package
    // inventory cannot contain links or special files" -- it refuses the reparse point by name
    // and never follows it. install_package therefore answers Invalid, one layer before the
    // copier ever runs. The copier's own UnsafePackagePath stays behind it as depth: redundant
    // through this door while the validator holds, load-bearing in any future that reorders.
    assert_eq!(
        install_package(&claim, root.path(), &package).err(),
        Some(InstallRefusal::Invalid),
        "a package carrying a junction must refuse, and the validator is the layer that answers"
    );
    assert!(
        !root.path().join("versions").exists()
            || std::fs::read_dir(root.path().join("versions"))
                .map(|entries| {
                    entries
                        .filter_map(Result::ok)
                        .all(|e| !e.file_name().to_string_lossy().starts_with(".staging-"))
                })
                .unwrap_or(true),
        "the refusal must not leave an adopted tree behind"
    );
}

/// Debt cell from #526 (Unix face): the same refusal for a symlink. Runs in the Linux container.
#[cfg(unix)]
#[test]
fn a_symlink_inside_a_package_refuses_to_install() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("symlink-package", None);

    let outside = tempfile::tempdir().expect("a temp dir");
    std::fs::write(outside.path().join("smuggled.txt"), b"outside bytes").unwrap();
    std::os::unix::fs::symlink(outside.path(), package.join("planted"))
        .expect("ARRANGEMENT: the symlink was not created");
    assert!(
        package.join("planted").join("smuggled.txt").exists(),
        "CONTROL: the symlink reaches outside bytes, or the refusal below proves nothing"
    );

    // Same layer as the junction face: the validator's GHEX004 refuses links by name before
    // the copier runs, so install answers Invalid. The copier's refusal is the layer behind.
    assert_eq!(
        install_package(&claim, root.path(), &package).err(),
        Some(InstallRefusal::Invalid),
        "a package carrying a symlink must refuse, and the validator is the layer that answers"
    );
}

/// Debt cell from #535: the Unix twin of the uninstall junction cell. A symlink planted in an
/// ADOPTED tree after adoption must be deleted as a link, never traversed -- the user's files on
/// the other side survive. Runs in the Linux container.
#[cfg(unix)]
#[test]
fn uninstall_removes_a_planted_symlink_without_reaching_its_target() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("planted-symlink", None);
    let installed = install_package(&claim, root.path(), &package).expect("the package adopts");

    let user_directory = root.path().join("user-authored");
    std::fs::create_dir(&user_directory).expect("the user directory creates");
    let user_file = user_directory.join("keep-me.txt");
    std::fs::write(&user_file, b"authored by a person").expect("the user file writes");

    std::os::unix::fs::symlink(&user_directory, installed.root.join("planted-link"))
        .expect("ARRANGEMENT: the symlink was not created");
    assert!(
        installed
            .root
            .join("planted-link")
            .join("keep-me.txt")
            .exists(),
        "CONTROL: the symlink reaches the user file, or removing it proves nothing"
    );

    uninstall_version(&claim, root.path(), &installed.digest)
        .expect("a version with a planted symlink still uninstalls");

    assert!(!installed.root.exists(), "the adopted tree is gone");
    assert!(
        user_file.exists(),
        "UNINSTALL REACHED THROUGH THE SYMLINK: a user-authored file died with the version tree"
    );
}

/// D's ancestor limit (found reviewing #535), Windows face -- RED FIRST: the check does not
/// exist yet. The link refusal so far applies to the TARGET tree; `versions/` itself replaced by
/// a junction hands every layout operation a path outside the root. The lifecycle must refuse
/// the arrangement rather than operate through it.
#[cfg(windows)]
#[test]
fn a_versions_directory_that_is_a_junction_refuses_the_lifecycle() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("junction-ancestor", None);
    let installed = install_package(&claim, root.path(), &package).expect("the package adopts");
    switch_active(&claim, root.path(), &installed.digest).expect("the switch lands");

    // Replace versions/ with a junction to an elsewhere holding the same layout.
    let elsewhere = tempfile::tempdir().expect("a temp dir");
    let moved = elsewhere.path().join("versions");
    // Windows cannot rename a directory into a junction atomically; the hostile arrangement is
    // built directly: move the real tree away, plant the junction where it stood.
    std::fs::rename(root.path().join("versions"), &moved).expect("the real tree moves aside");
    junction(&root.path().join("versions"), &moved);
    assert!(
        root.path()
            .join("versions")
            .join(installed.root.file_name().unwrap())
            .is_dir(),
        "CONTROL: the junction serves the layout, or the refusal below proves nothing"
    );

    assert_eq!(
        switch_active(&claim, root.path(), &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "a layout whose ancestor is a junction must refuse by name, not operate through it"
    );
    assert_eq!(
        uninstall_version(&claim, root.path(), &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "uninstall through a junctioned ancestor is the same arrangement"
    );
}

/// D's ancestor limit, Unix face -- RED FIRST. Runs in the Linux container.
#[cfg(unix)]
#[test]
fn a_versions_directory_that_is_a_symlink_refuses_the_lifecycle() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("symlink-ancestor", None);
    let installed = install_package(&claim, root.path(), &package).expect("the package adopts");
    switch_active(&claim, root.path(), &installed.digest).expect("the switch lands");

    let elsewhere = tempfile::tempdir().expect("a temp dir");
    let moved = elsewhere.path().join("versions");
    std::fs::rename(root.path().join("versions"), &moved).expect("the real tree moves aside");
    std::os::unix::fs::symlink(&moved, root.path().join("versions"))
        .expect("ARRANGEMENT: the symlink was not created");

    assert_eq!(
        switch_active(&claim, root.path(), &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "a layout whose ancestor is a symlink must refuse by name, not operate through it"
    );
    assert_eq!(
        uninstall_version(&claim, root.path(), &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "uninstall through a symlinked ancestor is the same arrangement"
    );
}

/// The LF/CRLF conformance cell, platform-neutral: the digest binds BYTES, so a declared file
/// carrying CRLF line endings must round-trip the whole lifecycle unchanged on every platform.
/// What this pins is that no layer between validation and adoption "helpfully" normalizes line
/// endings -- the checkout's job is done before the package reaches the installer, and from
/// here on the bytes are the identity.
#[test]
fn a_declared_file_with_crlf_endings_survives_the_whole_lifecycle() {
    let root = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(root.path()).expect("the claim must be granted");
    let crlf_bytes: &[u8] = b"first line\r\nsecond line\r\n";
    let (_keep, package) = synthetic_package(
        "crlf-cell",
        // Under skills/: the inventory's TOP LEVEL is a closed vocabulary (GHEX012 names an
        // unknown entry by name, measured), so this cell must not fail on geography when its
        // subject is the bytes.
        Some(("skills/journey-contract/windows-notes.txt", crlf_bytes)),
    );

    let installed = install_package(&claim, root.path(), &package)
        .expect("a CRLF-bearing declared file is bytes like any other and must adopt");
    switch_active(&claim, root.path(), &installed.digest)
        .expect("the switch re-derives over the same bytes and must land");

    let adopted_copy = std::fs::read(
        installed
            .root
            .join("skills/journey-contract/windows-notes.txt"),
    )
    .expect("the declared file exists in the adopted tree");
    assert_eq!(
        adopted_copy, crlf_bytes,
        "a layer between validation and adoption normalized line endings: the digest binds \
         bytes, and these bytes are no longer the identity that was declared"
    );
    assert_eq!(
        active_versions(root.path())
            .expect("the pointer reads")
            .expect("a version is active")
            .current,
        installed.digest,
        "the CRLF package is the active one"
    );
}

/// A root passed with a TRAILING SEPARATOR must not blind the ancestor probe.
///
/// Found by G reviewing #596, and it defeats the check itself rather than the check-then-use
/// window this module already declares. POSIX resolves a pathname ending in `/` as a directory,
/// so `lstat("link/")` DEREFERENCES a final-component symlink and reports the target: the probe
/// reads "not a link" about the very link it was aimed at. `--root /some/path/` is an ordinary way
/// to type a directory and reaches this function straight off the CLI argument with no trim
/// anywhere between, which is what makes it reachable rather than theoretical.
///
/// `versions/` is NOT exposed the same way -- `install_root.join("versions")` normalizes the
/// separator away, so only the root itself is passed raw. The cell aims at the reachable half.
///
/// **MEASURED, and it decides how to read this cell: on Windows it passes WITH AND WITHOUT the
/// cure.** `GetFileAttributes` reports the reparse point for a trailing-separator name too, so the
/// separator never blinds the probe here. This cell is REDUNDANT on Windows and LOAD-BEARING on
/// POSIX, and it stays for the platform that needs it. **The red was never observed on this
/// machine** -- the platform where it is red is the one this lane could not run, so whoever runs
/// the Linux verification takes this cell's real first measurement. The cure's mechanism is pinned
/// platform-independently by `probe_path`'s unit guard in install.rs.
#[cfg(windows)]
#[test]
fn a_root_named_with_a_trailing_separator_still_refuses_a_linked_root() {
    let real = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(real.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("trailing-separator", None);
    let installed = install_package(&claim, real.path(), &package).expect("the package adopts");
    switch_active(&claim, real.path(), &installed.digest).expect("the switch lands");

    // The hostile arrangement: the caller names the root through a link to it.
    let holder = tempfile::tempdir().expect("a temp dir");
    let linked_root = holder.path().join("root-by-link");
    junction(&linked_root, real.path());
    assert!(
        linked_root.join("versions").is_dir(),
        "CONTROL: the link serves the layout, or the refusals below prove nothing"
    );

    // CONTROL, and it is the one that isolates the cause: named WITHOUT the trailing separator,
    // the probe already refuses. Any difference below is attributable to the separator alone.
    assert_eq!(
        switch_active(&claim, &linked_root, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "CONTROL: a linked root refuses when named without a trailing separator"
    );

    let mut trailing = linked_root.clone().into_os_string();
    trailing.push(std::path::MAIN_SEPARATOR.to_string());
    let trailing = std::path::PathBuf::from(trailing);

    assert_eq!(
        switch_active(&claim, &trailing, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "a trailing separator must not turn the ancestor probe into a probe of the link's TARGET"
    );
    assert_eq!(
        uninstall_version(&claim, &trailing, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "uninstall reaches the same probe and must answer the same"
    );
}

/// The POSIX face of the trailing-separator cell, and **the half that carries the weight**.
///
/// This is where G's P1 is real: POSIX resolves a pathname ending in `/` as a directory, so
/// `lstat("link/")` dereferences a final-component symlink and the probe reads "not a link" about
/// the link it was aimed at. The Windows twin above passes with and without the cure -- measured
/// -- so this cell is the one whose red was never observable on the machine that wrote it.
#[cfg(unix)]
#[test]
fn a_root_named_with_a_trailing_separator_still_refuses_a_linked_root() {
    let real = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(real.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("trailing-separator", None);
    let installed = install_package(&claim, real.path(), &package).expect("the package adopts");
    switch_active(&claim, real.path(), &installed.digest).expect("the switch lands");

    let holder = tempfile::tempdir().expect("a temp dir");
    let linked_root = holder.path().join("root-by-link");
    std::os::unix::fs::symlink(real.path(), &linked_root).expect("the symlink is created");
    assert!(
        linked_root.join("versions").is_dir(),
        "CONTROL: the link serves the layout, or the refusals below prove nothing"
    );

    // CONTROL that isolates the cause: named WITHOUT the trailing separator, the probe already
    // refuses. Any difference below is attributable to the separator alone.
    assert_eq!(
        switch_active(&claim, &linked_root, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "CONTROL: a linked root refuses when named without a trailing separator"
    );

    let mut trailing = linked_root.clone().into_os_string();
    trailing.push(std::path::MAIN_SEPARATOR.to_string());
    let trailing = std::path::PathBuf::from(trailing);

    assert_eq!(
        switch_active(&claim, &trailing, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "a trailing separator must not turn the ancestor probe into a probe of the link's TARGET"
    );
    assert_eq!(
        uninstall_version(&claim, &trailing, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "uninstall reaches the same probe and must answer the same"
    );
}
