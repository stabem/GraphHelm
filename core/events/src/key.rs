use std::{future::Future, pin::Pin};

use graphhelm_protocols::{OpaqueId, RawSha256};
use serde::Serialize;
use thiserror::Error;

use crate::SecretBytes;

const MAX_KEY_AAD_BYTES: usize = 4 * 1024;
const MAX_AUTHENTICATED_BYTES: usize = 1024 * 1024;
const MAX_PURPOSE_BYTES: usize = 64;
const MAX_PROVIDER_METADATA_TOKEN_BYTES: usize = 64;
const MAX_WIRE_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Heap-owned future used by adapter-neutral repository boundaries.
pub type RepositoryFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Stable, redacted failures returned by key providers.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum KeyError {
    #[error("key provider request is invalid")]
    Invalid,
    #[error("key material is unavailable")]
    Unavailable,
    #[error("key material failed integrity verification")]
    Integrity,
    #[error("key provider request conflicts with durable state")]
    Conflict,
    #[error("key provider storage operation failed")]
    Storage,
}

impl KeyError {
    #[must_use]
    pub const fn code(self) -> &'static str {
        "GHK001_KEY_UNAVAILABLE"
    }
}

/// Bounded, non-secret provider identity and rollback-comparison metadata.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyProviderMetadata {
    key_id: OpaqueId,
    algorithm: String,
    version: String,
    current_revocation_epoch: u64,
}

impl std::fmt::Debug for KeyProviderMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KeyProviderMetadata")
            .field("key_id", &self.key_id)
            .field("algorithm", &self.algorithm)
            .field("version", &self.version)
            .field("current_revocation_epoch", &self.current_revocation_epoch)
            .finish()
    }
}

impl KeyProviderMetadata {
    pub fn new(
        key_id: impl Into<String>,
        algorithm: impl Into<String>,
        version: impl Into<String>,
        current_revocation_epoch: u64,
    ) -> Result<Self, KeyError> {
        let algorithm = algorithm.into();
        let version = version.into();
        if !valid_provider_metadata_token(&algorithm)
            || !valid_provider_metadata_token(&version)
            || current_revocation_epoch > MAX_WIRE_SAFE_INTEGER
        {
            return Err(KeyError::Invalid);
        }
        Ok(Self {
            key_id: OpaqueId::parse(key_id.into()).map_err(|_| KeyError::Invalid)?,
            algorithm,
            version,
            current_revocation_epoch,
        })
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        self.key_id.as_str()
    }

    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub const fn current_revocation_epoch(&self) -> u64 {
        self.current_revocation_epoch
    }
}

/// A per-item data-encryption key wrapped by a provider.
#[derive(Clone, PartialEq, Eq)]
pub struct WrappedKey {
    key_id: OpaqueId,
    handle: OpaqueId,
    algorithm: &'static str,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    aad_sha256: RawSha256,
}

impl std::fmt::Debug for WrappedKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WrappedKey")
            .field("key_id", &"[redacted]")
            .field("handle", &"[redacted]")
            .field("algorithm", &self.algorithm)
            .field("nonce", &"[redacted]")
            .field("ciphertext", &"[redacted]")
            .field("aad_sha256", &self.aad_sha256)
            .finish()
    }
}

impl WrappedKey {
    pub fn new(
        key_id: impl Into<String>,
        handle: impl Into<String>,
        algorithm: &str,
        nonce: Vec<u8>,
        ciphertext: Vec<u8>,
        aad_sha256: RawSha256,
    ) -> Result<Self, KeyError> {
        if algorithm != "xchacha20poly1305" || nonce.len() != 24 || ciphertext.len() != 48 {
            return Err(KeyError::Invalid);
        }
        Ok(Self {
            key_id: OpaqueId::parse(key_id.into()).map_err(|_| KeyError::Invalid)?,
            handle: OpaqueId::parse(handle.into()).map_err(|_| KeyError::Invalid)?,
            algorithm: "xchacha20poly1305",
            nonce,
            ciphertext,
            aad_sha256,
        })
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        self.key_id.as_str()
    }

    #[must_use]
    pub fn handle(&self) -> &str {
        self.handle.as_str()
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
    pub const fn aad_sha256(&self) -> &RawSha256 {
        &self.aad_sha256
    }

    pub fn into_parts(self) -> (String, String, Vec<u8>, Vec<u8>, RawSha256) {
        (
            self.key_id.to_string(),
            self.handle.to_string(),
            self.nonce,
            self.ciphertext,
            self.aad_sha256,
        )
    }
}

/// Request to wrap a zeroizing data-encryption key.
pub struct WrapKeyRequest {
    handle: OpaqueId,
    plaintext_key: SecretBytes,
    aad: Vec<u8>,
}

impl WrapKeyRequest {
    pub fn new(
        handle: impl Into<String>,
        plaintext_key: SecretBytes,
        aad: Vec<u8>,
    ) -> Result<Self, KeyError> {
        if plaintext_key.len() != 32 || aad.is_empty() || aad.len() > MAX_KEY_AAD_BYTES {
            return Err(KeyError::Invalid);
        }
        Ok(Self {
            handle: OpaqueId::parse(handle.into()).map_err(|_| KeyError::Invalid)?,
            plaintext_key,
            aad,
        })
    }

    #[must_use]
    pub fn handle(&self) -> &str {
        self.handle.as_str()
    }

    #[must_use]
    pub const fn plaintext_key(&self) -> &SecretBytes {
        &self.plaintext_key
    }

    #[must_use]
    pub fn aad(&self) -> &[u8] {
        &self.aad
    }

    pub fn into_parts(self) -> (String, SecretBytes, Vec<u8>) {
        (self.handle.to_string(), self.plaintext_key, self.aad)
    }
}

/// Durable revocation request.
pub struct RevokeKeyRequest {
    handle: OpaqueId,
    idempotency_key: OpaqueId,
}

impl RevokeKeyRequest {
    pub fn new(
        handle: impl Into<String>,
        idempotency_key: impl Into<String>,
    ) -> Result<Self, KeyError> {
        Ok(Self {
            handle: OpaqueId::parse(handle.into()).map_err(|_| KeyError::Invalid)?,
            idempotency_key: OpaqueId::parse(idempotency_key.into())
                .map_err(|_| KeyError::Invalid)?,
        })
    }

    #[must_use]
    pub fn handle(&self) -> &str {
        self.handle.as_str()
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        self.idempotency_key.as_str()
    }

    pub fn into_parts(self) -> (String, String) {
        (self.handle.to_string(), self.idempotency_key.to_string())
    }
}

/// Authenticated, monotonic proof of key-handle revocation.
#[derive(Clone, PartialEq, Eq)]
pub struct RevocationReceipt {
    handle: OpaqueId,
    idempotency_key: OpaqueId,
    epoch: u64,
    authentication_tag: AuthenticationTag,
}

impl std::fmt::Debug for RevocationReceipt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RevocationReceipt")
            .field("handle", &self.handle)
            .field("idempotency_key", &self.idempotency_key)
            .field("epoch", &self.epoch)
            .field("authentication_tag", &"[redacted]")
            .finish()
    }
}

impl RevocationReceipt {
    pub fn new(
        handle: impl Into<String>,
        idempotency_key: impl Into<String>,
        epoch: u64,
        authentication_tag: AuthenticationTag,
    ) -> Result<Self, KeyError> {
        if epoch == 0 || epoch > MAX_WIRE_SAFE_INTEGER {
            return Err(KeyError::Invalid);
        }
        Ok(Self {
            handle: OpaqueId::parse(handle.into()).map_err(|_| KeyError::Invalid)?,
            idempotency_key: OpaqueId::parse(idempotency_key.into())
                .map_err(|_| KeyError::Invalid)?,
            epoch,
            authentication_tag,
        })
    }

    #[must_use]
    pub fn handle(&self) -> &str {
        self.handle.as_str()
    }

    #[must_use]
    pub fn idempotency_key(&self) -> &str {
        self.idempotency_key.as_str()
    }

    #[must_use]
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    #[must_use]
    pub const fn authentication_tag(&self) -> &AuthenticationTag {
        &self.authentication_tag
    }
}

/// Provider-produced authentication tag over bounded bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct AuthenticationTag {
    key_id: OpaqueId,
    algorithm: &'static str,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for AuthenticationTag {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthenticationTag")
            .field("key_id", &"[redacted]")
            .field("algorithm", &self.algorithm)
            .field("bytes", &"[redacted]")
            .finish()
    }
}

impl AuthenticationTag {
    pub fn new(
        key_id: impl Into<String>,
        algorithm: &str,
        bytes: Vec<u8>,
    ) -> Result<Self, KeyError> {
        if algorithm != "hmac-sha256" || bytes.len() != 32 {
            return Err(KeyError::Invalid);
        }
        Ok(Self {
            key_id: OpaqueId::parse(key_id.into()).map_err(|_| KeyError::Invalid)?,
            algorithm: "hmac-sha256",
            bytes,
        })
    }

    #[must_use]
    pub fn key_id(&self) -> &str {
        self.key_id.as_str()
    }

    #[must_use]
    pub const fn algorithm(&self) -> &'static str {
        self.algorithm
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Bounded input for provider authentication.
pub struct AuthenticateRequest {
    purpose: String,
    bytes: Vec<u8>,
}

impl AuthenticateRequest {
    pub fn new(purpose: impl Into<String>, bytes: Vec<u8>) -> Result<Self, KeyError> {
        let purpose = purpose.into();
        if !valid_purpose(&purpose) || bytes.len() > MAX_AUTHENTICATED_BYTES {
            return Err(KeyError::Invalid);
        }
        Ok(Self { purpose, bytes })
    }

    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_parts(self) -> (String, Vec<u8>) {
        (self.purpose, self.bytes)
    }
}

/// Bounded verification request for provider-authenticated bytes.
pub struct VerifyAuthenticationRequest {
    purpose: String,
    bytes: Vec<u8>,
    tag: AuthenticationTag,
}

impl VerifyAuthenticationRequest {
    pub fn new(
        purpose: impl Into<String>,
        bytes: Vec<u8>,
        tag: AuthenticationTag,
    ) -> Result<Self, KeyError> {
        let authenticated = AuthenticateRequest::new(purpose, bytes)?;
        Ok(Self {
            purpose: authenticated.purpose,
            bytes: authenticated.bytes,
            tag,
        })
    }

    #[must_use]
    pub fn purpose(&self) -> &str {
        &self.purpose
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    pub const fn tag(&self) -> &AuthenticationTag {
        &self.tag
    }

    pub fn into_parts(self) -> (String, Vec<u8>, AuthenticationTag) {
        (self.purpose, self.bytes, self.tag)
    }
}

fn valid_purpose(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PURPOSE_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
}

fn valid_provider_metadata_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_PROVIDER_METADATA_TOKEN_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

/// Adapter-neutral key wrapping, revocation, and authentication boundary.
pub trait KeyProvider: Send + Sync {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>>;
    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>>;
    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>>;
    fn revoke<'a>(
        &'a self,
        request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>>;
    fn authenticate<'a>(
        &'a self,
        request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>>;
    fn verify<'a>(
        &'a self,
        request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>>;
}
