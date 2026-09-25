//! Staging a package, verifying it, and activating what was verified.

use crate::activation::{ActivationClaim, ActivationRecord};
use std::path::{Path, PathBuf};

/// Why a staged package was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StagingRefusal {
    /// The staged package did not validate.
    Invalid,
    /// The staged bytes are not the bytes that were verified.
    StagedBytesChanged,
}

/// A staged package that has been verified, and the digest it verified as.
#[derive(Clone, Debug)]
pub struct VerifiedStaging {
    package: PathBuf,
    digest: String,
}

/// Verify a staged package.
///
/// # Errors
///
/// Returns [`StagingRefusal`] when the staged package does not validate.
pub fn verify_staged(package: &Path) -> Result<VerifiedStaging, StagingRefusal> {
    let validated = graphhelm_schema::validate_extension_package(package)
        .map_err(|_| StagingRefusal::Invalid)?;
    Ok(VerifiedStaging {
        package: package.to_path_buf(),
        digest: validated.package_digest,
    })
}

/// Activate what was verified.
///
/// # Errors
///
/// Returns [`StagingRefusal`] when the staged bytes are no longer the verified ones.
pub fn activate_staged(
    claim: &ActivationClaim,
    verified: &VerifiedStaging,
    executable_dir: &Path,
) -> Result<ActivationRecord, StagingRefusal> {
    // The digest is RE-DERIVED from the staged bytes here, not carried over from verification.
    //
    // Trusting the recorded digest is verify-then-use over a mutable path, which is a TOCTOU by
    // construction: "we verified it" is worthless if the verified bytes are not the used bytes.
    // The window between the two calls is exactly where an attacker who can write into staging
    // does their work, and the recorded digest cannot see it -- it is a memory of the past.
    //
    // The same validator answers both times, so this is one oracle asked twice rather than two
    // oracles compared.
    let now = graphhelm_schema::validate_extension_package(&verified.package)
        .map_err(|_| StagingRefusal::Invalid)?;

    if now.package_digest != verified.digest {
        return Err(StagingRefusal::StagedBytesChanged);
    }

    Ok(ActivationRecord::activate(
        claim,
        &now,
        executable_dir.join("graphhelm"),
    ))
}
