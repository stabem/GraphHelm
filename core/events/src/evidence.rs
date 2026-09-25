use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use graphhelm_protocols::{
    EvidenceId, EvidenceReference, MediaType, RawSha256, RepositoryScope, Sensitivity,
};
use sha2::{Digest, Sha256};
use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

use crate::{KeyError, KeyProvider, RepositoryFuture, WrapKeyRequest, WrappedKey};

const MAX_EVIDENCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_EVIDENCE_BATCH_BYTES: usize = 64 * 1024 * 1024;
/// Matches the repository append ceiling so item count is rejected before batch work.
pub const MAX_EVIDENCE_ITEMS_PER_BATCH: usize = 10_000;
const EVIDENCE_SCHEMA_VERSION: &str = "1.0.0";
const EVIDENCE_ALGORITHM: &str = "xchacha20poly1305";

/// A secret byte buffer that zeroizes its allocation and permits only callback-scoped access.
pub struct SecretBytes(Zeroizing<Vec<u8>>);

impl SecretBytes {
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn expose<R>(&self, callback: impl FnOnce(&[u8]) -> R) -> R {
        callback(self.0.as_slice())
    }

    pub fn consume<R>(self, callback: impl FnOnce(&[u8]) -> R) -> R {
        callback(self.0.as_slice())
    }
}

impl Zeroize for SecretBytes {
    fn zeroize(&mut self) {
        self.0.zeroize();
    }
}

/// Stable, redacted Evidence failures.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum EvidenceError {
    #[error("evidence is unavailable")]
    Unavailable,
    #[error("evidence exceeds the item size limit")]
    TooLarge,
    #[error("evidence batch exceeds the aggregate size limit")]
    BatchTooLarge,
    #[error("evidence is invalid")]
    Invalid,
    #[error("evidence sealing failed")]
    SealingFailed,
}

impl EvidenceError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "GHEV001_EVIDENCE_UNAVAILABLE",
            Self::TooLarge | Self::BatchTooLarge => "GHE006_LIMIT_EXCEEDED",
            Self::Invalid | Self::SealingFailed => "GHEV004_EVIDENCE_INVALID",
        }
    }
}

impl From<KeyError> for EvidenceError {
    fn from(error: KeyError) -> Self {
        match error {
            KeyError::Unavailable => Self::Unavailable,
            KeyError::Invalid | KeyError::Integrity | KeyError::Conflict | KeyError::Storage => {
                Self::Invalid
            }
        }
    }
}

/// Bounded plaintext prepared for one Evidence record.
pub struct EvidenceInput {
    local_ref: EvidenceId,
    media_type: MediaType,
    sensitivity: Sensitivity,
    retention_class: &'static str,
    plaintext: SecretBytes,
}

impl EvidenceInput {
    pub fn new(
        local_ref: impl Into<String>,
        media_type: impl Into<String>,
        sensitivity: Sensitivity,
        retention_class: &str,
        plaintext: SecretBytes,
    ) -> Result<Self, EvidenceError> {
        if plaintext.len() > MAX_EVIDENCE_BYTES {
            return Err(EvidenceError::TooLarge);
        }
        Ok(Self {
            local_ref: EvidenceId::parse(local_ref.into()).map_err(|_| EvidenceError::Invalid)?,
            media_type: MediaType::parse(media_type.into()).map_err(|_| EvidenceError::Invalid)?,
            sensitivity,
            retention_class: parse_retention_class(retention_class)?,
            plaintext,
        })
    }

    #[must_use]
    pub fn plaintext_len(&self) -> usize {
        self.plaintext.len()
    }
}

/// Encrypted Evidence and the metadata required for durable verification.
#[derive(Clone, PartialEq, Eq)]
pub struct SealedEvidence {
    reference: EvidenceReference,
    scope: RepositoryScope,
    media_type: MediaType,
    sensitivity: Sensitivity,
    retention_class: &'static str,
    algorithm: &'static str,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    wrapped_key: WrappedKey,
}

impl std::fmt::Debug for SealedEvidence {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SealedEvidence")
            .field("evidence_id", self.reference.evidence_id())
            .field("scope", &self.scope)
            .field("media_type", &self.media_type)
            .field("sensitivity", &self.sensitivity)
            .field("retention_class", &self.retention_class)
            .field("algorithm", &self.algorithm)
            .field("nonce", &"[redacted]")
            .field("ciphertext", &"[redacted]")
            .field("wrapped_key", &"[redacted]")
            .finish()
    }
}

impl SealedEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        reference: EvidenceReference,
        scope: RepositoryScope,
        media_type: impl Into<String>,
        sensitivity: Sensitivity,
        retention_class: &str,
        algorithm: &str,
        nonce: Vec<u8>,
        ciphertext: Vec<u8>,
        wrapped_key: WrappedKey,
    ) -> Result<Self, EvidenceError> {
        if algorithm != EVIDENCE_ALGORITHM
            || nonce.len() != 24
            || !(16..=MAX_EVIDENCE_BYTES + 16).contains(&ciphertext.len())
        {
            return Err(EvidenceError::Invalid);
        }
        Ok(Self {
            reference,
            scope,
            media_type: MediaType::parse(media_type.into()).map_err(|_| EvidenceError::Invalid)?,
            sensitivity,
            retention_class: parse_retention_class(retention_class)?,
            algorithm: EVIDENCE_ALGORITHM,
            nonce,
            ciphertext,
            wrapped_key,
        })
    }

    #[must_use]
    pub const fn reference(&self) -> &EvidenceReference {
        &self.reference
    }

    #[must_use]
    pub const fn scope(&self) -> &RepositoryScope {
        &self.scope
    }

    #[must_use]
    pub const fn media_type(&self) -> &MediaType {
        &self.media_type
    }

    #[must_use]
    pub const fn sensitivity(&self) -> Sensitivity {
        self.sensitivity
    }

    #[must_use]
    pub const fn retention_class(&self) -> &'static str {
        self.retention_class
    }

    #[must_use]
    pub const fn algorithm(&self) -> &'static str {
        self.algorithm
    }

    #[must_use]
    pub fn nonce(&self) -> &[u8] {
        &self.nonce
    }

    #[must_use]
    pub fn ciphertext(&self) -> &[u8] {
        &self.ciphertext
    }

    #[must_use]
    pub const fn wrapped_key(&self) -> &WrappedKey {
        &self.wrapped_key
    }

    #[must_use]
    pub fn plaintext_byte_length(&self) -> usize {
        self.ciphertext.len() - 16
    }
}

/// Object-safe Evidence sealing boundary.
pub trait EvidenceSealer: Send + Sync {
    fn seal<'a>(
        &'a self,
        scope: RepositoryScope,
        input: EvidenceInput,
    ) -> RepositoryFuture<'a, Result<SealedEvidence, EvidenceError>>;
}

/// Object-safe Evidence opening boundary.
pub trait EvidenceOpener: Send + Sync {
    fn open<'a>(
        &'a self,
        scope: RepositoryScope,
        evidence: &'a SealedEvidence,
    ) -> RepositoryFuture<'a, Result<SecretBytes, EvidenceError>>;
}

/// Adapter-neutral XChaCha20-Poly1305 Evidence protection.
pub struct EvidenceProtector<K> {
    key_provider: K,
}

impl<K> EvidenceProtector<K> {
    #[must_use]
    pub const fn new(key_provider: K) -> Self {
        Self { key_provider }
    }

    /// The underlying key provider, for callers that need to `authenticate`/`verify` bytes
    /// directly rather than through [`EvidenceSealer::seal`]/[`EvidenceOpener::open`] — e.g. an
    /// entry-level integrity MAC over plaintext metadata that must never itself be encrypted.
    #[must_use]
    pub const fn key_provider(&self) -> &K {
        &self.key_provider
    }
}

impl<K: KeyProvider> EvidenceProtector<K> {
    pub fn seal_batch<'a>(
        &'a self,
        scope: RepositoryScope,
        inputs: Vec<EvidenceInput>,
    ) -> RepositoryFuture<'a, Result<Vec<SealedEvidence>, EvidenceError>> {
        Box::pin(async move {
            if inputs.len() > MAX_EVIDENCE_ITEMS_PER_BATCH {
                return Err(EvidenceError::BatchTooLarge);
            }
            let total = inputs.iter().try_fold(0_usize, |total, input| {
                total.checked_add(input.plaintext_len())
            });
            if total.is_none_or(|total| total > MAX_EVIDENCE_BATCH_BYTES) {
                return Err(EvidenceError::BatchTooLarge);
            }
            let mut sealed = Vec::with_capacity(inputs.len());
            for input in inputs {
                sealed.push(self.seal(scope.clone(), input).await?);
            }
            Ok(sealed)
        })
    }
}

impl<K: KeyProvider> EvidenceSealer for EvidenceProtector<K> {
    fn seal<'a>(
        &'a self,
        scope: RepositoryScope,
        input: EvidenceInput,
    ) -> RepositoryFuture<'a, Result<SealedEvidence, EvidenceError>> {
        Box::pin(async move {
            if input.plaintext.len() > MAX_EVIDENCE_BYTES {
                return Err(EvidenceError::TooLarge);
            }

            let content_sha256 = input.plaintext.expose(raw_sha256);
            let aad = evidence_aad(
                &scope,
                &input.local_ref,
                &input.media_type,
                input.sensitivity,
                input.retention_class,
                &content_sha256,
            )?;

            let mut dek_bytes = Zeroizing::new(vec![0_u8; 32]);
            fill_secret_with(&mut dek_bytes, getrandom::fill)
                .map_err(|_| EvidenceError::SealingFailed)?;
            let dek = SecretBytes(dek_bytes);

            let mut nonce = vec![0_u8; 24];
            getrandom::fill(&mut nonce).map_err(|_| EvidenceError::SealingFailed)?;
            let ciphertext = dek.expose(|key| {
                let cipher = XChaCha20Poly1305::new_from_slice(key)
                    .map_err(|_| EvidenceError::SealingFailed)?;
                let nonce =
                    XNonce::try_from(nonce.as_slice()).map_err(|_| EvidenceError::SealingFailed)?;
                input.plaintext.expose(|plaintext| {
                    cipher
                        .encrypt(
                            &nonce,
                            Payload {
                                msg: plaintext,
                                aad: &aad,
                            },
                        )
                        .map_err(|_| EvidenceError::SealingFailed)
                })
            })?;
            let ciphertext_sha256 = raw_sha256(&ciphertext);

            let wrapped_key = self
                .key_provider
                .wrap(WrapKeyRequest::new(input.local_ref.as_str(), dek, aad)?)
                .await?;
            let reference =
                EvidenceReference::new(input.local_ref, content_sha256, ciphertext_sha256);
            SealedEvidence::new(
                reference,
                scope,
                input.media_type.to_string(),
                input.sensitivity,
                input.retention_class,
                EVIDENCE_ALGORITHM,
                nonce,
                ciphertext,
                wrapped_key,
            )
        })
    }
}

impl<K: KeyProvider> EvidenceOpener for EvidenceProtector<K> {
    fn open<'a>(
        &'a self,
        scope: RepositoryScope,
        evidence: &'a SealedEvidence,
    ) -> RepositoryFuture<'a, Result<SecretBytes, EvidenceError>> {
        Box::pin(async move {
            if &scope != evidence.scope()
                || evidence.algorithm() != EVIDENCE_ALGORITHM
                || evidence.nonce().len() != 24
                || !(16..=MAX_EVIDENCE_BYTES + 16).contains(&evidence.ciphertext().len())
                || evidence.wrapped_key().handle() != evidence.reference().evidence_id().as_str()
                || raw_sha256(evidence.ciphertext()) != *evidence.reference().ciphertext_sha256()
            {
                return Err(EvidenceError::Invalid);
            }

            let aad = evidence_aad(
                &scope,
                evidence.reference().evidence_id(),
                evidence.media_type(),
                evidence.sensitivity(),
                evidence.retention_class(),
                evidence.reference().content_sha256(),
            )?;
            if raw_sha256(&aad) != *evidence.wrapped_key().aad_sha256() {
                return Err(EvidenceError::Invalid);
            }

            let dek = self
                .key_provider
                .unwrap(evidence.wrapped_key().clone())
                .await?;
            let plaintext = dek.expose(|key| {
                let cipher =
                    XChaCha20Poly1305::new_from_slice(key).map_err(|_| EvidenceError::Invalid)?;
                let nonce =
                    XNonce::try_from(evidence.nonce()).map_err(|_| EvidenceError::Invalid)?;
                cipher
                    .decrypt(
                        &nonce,
                        Payload {
                            msg: evidence.ciphertext(),
                            aad: &aad,
                        },
                    )
                    .map_err(|_| EvidenceError::Invalid)
            })?;
            let plaintext = SecretBytes::new(plaintext);
            if plaintext.len() > MAX_EVIDENCE_BYTES
                || plaintext.expose(raw_sha256) != *evidence.reference().content_sha256()
            {
                return Err(EvidenceError::Invalid);
            }
            Ok(plaintext)
        })
    }
}

fn parse_retention_class(value: &str) -> Result<&'static str, EvidenceError> {
    match value {
        "ephemeral" => Ok("ephemeral"),
        "standard" => Ok("standard"),
        "legal_hold" => Ok("legal_hold"),
        _ => Err(EvidenceError::Invalid),
    }
}

fn raw_sha256(bytes: &[u8]) -> RawSha256 {
    RawSha256::parse(hex::encode(Sha256::digest(bytes)))
        .expect("SHA-256 output is always lowercase hexadecimal")
}

fn evidence_aad(
    scope: &RepositoryScope,
    evidence_id: &EvidenceId,
    media_type: &MediaType,
    sensitivity: Sensitivity,
    retention_class: &str,
    content_sha256: &RawSha256,
) -> Result<Vec<u8>, EvidenceError> {
    let mut aad = Vec::with_capacity(512);
    push_field(&mut aad, b"graphhelm-evidence-aad-v1")?;
    push_field(&mut aad, scope.workspace_id().as_str().as_bytes())?;
    push_field(&mut aad, scope.project_id().as_str().as_bytes())?;
    match scope.execution_id() {
        Some(execution_id) => {
            aad.push(1);
            push_field(&mut aad, execution_id.as_str().as_bytes())?;
        }
        None => aad.push(0),
    }
    push_field(&mut aad, evidence_id.as_str().as_bytes())?;
    push_field(&mut aad, EVIDENCE_SCHEMA_VERSION.as_bytes())?;
    push_field(&mut aad, media_type.as_str().as_bytes())?;
    push_field(
        &mut aad,
        match sensitivity {
            Sensitivity::Public => b"public",
            Sensitivity::Internal => b"internal",
            Sensitivity::Confidential => b"confidential",
            Sensitivity::Restricted => b"restricted",
        },
    )?;
    push_field(&mut aad, retention_class.as_bytes())?;
    push_field(&mut aad, content_sha256.as_str().as_bytes())?;
    Ok(aad)
}

pub(crate) fn validate_sealed_metadata(value: &SealedEvidence) -> Result<(), EvidenceError> {
    if value.algorithm() != EVIDENCE_ALGORITHM
        || value.nonce().len() != 24
        || !(16..=MAX_EVIDENCE_BYTES + 16).contains(&value.ciphertext().len())
        || value.wrapped_key().handle() != value.reference().evidence_id().as_str()
        || raw_sha256(value.ciphertext()) != *value.reference().ciphertext_sha256()
    {
        return Err(EvidenceError::Invalid);
    }
    let aad = evidence_aad(
        value.scope(),
        value.reference().evidence_id(),
        value.media_type(),
        value.sensitivity(),
        value.retention_class(),
        value.reference().content_sha256(),
    )?;
    if raw_sha256(&aad) != *value.wrapped_key().aad_sha256() {
        return Err(EvidenceError::Invalid);
    }
    Ok(())
}

fn push_field(output: &mut Vec<u8>, field: &[u8]) -> Result<(), EvidenceError> {
    let length = u32::try_from(field.len()).map_err(|_| EvidenceError::Invalid)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(field);
    Ok(())
}

fn fill_secret_with<E>(
    destination: &mut Zeroizing<Vec<u8>>,
    fill: impl FnOnce(&mut [u8]) -> Result<(), E>,
) -> Result<(), E> {
    if let Err(error) = fill(destination.as_mut_slice()) {
        destination.zeroize();
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use zeroize::Zeroizing;

    use super::fill_secret_with;

    #[test]
    fn partial_random_fill_failure_clears_destination() {
        let mut destination = Zeroizing::new(vec![0_u8; 32]);
        let result = fill_secret_with(&mut destination, |bytes| {
            bytes[..8].fill(0xa5);
            Err::<(), ()>(())
        });

        assert_eq!(result, Err(()));
        assert!(destination.is_empty());
    }
}
