use graphhelm_protocols::{ArtifactReference, OpaqueId};
use thiserror::Error;

/// Rejected immutable artifact metadata.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
#[error("artifact registration is invalid")]
pub struct ArtifactRegistrationError;

impl ArtifactRegistrationError {
    /// Stable public diagnostic code for invalid artifact metadata.
    #[must_use]
    pub const fn code(self) -> &'static str {
        "GHEV004_EVIDENCE_INVALID"
    }
}

/// Immutable artifact metadata correlated to its producing append.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactRegistration {
    reference: ArtifactReference,
    producer_idempotency_key: OpaqueId,
}

impl ArtifactRegistration {
    pub fn new(
        reference: ArtifactReference,
        producer_idempotency_key: impl Into<String>,
    ) -> Result<Self, ArtifactRegistrationError> {
        validate_artifact_reference(&reference)?;
        Ok(Self {
            reference,
            producer_idempotency_key: OpaqueId::parse(producer_idempotency_key.into())
                .map_err(|_| ArtifactRegistrationError)?,
        })
    }

    #[must_use]
    pub const fn reference(&self) -> &ArtifactReference {
        &self.reference
    }

    #[must_use]
    pub const fn producer_idempotency_key(&self) -> &OpaqueId {
        &self.producer_idempotency_key
    }
}

/// Revalidates immutable Artifact locator/content identity after persistence.
pub fn validate_artifact_reference(
    reference: &ArtifactReference,
) -> Result<(), ArtifactRegistrationError> {
    let locator_digest = reference
        .locator()
        .as_str()
        .strip_prefix("artifact://sha256/")
        .ok_or(ArtifactRegistrationError)?;
    if locator_digest != reference.content_sha256().as_str() {
        return Err(ArtifactRegistrationError);
    }
    Ok(())
}
