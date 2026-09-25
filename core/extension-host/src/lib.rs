//! Atomic extension installation, activation, rollback and discovery (#212).

mod activation;
mod discovery;
mod install;
mod staging;
mod uninstall;

pub use activation::{
    ActivationClaim, ActivationRecord, ActivationStep, ClaimRefusal, ClaimStatus, activation_steps,
};
pub use discovery::{DiscoveryRefusal, resolve_executable};
pub use install::{
    ActiveVersions, InstallRefusal, InstalledVersion, active_versions, install_package, roll_back,
    switch_active,
};
pub use staging::{StagingRefusal, VerifiedStaging, activate_staged, verify_staged};
pub use uninstall::uninstall_version;
