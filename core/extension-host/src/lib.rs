//! Atomic extension installation, activation, rollback and discovery (#212).

mod activation;
mod discovery;
mod staging;

pub use activation::{
    ActivationClaim, ActivationRecord, ActivationStep, ClaimRefusal, activation_steps,
};
pub use discovery::{DiscoveryRefusal, resolve_executable};
pub use staging::{StagingRefusal, VerifiedStaging, activate_staged, verify_staged};
