//! What is active, and what it is allowed to run.

use std::path::{Path, PathBuf};

/// The record of an activated extension version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivationRecord {
    package_root: PathBuf,
    executables: Vec<PathBuf>,
    package_digest: String,
}

impl ActivationRecord {
    /// A record for a package whose canonical executable was never recorded.
    #[must_use]
    pub fn for_package_without_recorded_executable(package_root: &Path) -> Self {
        Self {
            package_root: package_root.to_path_buf(),
            executables: Vec::new(),
            package_digest: String::new(),
        }
    }

    /// Mint a record by ACTIVATING a validated package.
    ///
    /// Taking the claim by reference is the point: there is no path from a validated package to an
    /// activation record that does not pass through holding the claim. Validation answers
    /// "well-formed"; only this constructor answers "active".
    #[must_use]
    pub fn activate(
        _claim: &ActivationClaim,
        package: &graphhelm_schema::ValidatedExtensionPackage,
        executable: PathBuf,
    ) -> Self {
        Self {
            package_root: executable
                .parent()
                .map_or_else(PathBuf::new, Path::to_path_buf),
            executables: vec![executable],
            package_digest: package.package_digest.clone(),
        }
    }

    /// Whether this record authorizes the given package.
    ///
    /// The comparison is the DIGEST, not the id. Comparing ids reads as "is this the same
    /// extension?" and answers yes for every version of it, so a record minted for 1.0.0 would
    /// authorize 1.1.0 -- and 1.1.0 is a different artifact that merely shares a name. Validation
    /// says well-formed, identity says same family, and neither says "this is what was activated".
    #[must_use]
    pub fn authorizes(&self, package: &graphhelm_schema::ValidatedExtensionPackage) -> bool {
        !self.package_digest.is_empty() && self.package_digest == package.package_digest
    }

    /// A record naming the executables that installation recorded.
    ///
    /// A list rather than an option, because the interesting failure is a state that names TWO --
    /// which an `Option` cannot represent and therefore cannot refuse.
    #[must_use]
    pub fn for_package_with_recorded_executables(
        package_root: &Path,
        executables: Vec<PathBuf>,
    ) -> Self {
        Self {
            package_root: package_root.to_path_buf(),
            executables,
            package_digest: String::new(),
        }
    }

    /// Where the package lives.
    #[must_use]
    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    /// Every executable this record names.
    #[must_use]
    pub fn recorded_executables(&self) -> &[PathBuf] {
        &self.executables
    }
}

/// Why a claim was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimRefusal {
    /// Another activation already holds the claim.
    AlreadyHeld,
    /// The claim could not be written at all.
    Unwritable,
}

/// An exclusive claim over the activation state.
#[derive(Debug)]
pub struct ActivationClaim {
    path: PathBuf,
}

impl ActivationClaim {
    /// Take the claim, or refuse.
    ///
    /// # Errors
    ///
    /// Returns [`ClaimRefusal`] when the claim is already held or cannot be written.
    pub fn acquire(install_root: &Path) -> Result<Self, ClaimRefusal> {
        let path = install_root.join("activation.claim");

        // `create_new`, never `create`. Both are "make the file"; only one of them FAILS when the
        // file is already there. A create-if-absent that succeeds twice is not a claim -- it is two
        // writers each believing they hold the switch, and the second one silently truncates the
        // first one's record on the way in. The house rule (ED-22) names the same distinction on
        // the PowerShell side: CreateNew, never New-Item.
        //
        // The two refusals are kept apart because they call for different actions: AlreadyHeld
        // means wait or abort, Unwritable means the install root is wrong or unreachable. One code
        // for both would flatten "someone else is mid-activation" into "your disk is broken".
        match std::fs::File::create_new(&path) {
            Ok(_) => Ok(Self { path }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(ClaimRefusal::AlreadyHeld)
            }
            Err(_) => Err(ClaimRefusal::Unwritable),
        }
    }

    /// Where the claim lives on disk.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ActivationClaim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// A durable step in switching the active version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationStep {
    /// The staged copy has been verified against the manifest digest.
    StagedCopyVerified,
    /// The previous version is no longer the active one.
    PreviousVersionRetired,
    /// The new version is recorded as active.
    NewVersionRecorded,
}

/// The order the durable steps of an activation are written in.
#[must_use]
pub fn activation_steps() -> Vec<ActivationStep> {
    // Record the new version BEFORE retiring the previous one. Both orders complete identically;
    // they differ only in what a crash leaves behind.
    //
    // Retire-then-record leaves a machine with no active version at all -- which is not a rollback,
    // it is an outage, and it is indistinguishable from a machine that never had the extension.
    // Record-then-retire leaves two versions recorded for an instant, which is a state a reader can
    // resolve: the newest recorded one is active and the older is collectable.
    //
    // The rule is not "write in this order". It is: order the writes so that every intermediate a
    // crash can leave behind reads as the TRUTH.
    vec![
        ActivationStep::StagedCopyVerified,
        ActivationStep::NewVersionRecorded,
        ActivationStep::PreviousVersionRetired,
    ]
}
