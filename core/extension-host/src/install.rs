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
/// spelling of it would drift, and the digest is validator-produced so anything that does not
/// match the expected shape is refused rather than escaped.
fn directory_for_digest(digest: &str) -> Result<String, InstallRefusal> {
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or(InstallRefusal::UnknownVersion)?;
    if hex.is_empty() || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(InstallRefusal::UnknownVersion);
    }
    Ok(format!("sha256-{hex}"))
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

fn read_pointer(install_root: &Path) -> Result<Option<ActiveVersions>, InstallRefusal> {
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
    _claim: &ActivationClaim,
    install_root: &Path,
    package: &Path,
) -> Result<InstalledVersion, InstallRefusal> {
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
    _claim: &ActivationClaim,
    install_root: &Path,
    digest: &str,
) -> Result<ActiveVersions, InstallRefusal> {
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
pub fn roll_back(
    _claim: &ActivationClaim,
    install_root: &Path,
) -> Result<ActiveVersions, InstallRefusal> {
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
