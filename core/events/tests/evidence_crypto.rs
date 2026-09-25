use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
};

use graphhelm_events::{
    ArtifactRegistration, ArtifactRegistrationError, AuthenticateRequest, AuthenticationTag,
    EvidenceError, EvidenceInput, EvidenceOpener, EvidenceProtector, EvidenceSealer, KeyError,
    KeyProvider, KeyProviderMetadata, MAX_EVIDENCE_ITEMS_PER_BATCH, RepositoryFuture,
    RevocationReceipt, RevokeKeyRequest, SealedEvidence, SecretBytes, VerifyAuthenticationRequest,
    WrapKeyRequest, WrappedKey,
};
use graphhelm_protocols::{
    ArtifactId, ArtifactLocator, ArtifactReference, ExecutionId, MediaType, ProjectId, RawSha256,
    RepositoryScope, SemanticVersion, Sensitivity, WorkspaceId,
};
use sha2::{Digest, Sha256};
use static_assertions::assert_not_impl_any;
use zeroize::Zeroize;

const MIB: usize = 1024 * 1024;

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => thread::park(),
        }
    }
}

fn scope(execution: &str) -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-1").unwrap(),
        ProjectId::parse("project-1").unwrap(),
        Some(ExecutionId::parse(execution).unwrap()),
    )
}

fn digest(bytes: &[u8]) -> RawSha256 {
    RawSha256::parse(hex::encode(Sha256::digest(bytes))).unwrap()
}

fn input(id: &str, plaintext: Vec<u8>) -> EvidenceInput {
    EvidenceInput::new(
        id,
        "text/plain",
        Sensitivity::Restricted,
        "standard",
        SecretBytes::new(plaintext),
    )
    .unwrap()
}

fn evidence_error<T>(result: Result<T, EvidenceError>) -> EvidenceError {
    match result {
        Ok(_) => panic!("expected Evidence operation to fail"),
        Err(error) => error,
    }
}

#[derive(Default)]
struct InMemoryKeyState {
    keys: Mutex<BTreeMap<String, Vec<u8>>>,
    revoked: Mutex<BTreeSet<String>>,
    wrap_calls: AtomicUsize,
}

#[derive(Default)]
struct InMemoryKeyProvider {
    state: Arc<InMemoryKeyState>,
}

impl KeyProvider for InMemoryKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("test-key", "test-provider", "1.0.0", 0) })
    }

    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            self.state.wrap_calls.fetch_add(1, Ordering::SeqCst);
            let handle = request.handle().to_owned();
            let key = request.plaintext_key().expose(|bytes| bytes.to_vec());
            self.state.keys.lock().unwrap().insert(handle.clone(), key);
            WrappedKey::new(
                "test-key",
                handle,
                "xchacha20poly1305",
                vec![7; 24],
                vec![11; 48],
                digest(request.aad()),
            )
        })
    }

    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async move {
            if self
                .state
                .revoked
                .lock()
                .unwrap()
                .contains(wrapped.handle())
            {
                return Err(KeyError::Unavailable);
            }
            self.state
                .keys
                .lock()
                .unwrap()
                .get(wrapped.handle())
                .cloned()
                .map(SecretBytes::new)
                .ok_or(KeyError::Unavailable)
        })
    }

    fn revoke<'a>(
        &'a self,
        request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async move {
            self.state
                .revoked
                .lock()
                .unwrap()
                .insert(request.handle().to_owned());
            RevocationReceipt::new(
                request.handle(),
                request.idempotency_key(),
                1,
                AuthenticationTag::new("test-key", "hmac-sha256", vec![3; 32])?,
            )
        })
    }

    fn authenticate<'a>(
        &'a self,
        _request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async { AuthenticationTag::new("test-key", "hmac-sha256", vec![3; 32]) })
    }

    fn verify<'a>(
        &'a self,
        _request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async { Ok(()) })
    }
}

fn protector() -> EvidenceProtector<InMemoryKeyProvider> {
    EvidenceProtector::new(InMemoryKeyProvider::default())
}

#[test]
fn public_error_codes_match_the_normative_catalog_exactly() {
    assert_eq!(
        EvidenceError::Unavailable.code(),
        "GHEV001_EVIDENCE_UNAVAILABLE"
    );
    for error in [EvidenceError::Invalid, EvidenceError::SealingFailed] {
        assert_eq!(error.code(), "GHEV004_EVIDENCE_INVALID");
    }
    for error in [EvidenceError::TooLarge, EvidenceError::BatchTooLarge] {
        assert_eq!(error.code(), "GHE006_LIMIT_EXCEEDED");
    }
    for error in [
        KeyError::Invalid,
        KeyError::Unavailable,
        KeyError::Integrity,
        KeyError::Conflict,
        KeyError::Storage,
    ] {
        assert_eq!(error.code(), "GHK001_KEY_UNAVAILABLE");
    }
}

#[test]
fn key_provider_metadata_is_bounded_and_contains_only_safe_fields() {
    assert_not_impl_any!(KeyProviderMetadata: std::fmt::Display);
    let metadata = KeyProviderMetadata::new("test-key", "test-provider", "1.0.0", 7).unwrap();
    assert_eq!(metadata.key_id(), "test-key");
    assert_eq!(metadata.algorithm(), "test-provider");
    assert_eq!(metadata.version(), "1.0.0");
    assert_eq!(metadata.current_revocation_epoch(), 7);
    assert!(KeyProviderMetadata::new("test-key", "", "1.0.0", 0).is_err());
    assert!(KeyProviderMetadata::new("test-key", "test-provider", "x".repeat(65), 0).is_err());

    assert_eq!(
        serde_json::to_value(&metadata).unwrap(),
        serde_json::json!({
            "keyId": "test-key",
            "algorithm": "test-provider",
            "version": "1.0.0",
            "currentRevocationEpoch": 7
        })
    );
}

#[test]
fn key_provider_epochs_stay_within_the_wire_safe_integer_boundary() {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

    assert!(
        KeyProviderMetadata::new("test-key", "test-provider", "1.0.0", MAX_SAFE_INTEGER).is_ok()
    );
    assert!(
        KeyProviderMetadata::new("test-key", "test-provider", "1.0.0", MAX_SAFE_INTEGER + 1,)
            .is_err()
    );

    let tag = AuthenticationTag::new("test-key", "hmac-sha256", vec![0_u8; 32]).unwrap();
    assert!(
        RevocationReceipt::new("handle-1", "operation-1", MAX_SAFE_INTEGER, tag.clone()).is_ok()
    );
    assert!(RevocationReceipt::new("handle-1", "operation-1", 0, tag.clone()).is_err());
    assert!(RevocationReceipt::new("handle-1", "operation-1", MAX_SAFE_INTEGER + 1, tag).is_err());
}

fn counting_protector() -> (
    EvidenceProtector<InMemoryKeyProvider>,
    Arc<InMemoryKeyState>,
) {
    let state = Arc::new(InMemoryKeyState::default());
    (
        EvidenceProtector::new(InMemoryKeyProvider {
            state: Arc::clone(&state),
        }),
        state,
    )
}

fn rebuild(
    sealed: &SealedEvidence,
    scope: RepositoryScope,
    media_type: &str,
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
    wrapped_key: WrappedKey,
) -> SealedEvidence {
    SealedEvidence::new(
        sealed.reference().clone(),
        scope,
        media_type,
        sealed.sensitivity(),
        sealed.retention_class(),
        sealed.algorithm(),
        nonce,
        ciphertext,
        wrapped_key,
    )
    .unwrap()
}

#[test]
fn secret_bytes_exposes_only_through_callbacks_and_zeroizes_explicitly() {
    assert_not_impl_any!(SecretBytes: Clone, std::fmt::Debug, std::fmt::Display, serde::Serialize);

    let mut secret = SecretBytes::new(b"top-secret".to_vec());
    assert_eq!(secret.expose(|bytes| bytes.len()), 10);
    secret.zeroize();
    assert_eq!(secret.expose(|bytes| bytes.len()), 0);

    let consumed_length = SecretBytes::new(vec![1, 2, 3]).consume(|bytes| bytes.len());
    assert_eq!(consumed_length, 3);
}

#[test]
fn sealed_evidence_round_trips_without_plaintext_in_surfaces() {
    let protector = protector();
    let plaintext = b"instructions-canary".to_vec();
    let sealed =
        block_on(protector.seal(scope("execution-1"), input("evidence-1", plaintext.clone())))
            .unwrap();

    assert!(
        !sealed
            .ciphertext()
            .windows(plaintext.len())
            .any(|window| window == plaintext)
    );
    assert!(!format!("{sealed:?}").contains("instructions-canary"));

    let opened = block_on(protector.open(scope("execution-1"), &sealed)).unwrap();
    assert_eq!(opened.expose(|bytes| bytes.to_vec()), plaintext);
}

#[test]
fn evidence_open_rejects_scope_metadata_and_aad_mismatch() {
    let protector = protector();
    let sealed = block_on(protector.seal(
        scope("execution-1"),
        input("evidence-2", b"bound-content".to_vec()),
    ))
    .unwrap();

    let wrong_scope = evidence_error(block_on(protector.open(scope("execution-2"), &sealed)));
    assert_eq!(wrong_scope.code(), "GHEV004_EVIDENCE_INVALID");
    assert!(!wrong_scope.to_string().contains("execution-1"));

    let changed_metadata = rebuild(
        &sealed,
        scope("execution-1"),
        "application/json",
        sealed.nonce().to_vec(),
        sealed.ciphertext().to_vec(),
        sealed.wrapped_key().clone(),
    );
    let error = evidence_error(block_on(
        protector.open(scope("execution-1"), &changed_metadata),
    ));
    assert_eq!(error.code(), "GHEV004_EVIDENCE_INVALID");
}

#[test]
fn evidence_open_rejects_nonce_ciphertext_and_wrapped_key_tamper() {
    let protector = protector();
    let sealed = block_on(protector.seal(
        scope("execution-1"),
        input("evidence-3", b"tamper-canary".to_vec()),
    ))
    .unwrap();

    let mut nonce = sealed.nonce().to_vec();
    nonce[0] ^= 1;
    let changed_nonce = rebuild(
        &sealed,
        scope("execution-1"),
        sealed.media_type().as_str(),
        nonce,
        sealed.ciphertext().to_vec(),
        sealed.wrapped_key().clone(),
    );
    assert_eq!(
        evidence_error(block_on(
            protector.open(scope("execution-1"), &changed_nonce),
        ))
        .code(),
        "GHEV004_EVIDENCE_INVALID"
    );

    let mut ciphertext = sealed.ciphertext().to_vec();
    ciphertext[0] ^= 1;
    let changed_ciphertext = rebuild(
        &sealed,
        scope("execution-1"),
        sealed.media_type().as_str(),
        sealed.nonce().to_vec(),
        ciphertext,
        sealed.wrapped_key().clone(),
    );
    assert_eq!(
        evidence_error(block_on(
            protector.open(scope("execution-1"), &changed_ciphertext),
        ))
        .code(),
        "GHEV004_EVIDENCE_INVALID"
    );

    let wrapped = WrappedKey::new(
        sealed.wrapped_key().key_id(),
        "different-handle",
        sealed.wrapped_key().algorithm(),
        sealed.wrapped_key().nonce().to_vec(),
        sealed.wrapped_key().ciphertext().to_vec(),
        sealed.wrapped_key().aad_sha256().clone(),
    )
    .unwrap();
    let changed_wrapped = rebuild(
        &sealed,
        scope("execution-1"),
        sealed.media_type().as_str(),
        sealed.nonce().to_vec(),
        sealed.ciphertext().to_vec(),
        wrapped,
    );
    assert_eq!(
        evidence_error(block_on(
            protector.open(scope("execution-1"), &changed_wrapped),
        ))
        .code(),
        "GHEV004_EVIDENCE_INVALID"
    );
}

#[test]
fn evidence_limits_are_checked_before_crypto() {
    let protector = protector();

    let exact = block_on(protector.seal(
        scope("execution-1"),
        input("evidence-limit", vec![0x5a; 16 * MIB]),
    ))
    .unwrap();
    assert_eq!(exact.plaintext_byte_length(), 16 * MIB);

    let oversized = evidence_error(EvidenceInput::new(
        "evidence-too-large",
        "application/octet-stream",
        Sensitivity::Confidential,
        "ephemeral",
        SecretBytes::new(vec![0; 16 * MIB + 1]),
    ));
    assert_eq!(oversized.code(), "GHE006_LIMIT_EXCEEDED");

    let (aggregate_protector, key_state) = counting_protector();
    let inputs = (0..5)
        .map(|index| input(&format!("aggregate-{index}"), vec![0; 13 * MIB]))
        .collect::<Vec<_>>();
    let aggregate = evidence_error(block_on(
        aggregate_protector.seal_batch(scope("execution-1"), inputs),
    ));
    assert_eq!(aggregate.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(key_state.wrap_calls.load(Ordering::SeqCst), 0);

    let item_inputs = (0..=MAX_EVIDENCE_ITEMS_PER_BATCH)
        .map(|index| input(&format!("item-{index}"), Vec::new()))
        .collect::<Vec<_>>();
    let item_count = evidence_error(block_on(
        aggregate_protector.seal_batch(scope("execution-1"), item_inputs),
    ));
    assert_eq!(item_count.code(), "GHE006_LIMIT_EXCEEDED");
    assert_eq!(key_state.wrap_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn evidence_uses_a_fresh_nonce_for_each_seal() {
    let protector = protector();
    let first = block_on(protector.seal(
        scope("execution-1"),
        input("nonce-1", b"same-content".to_vec()),
    ))
    .unwrap();
    let second = block_on(protector.seal(
        scope("execution-1"),
        input("nonce-2", b"same-content".to_vec()),
    ))
    .unwrap();

    assert_ne!(first.nonce(), second.nonce());
    assert_ne!(first.ciphertext(), second.ciphertext());
}

#[test]
fn artifact_registration_requires_content_address_match() {
    let content_sha256 = digest(b"artifact bytes");
    let reference = ArtifactReference::new(
        ArtifactId::parse("artifact-1").unwrap(),
        ArtifactLocator::parse(format!("artifact://sha256/{content_sha256}")).unwrap(),
        content_sha256,
        MediaType::parse("application/octet-stream").unwrap(),
        14,
        Sensitivity::Internal,
        SemanticVersion::parse("1.0.0").unwrap(),
    )
    .unwrap();
    assert!(ArtifactRegistration::new(reference, "publish-1").is_ok());

    let mismatched = ArtifactReference::new(
        ArtifactId::parse("artifact-2").unwrap(),
        ArtifactLocator::parse(format!("artifact://sha256/{}", digest(b"other"))).unwrap(),
        digest(b"artifact bytes"),
        MediaType::parse("application/octet-stream").unwrap(),
        14,
        Sensitivity::Internal,
        SemanticVersion::parse("1.0.0").unwrap(),
    )
    .unwrap();
    assert!(ArtifactRegistration::new(mismatched, "publish-2").is_err());
}

#[test]
fn artifact_registration_error_is_public_and_safely_mapped() {
    let error = ArtifactRegistrationError;
    assert_eq!(error.code(), "GHEV004_EVIDENCE_INVALID");
    assert_eq!(error.to_string(), "artifact registration is invalid");
    assert_eq!(format!("{error:?}"), "ArtifactRegistrationError");

    for forbidden in [
        "artifact bytes",
        "artifact://",
        "C:\\Users\\example",
        "/home/example",
        "secret",
    ] {
        assert!(!error.to_string().contains(forbidden));
        assert!(!format!("{error:?}").contains(forbidden));
    }
}
