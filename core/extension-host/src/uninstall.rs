//! Safe uninstall that never removes user-authored artifacts (#212).
//!
//! Uninstall removes exactly one adopted tree under `versions/`, and refuses the two versions
//! the pointer still names: removing the current one is an outage, and removing the previous one
//! costs rollback its target.
//!
//! What the digest CANNOT do is spell an escaping path: the mapping admits exactly one canonical
//! form, and it is `install.rs`'s own, composed rather than re-spelled. **What that does NOT
//! cover, declared plainly because the first version of this comment overclaimed it: the
//! ancestors.** The link refusal applies to the TARGET tree -- a planted link inside it is
//! deleted, never traversed -- but if `versions/` itself (or the install root above it) has been
//! replaced by a link, this path-based removal reaches through the ancestor like any other
//! `std::fs` call. Defending ancestors is the anchored-handle discipline the activation walk
//! already implements, and wiring it through install/uninstall belongs to the conformance slice
//! (`links.rs`), where the whole link matrix lives. (Overclaim found by D on #535.)

use crate::activation::ActivationClaim;
use crate::install::{InstallRefusal, directory_for_digest, read_pointer};

/// Remove one adopted version's tree.
///
/// The removal is `remove_dir_all`, and the property it leans on is load-bearing: it deletes a
/// symbolic link or junction ITSELF rather than traversing it, so a link planted inside an
/// adopted tree after adoption cannot hand the removal a path outside the tree. That property is
/// measured by the junction cell in `tests/uninstall.rs` rather than trusted from documentation,
/// because user-authored artifacts are what pay if it drifts.
///
/// # Errors
///
/// Returns [`InstallRefusal::VersionRetained`] when the pointer still names the digest as
/// current or previous, and [`InstallRefusal::UnknownVersion`] when it was never adopted.
pub fn uninstall_version(claim: &ActivationClaim, digest: &str) -> Result<(), InstallRefusal> {
    let install_root = claim.install_root();
    crate::install::require_unlinked_layout(claim)?;
    let directory = directory_for_digest(digest).map_err(|_| InstallRefusal::UnknownVersion)?;
    let adopted = install_root.join("versions").join(directory);
    if !adopted.is_dir() {
        return Err(InstallRefusal::UnknownVersion);
    }
    if let Some(active) = read_pointer(install_root)?
        && (active.current == digest || active.previous.as_deref() == Some(digest))
    {
        return Err(InstallRefusal::VersionRetained);
    }
    std::fs::remove_dir_all(&adopted).map_err(|_| InstallRefusal::Unwritable)
}
