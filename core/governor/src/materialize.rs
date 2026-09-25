use std::{collections::BTreeMap, sync::Arc};

use graphhelm_events::{
    EvidenceError, EvidenceOpener, EvidenceRead, EvidenceRepository, EvidenceUnavailableReason,
    RepositoryFuture, SecretBytes, validate_sealed_evidence,
};
use graphhelm_graph::{
    raw_content_sha256, validate_persisted_projection, validate_publication_evidence_ids,
};
use graphhelm_protocols::{OpaqueId, PersistedGraphVersion, RepositoryScope};
use serde_json::Value;
use thiserror::Error;

const MAX_MATERIALIZED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum MaterializationError {
    #[error("required executable content is unavailable")]
    ContentUnavailable,
    #[error("safe graph or Evidence failed integrity verification")]
    Integrity,
    #[error("materialization exceeded a deterministic bound")]
    LimitExceeded,
    #[error("Evidence storage failed")]
    Storage,
}

impl MaterializationError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::ContentUnavailable => "GHE012_CONTENT_UNAVAILABLE",
            Self::Integrity => "GHE005_INTEGRITY_FAILURE",
            Self::LimitExceeded => "GHE006_LIMIT_EXCEEDED",
            Self::Storage => "GHE008_STORAGE_FAILURE",
        }
    }
}

pub enum MaterializedContent {
    Available(MaterializedValue),
    Unavailable(EvidenceUnavailableReason),
}

impl std::fmt::Debug for MaterializedContent {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Available(_) => formatter.write_str("Available([redacted])"),
            Self::Unavailable(reason) => {
                formatter.debug_tuple("Unavailable").field(reason).finish()
            }
        }
    }
}

/// Canonical JSON held in a zeroizing buffer and exposed only to a callback.
pub struct MaterializedValue(SecretBytes);

impl MaterializedValue {
    pub fn expose_json<R>(
        &self,
        callback: impl FnOnce(&Value) -> R,
    ) -> Result<R, MaterializationError> {
        let value = ZeroizingJson(
            self.0
                .expose(|bytes| serde_json::from_slice::<Value>(bytes))
                .map_err(|_| MaterializationError::Integrity)?,
        );
        Ok(callback(&value.0))
    }
}

impl std::fmt::Debug for MaterializedValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[redacted]")
    }
}

/// Ephemeral executable content bound to the verified safe graph projection.
pub struct MaterializedGraph {
    version: PersistedGraphVersion,
    content: BTreeMap<OpaqueId, MaterializedContent>,
}

impl std::fmt::Debug for MaterializedGraph {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MaterializedGraph")
            .field("version", &self.version)
            .field("content", &self.content)
            .finish()
    }
}

impl MaterializedGraph {
    #[must_use]
    pub const fn version(&self) -> &PersistedGraphVersion {
        &self.version
    }
    #[must_use]
    pub fn content(&self) -> &BTreeMap<OpaqueId, MaterializedContent> {
        &self.content
    }
    #[must_use]
    pub fn content_for(&self, slot_id: &OpaqueId) -> Option<&MaterializedContent> {
        self.content.get(slot_id)
    }
}

/// Exact-scope, fail-closed inverse of safe graph externalization.
pub struct ExecutableGraphMaterializer {
    repository: Arc<dyn EvidenceRepository>,
    opener: Arc<dyn EvidenceOpener>,
}

impl ExecutableGraphMaterializer {
    #[must_use]
    pub fn new(repository: Arc<dyn EvidenceRepository>, opener: Arc<dyn EvidenceOpener>) -> Self {
        Self { repository, opener }
    }

    pub fn materialize<'a>(
        &'a self,
        scope: RepositoryScope,
        version: &'a PersistedGraphVersion,
    ) -> RepositoryFuture<'a, Result<MaterializedGraph, MaterializationError>> {
        Box::pin(async move {
            validate_persisted_projection(version).map_err(|_| MaterializationError::Integrity)?;
            validate_publication_evidence_ids(&scope, version)
                .map_err(|_| MaterializationError::Integrity)?;
            let mut total_bytes = 0_usize;
            let mut content = BTreeMap::new();
            for slot in version.content_slots() {
                let read = self
                    .repository
                    .get_sealed(scope.clone(), slot.evidence_id().clone())
                    .await
                    .map_err(|_| MaterializationError::Storage)?;
                let materialized = match read {
                    EvidenceRead::Unavailable(reason) => {
                        if slot.required_for_execution() {
                            return Err(MaterializationError::ContentUnavailable);
                        }
                        MaterializedContent::Unavailable(reason)
                    }
                    EvidenceRead::Available(sealed) => {
                        validate_sealed_evidence(&sealed)
                            .map_err(|_| MaterializationError::Integrity)?;
                        if sealed.scope() != &scope
                            || sealed.reference().evidence_id() != slot.evidence_id()
                            || sealed.reference().content_sha256() != slot.content_sha256()
                            || sealed.sensitivity() != slot.sensitivity()
                            || sealed.media_type().as_str() != "application/json"
                        {
                            return Err(MaterializationError::Integrity);
                        }
                        let plaintext = match self.opener.open(scope.clone(), &sealed).await {
                            Ok(value) => value,
                            Err(EvidenceError::Unavailable) if slot.required_for_execution() => {
                                return Err(MaterializationError::ContentUnavailable);
                            }
                            Err(EvidenceError::Unavailable) => {
                                content.insert(
                                    slot.slot_id().clone(),
                                    MaterializedContent::Unavailable(
                                        EvidenceUnavailableReason::MissingKey,
                                    ),
                                );
                                continue;
                            }
                            Err(EvidenceError::TooLarge | EvidenceError::BatchTooLarge) => {
                                return Err(MaterializationError::LimitExceeded);
                            }
                            Err(EvidenceError::Invalid | EvidenceError::SealingFailed) => {
                                return Err(MaterializationError::Integrity);
                            }
                        };
                        total_bytes = total_bytes
                            .checked_add(plaintext.len())
                            .ok_or(MaterializationError::LimitExceeded)?;
                        if total_bytes > MAX_MATERIALIZED_BYTES {
                            return Err(MaterializationError::LimitExceeded);
                        }
                        let _validated = ZeroizingJson(
                            plaintext
                                .expose(|bytes| serde_json::from_slice::<Value>(bytes))
                                .map_err(|_| MaterializationError::Integrity)?,
                        );
                        if plaintext
                            .expose(raw_content_sha256)
                            .map_err(|_| MaterializationError::Integrity)?
                            != *slot.content_sha256()
                        {
                            return Err(MaterializationError::Integrity);
                        }
                        MaterializedContent::Available(MaterializedValue(plaintext))
                    }
                };
                if content
                    .insert(slot.slot_id().clone(), materialized)
                    .is_some()
                {
                    return Err(MaterializationError::Integrity);
                }
            }
            Ok(MaterializedGraph {
                version: version.clone(),
                content,
            })
        })
    }
}

struct ZeroizingJson(Value);

impl Drop for ZeroizingJson {
    fn drop(&mut self) {
        zeroize_json(&mut self.0);
    }
}

fn zeroize_json(value: &mut Value) {
    match value {
        Value::String(text) => {
            // SAFETY: replacing every byte with zero preserves UTF-8 validity until `String` is
            // dropped. Volatile writes prevent the compiler from eliminating the wipe.
            secure_zero_bytes(unsafe { text.as_bytes_mut() });
        }
        Value::Array(values) => values.iter_mut().for_each(zeroize_json),
        Value::Object(values) => {
            let taken = std::mem::take(values);
            for (mut key, mut value) in taken {
                // SAFETY: replacing every byte with zero preserves UTF-8 validity until `String`
                // is dropped.
                secure_zero_bytes(unsafe { key.as_bytes_mut() });
                zeroize_json(&mut value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}

fn secure_zero_bytes(bytes: &mut [u8]) {
    for byte in bytes {
        // SAFETY: `byte` is a valid, uniquely borrowed byte within the supplied slice.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}
