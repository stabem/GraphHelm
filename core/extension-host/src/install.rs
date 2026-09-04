//! Atomic version switch with rollback (#212).
//!
//! Layout, under the install root:
//!
//! ```text
//! versions/<digest-dir>/     immutable adopted package trees
//! versions/.staging-*        in-flight copies, same filesystem so adoption is one rename
//! active.json                the pointer: current digest + previous digest
//! ```
//!
//! The pointer is written whole to a temporary and renamed into place, so the flip that records
//! the new version and the flip that retires the old one are the SAME atomic act: no crash point
//! leaves the machine without an active version, which is the invariant `activation_steps()`
//! declares (record-then-retire, collapsed here so the two-recorded intermediate never exists on
//! disk at all). `std::fs::rename` replaces an existing FILE on both platforms, which is exactly
//! the property the pointer flip leans on.
//!
//! Every mutating entry point takes `&ActivationClaim`: authority is the claim, at compile level,
//! exactly as `ActivationRecord::activate` already requires it.
//!
//! And the claim names the ROOT, rather than sitting beside a root argument that could disagree
//! with it (#546). Possessing a claim proves the caller acquired the lock; it did not prove the
//! caller acquired the lock over the tree it is about to change, and `uninstall_version` is a
//! `remove_dir_all`. The parameter that could disagree is gone instead of checked, so the
//! mismatch has no spelling -- `claim.install_root()` is the only root these functions can reach.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::activation::ActivationClaim;

const POINTER_NAME: &str = "active.json";
const POINTER_VERSION: u8 = 1;

/// Why an install, switch or rollback was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallRefusal {
    /// The package (or the adopted tree at switch time) does not validate.
    Invalid,
    /// The tree's re-derived digest no longer matches the digest it was adopted under.
    AdoptedBytesChanged,
    /// The requested digest was never adopted under this root.
    UnknownVersion,
    /// Rollback was requested and no previous version is recorded.
    NoPreviousVersion,
    /// The pointer file exists but cannot be read as a pointer.
    CorruptPointer,
    /// The package tree carries a symbolic link or another entry the copier refuses to follow.
    UnsafePackagePath,
    /// The pointer still names this version as current or previous, so removing it would cost
    /// the machine its active version or rollback its target.
    VersionRetained,
    /// The layout's own ancestors -- the install root or versions/ -- are a link, so every
    /// path-based operation would act outside the root the caller named.
    UnsafeLayoutPath,
    /// The layout could not be written at all.
    Unwritable,
}

/// A package adopted under `versions/`, and the digest it was adopted as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledVersion {
    pub digest: String,
    pub root: PathBuf,
}

/// What the pointer says: the active digest, and the previous one when there is any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveVersions {
    pub current: String,
    pub previous: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersistedPointer {
    version: u8,
    current: String,
    previous: Option<String>,
}

/// The directory a digest is adopted under.
///
/// Digests read `sha256:<hex>`, and `:` is not a legal filename byte on Windows, so the adopted
/// directory spells the same identity with `-`. The mapping lives HERE and nowhere else: a second
/// spelling of it would drift, and every consumer -- install, switch, rollback, uninstall --
/// closes or opens together with this function.
///
/// **The shape is exact: `sha256:` + 64 lowercase hex, one spelling per identity.** The first
/// version checked only the ALPHABET (any casing, any length), and that was a P1 found by D on
/// #535: retention compares exact strings while the filesystem lookup on Windows is
/// case-insensitive, so `sha256:1ABB...` walked past the retention check as a stranger and then
/// FOUND the current version's directory -- one shouted digit uninstalled the active version.
/// The search accepted an identity retention did not recognize; casing was an instance of the
/// gap, not the gap. Canonical-or-refused is what makes the two sides agree by construction.
pub(crate) fn directory_for_digest(digest: &str) -> Result<String, InstallRefusal> {
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or(InstallRefusal::UnknownVersion)?;
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(InstallRefusal::UnknownVersion);
    }
    Ok(format!("sha256-{hex}"))
}

/// Refuse a layout whose own ANCESTORS are links: the install root, and `versions/` under it.
///
/// The link refusals elsewhere in this crate apply to the target tree; this one covers the path
/// that every layout operation walks to REACH a target. `versions/` replaced by a junction or a
/// symlink hands each `install_root.join(...)` a location outside the root the caller named,
/// and every subsequent guarantee -- re-derivation, retention, the pointer flip -- then holds
/// about the wrong place. (Ancestor limit found by D reviewing #535; the wiring is this check.)
///
/// On Windows the probe is the REPARSE attribute, not `is_symlink`: junctions are mount-point
/// reparse points and a symlink-only probe can miss them, which is precisely the face the cells
/// measure. `NotFound` passes -- an absent `versions/` is a fresh root, and absence is the root
/// assertions' vocabulary elsewhere, not a link.
///
/// DECLARED LIMIT 1: check-then-use. The gap between this check and the operation that follows is
/// real, and the full cure is the anchored-handle discipline the activation walk implements;
/// this refusal raises the bar for the arrangements a lifecycle actually meets, and says so
/// rather than claiming the discipline it does not have.
///
/// DECLARED LIMIT 2, and it is a DIFFERENT one: **this probes two paths, not every component of
/// them.** `symlink_metadata` does not follow the FINAL component, but it does resolve every
/// intermediate one -- so a symlink at an ancestor ABOVE `install_root` is followed silently, both
/// probes report ordinary directories, and the lifecycle operates on the redirected tree.
/// `uninstall_version` can then remove a matching version outside the claimed root. (Found by
/// Codex reviewing #596.)
///
/// This is NOT limit 1 wearing another face, and saying so matters: limit 1 is a RACE, and the
/// hostile link here can already exist when the check runs. A gap must not shelter under a
/// declaration written for a different gap.
///
/// **Why the obvious fix is not applied here.** Walking every ancestor and refusing any link among
/// them would refuse legitimate layouts on any system where a parent is a symlink -- macOS resolves
/// `/tmp` to `/private/tmp`, which is where temporary install roots live, so the naive walk turns
/// a security probe into a refusal of ordinary use. (Reasoned, not measured: this lane has no macOS
/// host.) The cure that actually holds is the anchored-handle discipline -- resolve the root once
/// and perform every operation through that handle -- which is a larger change than the one this
/// module makes, and is named here so the next person does not re-derive it from the symptom.
/// The path the ancestor probe is allowed to see: the same location, named as a FINAL COMPONENT.
///
/// Purely lexical -- it drops a trailing separator (and redundant `.` components) without
/// resolving `..` and without following any link, which is the only normalization safe to run
/// BEFORE a security probe. A path that is nothing but a root survives as itself, and a root
/// cannot be a symlink.
///
/// It is a NAMED function so the guard pinning this behaviour has THIS code as its subject.
/// Asserting the same property against `Path::components` directly would have been a test of the
/// standard library: green with the normalization deleted from the probe, which is exactly the
/// shape of a guard that certifies nothing.
fn probe_path(path: &Path) -> &Path {
    path.components().as_path()
}

pub(crate) fn require_unlinked_layout(claim: &ActivationClaim) -> Result<(), InstallRefusal> {
    // FIRST, because the two probes below read the path and a redirected path answers about
    // somewhere else (#772). The claim proved the ancestors were real directories when it was
    // taken; this proves the name still reaches the directory it proved. Taking the claim rather
    // than a bare `&Path` is deliberate: every one of the four entry points already holds one,
    // and a signature that cannot express the check is a check that gets forgotten at a call
    // site.
    claim
        .root_still_anchored()
        .map_err(|_| InstallRefusal::UnsafeLayoutPath)?;
    let install_root = claim.install_root();
    fn is_link(path: &Path) -> Result<bool, InstallRefusal> {
        // The probe must see the path as a FINAL COMPONENT, never as a trailing-separator
        // directory reference. POSIX resolves a pathname ending in `/` as a directory, so
        // `lstat("link/")` DEREFERENCES a final-component symlink and answers about its target:
        // the probe would read "not a link" about the very link it was aimed at, and
        // `--root /some/path/` reaches here straight off a CLI argument with no trim in between.
        // (Found by G reviewing #596. This defeats the CHECK, and is not the check-then-use
        // window declared above.)
        //
        // `components().as_path()` drops the trailing separator without resolving `..` or
        // following any link -- it is a purely lexical normalization, which is the only kind
        // safe to run before a security probe. A path that is nothing but a root survives as
        // itself, and a root cannot be a symlink.
        //
        // DECLARED LIMIT: Rust does not normalize Windows VERBATIM paths (`\\?\C:\...`), by
        // design, so one named that way keeps whatever separator it was given.
        let path = probe_path(path);
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(_) => return Err(InstallRefusal::Unwritable),
        };
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
            Ok(metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        }
        #[cfg(not(windows))]
        {
            Ok(metadata.file_type().is_symlink())
        }
    }
    // NEITHER HALF IS DEAD, and the first one now LOOKS dead, which is worse than looking
    // redundant (found by a peer reviewing #546).
    //
    // Since #546 the only root that reaches here is `claim.install_root()`, and the claim walk
    // opened every component of it without following a link. The obvious reading is therefore
    // "the anchor already proved this root is not a link, delete the probe" -- and that reading is
    // wrong. The anchor proves it AT ACQUIRE TIME. An open directory handle does not stop the
    // directory being renamed away and replaced by a link afterwards, and all four entry points
    // reach the tree by PATH (`install_root.join("versions")`) rather than through the retained
    // descriptor. This probe is the only thing covering the acquire-to-use window.
    //
    // The `versions/` half is not covered by the anchor at all: the claim walk stops at the root,
    // and `versions/` may not even exist when the claim is taken.
    if is_link(install_root)? || is_link(&install_root.join("versions"))? {
        return Err(InstallRefusal::UnsafeLayoutPath);
    }
    Ok(())
}

/// Copy a package tree, refusing to follow anything that is not a plain file or directory.
///
/// `std::fs::copy` follows symlinks, so a package carrying one would smuggle bytes from OUTSIDE
/// the package into the adopted tree -- and the adopted tree is what later re-derivation trusts.
/// `DirEntry::file_type` does not follow, which is the property this refusal leans on.
fn copy_tree(from: &Path, to: &Path) -> Result<(), InstallRefusal> {
    std::fs::create_dir_all(to).map_err(|_| InstallRefusal::Unwritable)?;
    for entry in std::fs::read_dir(from).map_err(|_| InstallRefusal::Unwritable)? {
        let entry = entry.map_err(|_| InstallRefusal::Unwritable)?;
        let kind = entry.file_type().map_err(|_| InstallRefusal::Unwritable)?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target).map_err(|_| InstallRefusal::Unwritable)?;
        } else {
            return Err(InstallRefusal::UnsafePackagePath);
        }
    }
    Ok(())
}

pub(crate) fn read_pointer(install_root: &Path) -> Result<Option<ActiveVersions>, InstallRefusal> {
    let bytes = match std::fs::read(install_root.join(POINTER_NAME)) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(InstallRefusal::Unwritable),
    };
    let persisted: PersistedPointer =
        serde_json::from_slice(&bytes).map_err(|_| InstallRefusal::CorruptPointer)?;
    if persisted.version != POINTER_VERSION {
        return Err(InstallRefusal::CorruptPointer);
    }
    Ok(Some(ActiveVersions {
        current: persisted.current,
        previous: persisted.previous,
    }))
}

fn write_pointer(install_root: &Path, pointer: &ActiveVersions) -> Result<(), InstallRefusal> {
    let persisted = PersistedPointer {
        version: POINTER_VERSION,
        current: pointer.current.clone(),
        previous: pointer.previous.clone(),
    };
    let bytes = serde_json::to_vec(&persisted).map_err(|_| InstallRefusal::Unwritable)?;
    let temporary = install_root.join(format!("{POINTER_NAME}.tmp-{}", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, &bytes).map_err(|_| InstallRefusal::Unwritable)?;
    if std::fs::rename(&temporary, install_root.join(POINTER_NAME)).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(InstallRefusal::Unwritable);
    }
    Ok(())
}

/// Validate the tree at `path` and require it to re-derive as `digest`, at this instant.
///
/// The recorded digest -- whether in a directory name or a pointer -- is a memory of the past;
/// only re-derivation speaks about the bytes that are about to be used. One oracle, asked again,
/// exactly as `activate_staged` does it.
fn require_digest(path: &Path, digest: &str) -> Result<(), InstallRefusal> {
    let validated =
        graphhelm_schema::validate_extension_package(path).map_err(|_| InstallRefusal::Invalid)?;
    if validated.package_digest != digest {
        return Err(InstallRefusal::AdoptedBytesChanged);
    }
    Ok(())
}

/// Stage, verify and adopt a package under `versions/`. Does NOT switch.
///
/// # Errors
///
/// Returns [`InstallRefusal`] when the package does not validate, carries entries the copier
/// refuses to follow, or the layout is unwritable.
pub fn install_package(
    claim: &ActivationClaim,
    package: &Path,
) -> Result<InstalledVersion, InstallRefusal> {
    let install_root = claim.install_root();
    require_unlinked_layout(claim)?;
    let validated = graphhelm_schema::validate_extension_package(package)
        .map_err(|_| InstallRefusal::Invalid)?;
    let digest = validated.package_digest;
    let directory = directory_for_digest(&digest)?;
    let versions = install_root.join("versions");
    std::fs::create_dir_all(&versions).map_err(|_| InstallRefusal::Unwritable)?;
    let adopted = versions.join(&directory);
    if adopted.exists() {
        // Idempotent by identity: the digest names the bytes, so a second install of the same
        // bytes has nothing to do. Whether the adopted tree still MATCHES its digest is decided
        // where it matters -- at switch time, by re-derivation -- not optimistically here.
        return Ok(InstalledVersion {
            digest,
            root: adopted,
        });
    }

    let staging = versions.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    let staged = copy_tree(package, &staging).and_then(|()| {
        // Re-derive from the STAGED bytes: the source may have moved while it was being copied,
        // and the adopted directory's name is a claim about the bytes inside it.
        require_digest(&staging, &digest)
    });
    if let Err(refusal) = staged {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(refusal);
    }

    match std::fs::rename(&staging, &adopted) {
        Ok(()) => {}
        Err(_) if adopted.exists() => {
            // A concurrent install of the same digest won the rename. Same identity, same
            // outcome: idempotent.
            let _ = std::fs::remove_dir_all(&staging);
        }
        Err(_) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(InstallRefusal::Unwritable);
        }
    }
    Ok(InstalledVersion {
        digest,
        root: adopted,
    })
}

/// Atomically make an adopted version the active one, retiring the current to `previous`.
///
/// The target tree's digest is re-derived before the flip, and any refusal leaves the pointer
/// untouched: failed activation leaves the previous version active.
///
/// # Errors
///
/// Returns [`InstallRefusal`] when the target is unknown, no longer matches its adopted digest,
/// or the pointer cannot be written.
pub fn switch_active(
    claim: &ActivationClaim,
    digest: &str,
) -> Result<ActiveVersions, InstallRefusal> {
    let install_root = claim.install_root();
    require_unlinked_layout(claim)?;
    let directory = directory_for_digest(digest)?;
    let adopted = install_root.join("versions").join(directory);
    if !adopted.is_dir() {
        return Err(InstallRefusal::UnknownVersion);
    }
    require_digest(&adopted, digest)?;

    let pointer = read_pointer(install_root)?;
    if let Some(active) = &pointer
        && active.current == digest
    {
        // Switching to what is already active moves nothing, and clobbering `previous` with
        // the current version would cost rollback its target for no state change.
        return Ok(active.clone());
    }
    let next = ActiveVersions {
        current: digest.to_owned(),
        previous: pointer.map(|active| active.current),
    };
    write_pointer(install_root, &next)?;
    Ok(next)
}

/// Atomically return to the previous known-good version.
///
/// The rolled-away version stays recorded as the new `previous`, so the flip is reversible
/// rather than lossy. The target is re-derived like any other switch: a rollback to a tree
/// whose bytes moved is not a rollback to known-good.
///
/// # Errors
///
/// Returns [`InstallRefusal::NoPreviousVersion`] when the pointer records none.
pub fn roll_back(claim: &ActivationClaim) -> Result<ActiveVersions, InstallRefusal> {
    let install_root = claim.install_root();
    require_unlinked_layout(claim)?;
    let Some(active) = read_pointer(install_root)? else {
        return Err(InstallRefusal::NoPreviousVersion);
    };
    let Some(previous) = active.previous else {
        return Err(InstallRefusal::NoPreviousVersion);
    };
    let directory = directory_for_digest(&previous)?;
    let adopted = install_root.join("versions").join(directory);
    if !adopted.is_dir() {
        return Err(InstallRefusal::UnknownVersion);
    }
    require_digest(&adopted, &previous)?;

    let next = ActiveVersions {
        current: previous,
        previous: Some(active.current),
    };
    write_pointer(install_root, &next)?;
    Ok(next)
}

/// Read the pointer. `Ok(None)` when no version was ever activated. Requires no claim: reading
/// grants nothing.
///
/// # Errors
///
/// Returns [`InstallRefusal::CorruptPointer`] when a pointer exists but cannot be read as one.
pub fn active_versions(install_root: &Path) -> Result<Option<ActiveVersions>, InstallRefusal> {
    read_pointer(install_root)
}

#[cfg(test)]
mod probe_path_tests {
    use super::probe_path;

    /// The cure's mechanism for G's P1, decidable on EVERY platform.
    ///
    /// The behavioural cell in `tests/links.rs` is redundant on Windows -- measured -- because the
    /// OS does not dereference a trailing-separator name there. This one does not depend on OS
    /// path semantics at all: it pins that the production probe normalizes before it looks.
    /// Delete the body of `probe_path` and this goes red everywhere.
    #[test]
    fn a_trailing_separator_is_dropped_before_the_probe_looks() {
        let mut trailing = std::path::PathBuf::from("root").into_os_string();
        trailing.push(std::path::MAIN_SEPARATOR.to_string());
        let trailing = std::path::PathBuf::from(trailing);

        assert!(
            trailing
                .as_os_str()
                .to_string_lossy()
                .ends_with(std::path::MAIN_SEPARATOR),
            "CONTROL: the arrangement must actually carry a trailing separator"
        );
        assert!(
            !probe_path(&trailing)
                .as_os_str()
                .to_string_lossy()
                .ends_with(std::path::MAIN_SEPARATOR),
            "the ancestor probe must be handed a final component, never a directory reference"
        );
    }
}
