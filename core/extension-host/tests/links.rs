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
    ActivationClaim, ClaimRefusal, InstallRefusal, active_versions, install_package, switch_active,
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
        install_package(&claim, &package).err(),
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
        install_package(&claim, &package).err(),
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
    let installed = install_package(&claim, &package).expect("the package adopts");

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

    uninstall_version(&claim, &installed.digest)
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
    let installed = install_package(&claim, &package).expect("the package adopts");
    switch_active(&claim, &installed.digest).expect("the switch lands");

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
        switch_active(&claim, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "a layout whose ancestor is a junction must refuse by name, not operate through it"
    );
    assert_eq!(
        uninstall_version(&claim, &installed.digest).err(),
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
    let installed = install_package(&claim, &package).expect("the package adopts");
    switch_active(&claim, &installed.digest).expect("the switch lands");

    let elsewhere = tempfile::tempdir().expect("a temp dir");
    let moved = elsewhere.path().join("versions");
    std::fs::rename(root.path().join("versions"), &moved).expect("the real tree moves aside");
    std::os::unix::fs::symlink(&moved, root.path().join("versions"))
        .expect("ARRANGEMENT: the symlink was not created");

    assert_eq!(
        switch_active(&claim, &installed.digest).err(),
        Some(InstallRefusal::UnsafeLayoutPath),
        "a layout whose ancestor is a symlink must refuse by name, not operate through it"
    );
    assert_eq!(
        uninstall_version(&claim, &installed.digest).err(),
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

    let installed = install_package(&claim, &package)
        .expect("a CRLF-bearing declared file is bytes like any other and must adopt");
    switch_active(&claim, &installed.digest)
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

/// A root named through a LINK, with or without a trailing separator, must be refused.
///
/// **The subject moved, and saying where it went is the point of this comment.** Until #546 these
/// assertions ran against `switch_active` and `uninstall_version`, which took a root argument
/// beside the claim; the hostile root was handed straight to them and `require_unlinked_layout`
/// was the layer that answered. #546 removed that parameter -- the four mutating entry points now
/// derive their root from `claim.install_root()` -- so a hostile root can no longer BE handed to
/// them, and the old form of this cell does not compile. The door the hostile root can still knock
/// on is `ActivationClaim::acquire`, and that is what these two cells now measure.
///
/// The refusal moved layer as well as subject: `ClaimRefusal::UnsafeClaimPath` from the claim
/// walk, not `InstallRefusal::UnsafeLayoutPath` from the layout probe. The mechanisms are not the
/// same and the difference is what makes the trailing separator uninteresting here. G's P1 (found
/// reviewing #596) was that POSIX resolves a pathname ending in `/` as a directory, so
/// `lstat("link/")` DEREFERENCES a final-component symlink and the probe reads "not a link" about
/// the link it was aimed at. `acquire` never lstats a path string: `open_root_anchor` walks the
/// path COMPONENT BY COMPONENT and opens each one with `O_NOFOLLOW` (POSIX) or
/// `FILE_FLAG_OPEN_REPARSE_POINT` plus a reparse-attribute check (Windows). `Path::components`
/// has already dropped the trailing separator before the first open, so the separator cannot
/// reach the probe to blind it -- by construction, not by a normalization step that could be
/// deleted.
///
/// **Which means these two cells pass with and without `probe_path`'s cure, on BOTH platforms**,
/// where the Windows face already did before #546 and the POSIX face carried the weight. That
/// makes them regression guards on `acquire`'s walk, and NOT a measurement of the trailing-
/// separator cure. The cure keeps its own subject: `probe_path`'s unit guard in `install.rs`,
/// which is red when the normalization is deleted. Two things are declared rather than left to be
/// re-derived from a green: the cure is now reached only by a caller that hands a raw root to
/// `require_unlinked_layout`, which the public API no longer lets anyone do; and the POSIX red
/// for the old form was never observed on the machine that wrote either version of this cell.
#[cfg(windows)]
#[test]
fn a_root_named_through_a_link_is_refused_with_or_without_a_trailing_separator() {
    let real = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(real.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("trailing-separator", None);
    let installed = install_package(&claim, &package).expect("the package adopts");
    switch_active(&claim, &installed.digest).expect("the switch lands");
    drop(claim);

    // The hostile arrangement: the caller names the root through a link to it.
    let holder = tempfile::tempdir().expect("a temp dir");
    let linked_root = holder.path().join("root-by-link");
    junction(&linked_root, real.path());
    assert!(
        linked_root.join("versions").is_dir(),
        "CONTROL: the link serves the layout, or the refusals below prove nothing"
    );

    // CONTROL that isolates the cause: named WITHOUT the trailing separator, the walk already
    // refuses. Any difference below is attributable to the separator alone.
    assert_eq!(
        ActivationClaim::acquire(&linked_root).err(),
        Some(ClaimRefusal::UnsafeClaimPath),
        "CONTROL: a linked root refuses when named without a trailing separator"
    );

    let mut trailing = linked_root.clone().into_os_string();
    trailing.push(std::path::MAIN_SEPARATOR.to_string());
    let trailing = PathBuf::from(trailing);

    assert_eq!(
        ActivationClaim::acquire(&trailing).err(),
        Some(ClaimRefusal::UnsafeClaimPath),
        "a trailing separator must not turn the claim walk into a walk of the link's TARGET"
    );
    assert!(
        installed.root.is_dir(),
        "the refused acquisitions must have left the real layout alone"
    );
}

/// The POSIX face of the cell above. Same subject, same reason, the platform's own link.
#[cfg(unix)]
#[test]
fn a_root_named_through_a_link_is_refused_with_or_without_a_trailing_separator() {
    let real = tempfile::tempdir().expect("a temp dir");
    let claim = ActivationClaim::acquire(real.path()).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("trailing-separator", None);
    let installed = install_package(&claim, &package).expect("the package adopts");
    switch_active(&claim, &installed.digest).expect("the switch lands");
    drop(claim);

    let holder = tempfile::tempdir().expect("a temp dir");
    let linked_root = holder.path().join("root-by-link");
    std::os::unix::fs::symlink(real.path(), &linked_root).expect("the symlink is created");
    assert!(
        linked_root.join("versions").is_dir(),
        "CONTROL: the link serves the layout, or the refusals below prove nothing"
    );

    assert_eq!(
        ActivationClaim::acquire(&linked_root).err(),
        Some(ClaimRefusal::UnsafeClaimPath),
        "CONTROL: a linked root refuses when named without a trailing separator"
    );

    let mut trailing = linked_root.clone().into_os_string();
    trailing.push(std::path::MAIN_SEPARATOR.to_string());
    let trailing = PathBuf::from(trailing);

    assert_eq!(
        ActivationClaim::acquire(&trailing).err(),
        Some(ClaimRefusal::UnsafeClaimPath),
        "a trailing separator must not turn the claim walk into a walk of the link's TARGET"
    );
    assert!(
        installed.root.is_dir(),
        "the refused acquisitions must have left the real layout alone"
    );
}

/// #772, the WINDOWS face: the swap this defect needs is refused by the platform while a claim is
/// held.
///
/// An open handle inside a tree pins every directory above it on Windows, so an ancestor cannot
/// be renamed away and replaced by a junction while the claim lives.
///
/// **This cell pins the OUTCOME, not any one field, and the distinction was measured after an
/// attribution of mine turned out to be wrong.** The first version of this comment credited
/// `RootAnchor::_ancestors` -- the retained ancestor handles, named with a leading underscore
/// because nothing reads them -- and said this cell would go red if they were deleted. It does
/// not. Dropping them at construction leaves every cell here green, because the pinning is
/// OVER-DETERMINED: measured on this host, the root's own directory handle alone refuses the
/// rename, and a file handle anywhere inside the root alone refuses it too.
///
/// So do not cite this cell as protecting `_ancestors`. Whether that field earns its place is a
/// question about the claim walk's own no-follow guarantee, and nothing here answers it.
///
/// The CONTROL is the same rename after the claim is dropped. Without it, a rename that failed
/// for any unrelated reason -- a stray handle, a scanner, a path typo -- would read as the
/// mitigation working.
#[cfg(windows)]
#[test]
fn a_live_claim_pins_the_ancestors_against_the_swap_this_defect_needs() {
    let ground = tempfile::tempdir().expect("a temp dir");
    let anchor = ground.path().join("anchor");
    std::fs::create_dir(&anchor).expect("the ancestor creates");
    let root = anchor.join("root");
    std::fs::create_dir(&root).expect("the root creates");

    let claim = ActivationClaim::acquire(&root).expect("the claim must be granted");
    let while_held = std::fs::rename(&anchor, ground.path().join("moved-while-held"));
    assert!(
        while_held.is_err(),
        "an ancestor was renamed out from under a live claim; the redirect this defect needs is reachable on this platform after all"
    );

    drop(claim);
    std::fs::rename(&anchor, ground.path().join("moved-after-release")).expect(
        "CONTROL: the rename must succeed once the claim is released, or the refusal above says nothing about the claim",
    );
}

/// The positive control for `root_still_anchored`: an ordinary layout must pass it, and the
/// lifecycle must still work.
///
/// A guard that refused every path would satisfy every "the far side survived" assertion in this
/// file. This is what makes those assertions mean something.
#[test]
fn an_unlinked_layout_is_still_anchored_and_still_uninstalls() {
    let ground = tempfile::tempdir().expect("a temp dir");
    let anchor = ground.path().join("anchor");
    std::fs::create_dir(&anchor).expect("the ancestor creates");
    let root = anchor.join("root");
    std::fs::create_dir(&root).expect("the root creates");

    let claim = ActivationClaim::acquire(&root).expect("the claim must be granted");
    claim
        .root_still_anchored()
        .expect("an ordinary layout must be anchored");

    let (_keep, package) = synthetic_package("anchored-ordinary", None);
    let installed = install_package(&claim, &package).expect("the package adopts");
    uninstall_version(&claim, &installed.digest).expect("an unlinked layout still uninstalls");
    assert!(
        !installed.root.exists(),
        "the adopted tree must be gone, or the anchor check is refusing everything"
    );
}

/// #772, the UNIX face -- the one that is actually reachable.
///
/// **NOT RUN in the lane that wrote it.** This repository's toolchain here is Windows and there is
/// no Rust toolchain on the WSL side, so this cell is type-checked against
/// `x86_64-unknown-linux-gnu` and has never been executed. It is written because the platform
/// premise underneath it WAS measured, in Python under WSL2 Ubuntu 6.6.87.2:
///
/// ```text
/// rename of an ancestor with an open descriptor inside : Ok (Windows: os error 32)
/// root/versions resolves through the planted symlink   : True
/// lstat(root).is_symlink() : False     <- the probe sees a directory
/// ```
///
/// The third line is the defect: the link is at an ANCESTOR, so the final component is an
/// ordinary directory and `symlink_metadata` reports one. The operation proceeds, on the far side.
///
/// Whoever runs this on a Linux host: it must be RED against a build without
/// `root_still_anchored`, or it proves only that the code does what its author just wrote.
#[cfg(unix)]
#[test]
fn an_ancestor_replaced_by_a_symlink_after_the_claim_refuses_the_lifecycle() {
    let ground = tempfile::tempdir().expect("a temp dir");
    let anchor = ground.path().join("anchor");
    std::fs::create_dir(&anchor).expect("the ancestor creates");
    let root = anchor.join("root");
    std::fs::create_dir(&root).expect("the root creates");

    let claim = ActivationClaim::acquire(&root).expect("the claim must be granted");
    let (_keep, package) = synthetic_package("ancestor-symlink-swap", None);
    let installed = install_package(&claim, &package).expect("the package adopts");
    let adopted_name = installed
        .root
        .file_name()
        .expect("the adopted tree has a name")
        .to_owned();

    // A tree the operator never named, shaped like the one they did, so every precondition the
    // removal checks is satisfied on the far side and it PROCEEDS rather than tripping something
    // unrelated. The defect's face is a success, not a refusal.
    let decoy = ground.path().join("decoy");
    let decoy_version = decoy.join("root").join("versions").join(&adopted_name);
    std::fs::create_dir_all(&decoy_version).expect("the decoy layout creates");
    std::fs::write(
        decoy_version.join("marker.txt"),
        b"outside the claimed root",
    )
    .expect("the decoy marker writes");

    std::fs::rename(&anchor, ground.path().join("anchor-real")).expect("the real tree moves aside");
    std::os::unix::fs::symlink(&decoy, &anchor).expect("ARRANGEMENT: the symlink was not created");
    assert!(
        root.join("versions").join(&adopted_name).is_dir(),
        "CONTROL: the layout must still resolve THROUGH the link, or the removal would fail for an ordinary reason and this cell would prove nothing"
    );

    let outcome = uninstall_version(&claim, &installed.digest);

    assert!(
        decoy_version.join("marker.txt").exists(),
        "THE REMOVAL FOLLOWED THE ANCESTOR LINK: a tree outside the claimed root was deleted"
    );
    assert!(
        matches!(outcome, Err(InstallRefusal::UnsafeLayoutPath)),
        "an ancestor that became a link after the claim must refuse, got {outcome:?}"
    );
}

/// The arrangement the four #772 entry-point cells share: an adopted layout, then an ancestor
/// ABOVE the root replaced by a symlink to a decoy shaped like it.
///
/// Extracted because the four differ only in the VERB they call afterwards. Writing it four times
/// would make a divergence between them look like a difference in the defect.
///
/// Returns the claim, the installed version, a second package ready to adopt, and the decoy's
/// marker path — the file that must still exist afterwards.
#[cfg(unix)]
fn layout_behind_a_swapped_ancestor(
    ground: &TempDir,
    label: &str,
) -> (
    ActivationClaim,
    graphhelm_extension_host::InstalledVersion,
    (TempDir, PathBuf),
    PathBuf,
) {
    let anchor = ground.path().join("anchor");
    std::fs::create_dir(&anchor).expect("the ancestor creates");
    let root = anchor.join("root");
    std::fs::create_dir(&root).expect("the root creates");

    let claim = ActivationClaim::acquire(&root).expect("the claim must be granted");
    let (keep_first, first) = synthetic_package(&format!("{label}-first"), None);
    let installed = install_package(&claim, &first).expect("the package adopts");
    drop(keep_first);
    let adopted_name = installed
        .root
        .file_name()
        .expect("the adopted tree has a name")
        .to_owned();

    let decoy = ground.path().join("decoy");
    let decoy_version = decoy.join("root").join("versions").join(&adopted_name);
    std::fs::create_dir_all(&decoy_version).expect("the decoy layout creates");
    let marker = decoy_version.join("marker.txt");
    std::fs::write(&marker, b"outside the claimed root").expect("the decoy marker writes");

    std::fs::rename(&anchor, ground.path().join("anchor-real")).expect("the real tree moves aside");
    std::os::unix::fs::symlink(&decoy, &anchor).expect("ARRANGEMENT: the symlink was not created");
    assert!(
        root.join("versions").join(&adopted_name).is_dir(),
        "CONTROL: the layout must still resolve THROUGH the link, or the verb would fail for an ordinary reason and the cell would prove nothing"
    );

    let second = synthetic_package(&format!("{label}-second"), None);
    (claim, installed, second, marker)
}

/// **THE FOUR ANCESTOR CELLS DO NOT PROVE THE SAME THING, and the count invites that reading.**
///
/// Measured by neutering `root_still_anchored` and reading WHICH assertion fires in each:
///
/// ```text
/// an_ancestor_replaced_by_a_symlink...  THE REMOVAL FOLLOWED THE ANCESTOR LINK:
///                                        a tree outside the claimed root was deleted
/// install_behind_a_swapped_ancestor      ... got Err(Invalid)
/// switch_behind_a_swapped_ancestor       ... got Err(Invalid)
/// roll_back_behind_a_swapped_ancestor    ... got Err(NoPreviousVersion)
/// ```
///
/// Only the first demonstrates DAMAGE. In the other three the verb, with the guard removed, still
/// refuses -- for an unrelated reason -- and the marker outside the claimed root survives: their
/// first assertion passes and the second does all the work. So `uninstall_version` is the
/// destructive face, and the other three are pinned on THE CAUSE THEY REPORT.
///
/// That is still a defect worth a cell. "Your package is invalid" is a lie about a redirected
/// root, and an operator who believes it edits a package that was never the problem. But it is one
/// destructive face and three misreported causes, not four destructive faces, and a reader seeing
/// four near-identical cells will otherwise assume four identical hazards.
///
/// Found by a peer who asked which assertion fires in each rather than taking "four red" as one
/// fact. Not answered here: whether a decoy shaped for `install` rather than for `uninstall` could
/// let an adoption actually land outside the root. This fixture is built for uninstall's lookup.
///
/// #772 blast radius, 1 of 3: `install_package` behind a swapped ancestor.
///
/// The cell that shipped with the fix exercises `uninstall_version` ALONE — `install_package`
/// appears in it only as arrangement, BEFORE the link is planted, so it never meets the redirected
/// ancestor. A peer measured the radius by neutering `root_still_anchored` and watching exactly
/// one cell fall.
///
/// The guard lives inside `require_unlinked_layout` and all four entry points call it, and the
/// `&ActivationClaim` signature makes forgetting it a compile error. That is a structural argument
/// and it is a good one. **It is still an argument.** These three cells make it a measurement.
#[cfg(unix)]
#[test]
fn install_behind_a_swapped_ancestor_refuses() {
    let ground = tempfile::tempdir().expect("a temp dir");
    let (claim, _installed, (_keep, second), marker) =
        layout_behind_a_swapped_ancestor(&ground, "install");

    let outcome = install_package(&claim, &second);

    assert!(
        marker.exists(),
        "THE ADOPTION FOLLOWED THE ANCESTOR LINK: it wrote into a tree outside the claimed root"
    );
    assert!(
        matches!(outcome, Err(InstallRefusal::UnsafeLayoutPath)),
        "install_package must refuse an ancestor that became a link after the claim, got {outcome:?}"
    );
}

/// #772 blast radius, 2 of 3: `switch_active` behind a swapped ancestor.
#[cfg(unix)]
#[test]
fn switch_behind_a_swapped_ancestor_refuses() {
    let ground = tempfile::tempdir().expect("a temp dir");
    let (claim, installed, _second, marker) = layout_behind_a_swapped_ancestor(&ground, "switch");

    let outcome = switch_active(&claim, &installed.digest);

    assert!(
        marker.exists(),
        "THE SWITCH FOLLOWED THE ANCESTOR LINK: it touched a tree outside the claimed root"
    );
    assert!(
        matches!(outcome, Err(InstallRefusal::UnsafeLayoutPath)),
        "switch_active must refuse an ancestor that became a link after the claim, got {outcome:?}"
    );
}

/// #772 blast radius, 3 of 3: `roll_back` behind a swapped ancestor.
///
/// This one would refuse for a second reason on a layout with nothing to roll back to, so the
/// assertion is on the CODE and not merely on failure — otherwise it would pass with the guard
/// removed, which is the vacuity these three exist to close.
#[cfg(unix)]
#[test]
fn roll_back_behind_a_swapped_ancestor_refuses() {
    let ground = tempfile::tempdir().expect("a temp dir");
    let (claim, _installed, _second, marker) =
        layout_behind_a_swapped_ancestor(&ground, "rollback");

    let outcome = graphhelm_extension_host::roll_back(&claim);

    assert!(
        marker.exists(),
        "THE ROLLBACK FOLLOWED THE ANCESTOR LINK: it touched a tree outside the claimed root"
    );
    assert!(
        matches!(outcome, Err(InstallRefusal::UnsafeLayoutPath)),
        "roll_back must refuse an ancestor that became a link after the claim, got {outcome:?}"
    );
}
