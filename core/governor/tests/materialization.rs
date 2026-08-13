use std::{
    collections::BTreeMap,
    future::Future,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
    thread,
};

use chrono::{TimeZone, Utc};
use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, EventRepositoryError, EvidenceOpener,
    EvidenceProtector, EvidenceRead, EvidenceRepository, KeyError, KeyProvider,
    KeyProviderMetadata, RepositoryFuture, RevocationReceipt, RevokeKeyRequest, SealedEvidence,
    SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
};
use graphhelm_governor::{
    ExecutableGraphMaterializer, GraphExternalizer, MaterializationError, MaterializedContent,
    SealingGraphExternalizer,
};
use graphhelm_graph::{GraphVersion, raw_content_sha256};
use graphhelm_protocols::{
    Actor, ActorType, ContentSlot, EvidenceId, ExecutionId, PersistedGraphVersion, ProjectId,
    RepositoryScope, WorkspaceId,
};

fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match Pin::as_mut(&mut future).poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

#[derive(Clone, Default)]
struct Keys(Arc<Mutex<BTreeMap<String, Vec<u8>>>>);
impl KeyProvider for Keys {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async { KeyProviderMetadata::new("key-1", "test", "1.0.0", 0) })
    }
    fn wrap<'a>(
        &'a self,
        request: WrapKeyRequest,
    ) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async move {
            let handle = request.handle().to_owned();
            self.0.lock().unwrap().insert(
                handle.clone(),
                request.plaintext_key().expose(|v| v.to_vec()),
            );
            WrappedKey::new(
                "key-1",
                handle,
                "xchacha20poly1305",
                vec![7; 24],
                vec![9; 48],
                raw_content_sha256(request.aad()).unwrap(),
            )
        })
    }
    fn unwrap<'a>(
        &'a self,
        wrapped: WrappedKey,
    ) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async move {
            self.0
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
        _: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn authenticate<'a>(
        &'a self,
        _: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn verify<'a>(
        &'a self,
        _: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
}

struct EvidenceRows {
    rows: BTreeMap<EvidenceId, SealedEvidence>,
    unavailable: Option<EvidenceId>,
}
impl EvidenceRepository for EvidenceRows {
    fn get_sealed<'a>(
        &'a self,
        _: RepositoryScope,
        id: EvidenceId,
    ) -> RepositoryFuture<'a, Result<EvidenceRead, EventRepositoryError>> {
        Box::pin(async move {
            if self.unavailable.as_ref() == Some(&id) {
                Ok(EvidenceRead::Unavailable(
                    graphhelm_events::EvidenceUnavailableReason::Erased,
                ))
            } else {
                self.rows
                    .get(&id)
                    .cloned()
                    .map(EvidenceRead::Available)
                    .ok_or(EventRepositoryError::Integrity)
            }
        })
    }
}

struct WrongPlaintext;
impl EvidenceOpener for WrongPlaintext {
    fn open<'a>(
        &'a self,
        _: RepositoryScope,
        _: &'a SealedEvidence,
    ) -> RepositoryFuture<'a, Result<SecretBytes, graphhelm_events::EvidenceError>> {
        Box::pin(async { Ok(SecretBytes::new(b"{}".to_vec())) })
    }
}

struct FailedAuthentication;
impl EvidenceOpener for FailedAuthentication {
    fn open<'a>(
        &'a self,
        _: RepositoryScope,
        _: &'a SealedEvidence,
    ) -> RepositoryFuture<'a, Result<SecretBytes, graphhelm_events::EvidenceError>> {
        Box::pin(async { Err(graphhelm_events::EvidenceError::Invalid) })
    }
}

fn scope() -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("exec_feature").unwrap()),
    )
}

fn preparation(keys: Keys) -> graphhelm_governor::ProjectionPreparation {
    let graph = graphhelm_schema::load_graph(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/graphs/software-feature.yaml"),
    )
    .unwrap()
    .graph;
    let record = GraphVersion::publish(
        graph,
        None,
        Actor::new(ActorType::Owner, "owner-test"),
        Utc.with_ymd_and_hms(2026, 8, 11, 12, 0, 0).unwrap(),
    )
    .unwrap()
    .to_record();
    block_on(SealingGraphExternalizer::new(EvidenceProtector::new(keys)).prepare(scope(), &record))
        .unwrap()
}

#[test]
fn required_unavailable_content_has_the_stable_ghe008_diagnostic() {
    assert_eq!(
        MaterializationError::ContentUnavailable.code(),
        "GHE012_CONTENT_UNAVAILABLE"
    );
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let required = prepared
        .version()
        .content_slots()
        .iter()
        .find(|slot| slot.required_for_execution())
        .unwrap()
        .evidence_id()
        .clone();
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: Some(required),
    };
    let materializer = ExecutableGraphMaterializer::new(
        Arc::new(rows),
        Arc::new(EvidenceProtector::new(keys)) as Arc<dyn EvidenceOpener>,
    );
    assert!(matches!(
        block_on(materializer.materialize(scope(), prepared.version())),
        Err(MaterializationError::ContentUnavailable)
    ));
}

#[test]
fn successful_materialization_opens_canonical_content_without_fallback() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    let materializer =
        ExecutableGraphMaterializer::new(Arc::new(rows), Arc::new(EvidenceProtector::new(keys)));
    let result = block_on(materializer.materialize(scope(), prepared.version())).unwrap();
    assert_eq!(
        result.content().len(),
        prepared.version().content_slots().len()
    );
    assert!(
        result
            .content()
            .values()
            .all(|item| matches!(item, MaterializedContent::Available(_)))
    );
    let first = result.content().values().next().unwrap();
    let MaterializedContent::Available(value) = first else {
        panic!("expected available content")
    };
    assert!(value.expose_json(|json| !json.is_null()).unwrap());
    let debug = format!("{result:?}");
    assert!(!debug.contains("Mapear reposit"));
    assert!(debug.contains("[redacted]"));
}

#[test]
fn optional_unavailable_content_remains_explicit() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let optional = prepared
        .version()
        .content_slots()
        .iter()
        .find(|slot| !slot.required_for_execution())
        .unwrap();
    let optional_id = optional.evidence_id().clone();
    let optional_slot = optional.slot_id().clone();
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: Some(optional_id),
    };
    let result = block_on(
        ExecutableGraphMaterializer::new(Arc::new(rows), Arc::new(EvidenceProtector::new(keys)))
            .materialize(scope(), prepared.version()),
    )
    .unwrap();
    assert!(matches!(
        result.content_for(&optional_slot),
        Some(MaterializedContent::Unavailable(
            graphhelm_events::EvidenceUnavailableReason::Erased
        ))
    ));
}

#[test]
fn decrypted_digest_mismatch_fails_integrity() {
    let keys = Keys::default();
    let prepared = preparation(keys);
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    let result = block_on(
        ExecutableGraphMaterializer::new(Arc::new(rows), Arc::new(WrongPlaintext))
            .materialize(scope(), prepared.version()),
    );
    assert!(matches!(result, Err(MaterializationError::Integrity)));
}

#[test]
fn wrong_scope_is_rejected_before_evidence_can_supply_content() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let rows = EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    let foreign = RepositoryScope::new(
        WorkspaceId::parse("workspace-test").unwrap(),
        ProjectId::parse("project-test").unwrap(),
        Some(ExecutionId::parse("execution-foreign").unwrap()),
    );
    assert!(matches!(
        block_on(
            ExecutableGraphMaterializer::new(
                Arc::new(rows),
                Arc::new(EvidenceProtector::new(keys)),
            )
            .materialize(foreign, prepared.version())
        ),
        Err(MaterializationError::Integrity)
    ));
}

#[test]
fn owner_ordinal_mutation_and_decrypt_authentication_failure_are_rejected() {
    let keys = Keys::default();
    let prepared = preparation(keys.clone());
    let mut slots = prepared.version().content_slots().to_vec();
    let original = &slots[0];
    slots[0] = ContentSlot::new(
        original.slot_id().clone(),
        original.owner_kind(),
        original.owner_id().clone(),
        original.field_kind(),
        original.ordinal() + 1,
        original.evidence_id().clone(),
        original.content_sha256().clone(),
        original.sensitivity(),
        original.required_for_execution(),
    );
    let mutated = PersistedGraphVersion::new(
        prepared.version().number(),
        prepared.version().predecessor().cloned(),
        prepared.version().topology().clone(),
        prepared.version().topology_hash().clone(),
        prepared.version().semantic_hash().clone(),
        slots,
        prepared.version().created_by().clone(),
        prepared.version().created_at().clone(),
    )
    .unwrap();
    let rows = || EvidenceRows {
        rows: prepared
            .evidence()
            .iter()
            .cloned()
            .map(|value| (value.reference().evidence_id().clone(), value))
            .collect(),
        unavailable: None,
    };
    assert!(matches!(
        block_on(
            ExecutableGraphMaterializer::new(
                Arc::new(rows()),
                Arc::new(EvidenceProtector::new(keys)),
            )
            .materialize(scope(), &mutated)
        ),
        Err(MaterializationError::Integrity)
    ));
    assert!(matches!(
        block_on(
            ExecutableGraphMaterializer::new(Arc::new(rows()), Arc::new(FailedAuthentication))
                .materialize(scope(), prepared.version())
        ),
        Err(MaterializationError::Integrity)
    ));
}

#[test]
fn integrity_failures_never_collapse_into_content_unavailable() {
    assert_eq!(
        MaterializationError::Integrity.code(),
        "GHE005_INTEGRITY_FAILURE"
    );
}
