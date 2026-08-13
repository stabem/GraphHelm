use graphhelm_events::{EventRepositoryError, SealedEvidence, WrappedKey};
use graphhelm_protocols::{EvidenceReference, MediaType, RawSha256, RepositoryScope, Sensitivity};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredEvidence {
    reference: EvidenceReference,
    scope: RepositoryScope,
    media_type: MediaType,
    sensitivity: Sensitivity,
    retention_class: String,
    algorithm: String,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    wrapped_key_id: String,
    wrapped_handle: String,
    wrapped_algorithm: String,
    wrapped_nonce: Vec<u8>,
    wrapped_ciphertext: Vec<u8>,
    wrapped_aad_sha256: RawSha256,
}

impl StoredEvidence {
    pub(crate) fn from_sealed(value: &SealedEvidence) -> Self {
        Self {
            reference: value.reference().clone(),
            scope: value.scope().clone(),
            media_type: value.media_type().clone(),
            sensitivity: value.sensitivity(),
            retention_class: value.retention_class().to_owned(),
            algorithm: value.algorithm().to_owned(),
            nonce: value.nonce().to_vec(),
            ciphertext: value.ciphertext().to_vec(),
            wrapped_key_id: value.wrapped_key().key_id().to_owned(),
            wrapped_handle: value.wrapped_key().handle().to_owned(),
            wrapped_algorithm: value.wrapped_key().algorithm().to_owned(),
            wrapped_nonce: value.wrapped_key().nonce().to_vec(),
            wrapped_ciphertext: value.wrapped_key().ciphertext().to_vec(),
            wrapped_aad_sha256: value.wrapped_key().aad_sha256().clone(),
        }
    }

    pub(crate) fn into_sealed(self) -> Result<SealedEvidence, EventRepositoryError> {
        let wrapped = WrappedKey::new(
            self.wrapped_key_id,
            self.wrapped_handle,
            &self.wrapped_algorithm,
            self.wrapped_nonce,
            self.wrapped_ciphertext,
            self.wrapped_aad_sha256,
        )
        .map_err(|_| EventRepositoryError::Integrity)?;
        SealedEvidence::new(
            self.reference,
            self.scope,
            self.media_type.to_string(),
            self.sensitivity,
            &self.retention_class,
            &self.algorithm,
            self.nonce,
            self.ciphertext,
            wrapped,
        )
        .map_err(|_| EventRepositoryError::Integrity)
    }
}
