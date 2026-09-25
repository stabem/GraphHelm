use std::{
    collections::BTreeMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread,
};

use graphhelm_events::{
    AuthenticateRequest, AuthenticationTag, CleanupReceipt, EvidencePriorAvailability,
    FinalizedRetention, KeyError, KeyProvider, KeyProviderMetadata, LegalHoldChange,
    PreparedRetention, RepositoryFuture, RetentionAuthority, RetentionBlockReason, RetentionClock,
    RetentionError, RetentionPlan, RetentionPlanTarget, RetentionPolicy, RetentionRepository,
    RetentionRequest, RetentionService, RetentionTarget, RevocationReceipt, RevokeKeyRequest,
    SecretBytes, VerifyAuthenticationRequest, WrapKeyRequest, WrappedKey,
    provider_revocation_idempotency_key, retention_request_digest,
};
use static_assertions::assert_obj_safe;

assert_obj_safe!(RetentionRepository);

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

fn assert_public_saga_types_are_send_sync() {
    fn require<T: Send + Sync>() {}
    require::<PreparedRetention>();
    require::<FinalizedRetention>();
    require::<LegalHoldChange>();
    require::<CleanupReceipt>();
    require::<RetentionService>();
}
use graphhelm_protocols::{
    EvidenceId, ExecutionId, OpaqueId, PersistedTimestamp, ProjectId, RawSha256, RepositoryScope,
    SafeCode, SemanticVersion, Sensitivity, WorkspaceId,
};

fn scope(name: &str) -> RepositoryScope {
    RepositoryScope::new(
        WorkspaceId::parse(format!("ws-{name}")).unwrap(),
        ProjectId::parse(format!("prj-{name}")).unwrap(),
        Some(ExecutionId::parse(format!("exec-{name}")).unwrap()),
    )
}

#[test]
fn dry_run_plan_is_ordered_and_contains_only_safe_metadata() {
    assert_public_saga_types_are_send_sync();
    let plan = RetentionPlan::new(
        OpaqueId::parse("operation-1").unwrap(),
        RawSha256::parse("11".repeat(32)).unwrap(),
        PersistedTimestamp::parse("2026-08-11T12:00:00Z").unwrap(),
        vec![
            RetentionPlanTarget::new(
                EvidenceId::parse("evidence-z").unwrap(),
                OpaqueId::parse("handle-z").unwrap(),
                RawSha256::parse("22".repeat(32)).unwrap(),
                Sensitivity::Restricted,
                EvidencePriorAvailability::Available,
                None,
            )
            .unwrap(),
            RetentionPlanTarget::new(
                EvidenceId::parse("evidence-a").unwrap(),
                OpaqueId::parse("handle-a").unwrap(),
                RawSha256::parse("33".repeat(32)).unwrap(),
                Sensitivity::Restricted,
                EvidencePriorAvailability::Expired,
                Some(RetentionBlockReason::LegalHold),
            )
            .unwrap(),
        ],
    )
    .unwrap();

    assert_eq!(plan.targets()[0].evidence_id().as_str(), "evidence-a");
    assert_eq!(plan.targets()[1].evidence_id().as_str(), "evidence-z");
    assert!(!plan.is_eligible());
    let debug = format!("{plan:?}");
    assert!(!debug.contains("C:\\"));
    assert!(!debug.contains("plaintext"));
    assert!(!debug.contains("handle-a"));
    assert!(!debug.contains(&"33".repeat(32)));
}

fn target(id: &str) -> RetentionTarget {
    RetentionTarget::new(EvidenceId::parse(id).unwrap()).unwrap()
}

fn test_auth_tag() -> AuthenticationTag {
    AuthenticationTag::new("test-key", "hmac-sha256", vec![7; 32]).unwrap()
}

fn request() -> RetentionRequest {
    let policy = RetentionPolicy::new(
        OpaqueId::parse("policy-standard").unwrap(),
        SemanticVersion::parse("1.0.0").unwrap(),
        "standard",
        0,
        0,
    )
    .unwrap();
    RetentionRequest::new(
        scope("alpha"),
        OpaqueId::parse("operation-1").unwrap(),
        OpaqueId::parse("retention-request-1").unwrap(),
        policy.clone(),
        RetentionAuthority::new(
            OpaqueId::parse("authority-compliance").unwrap(),
            scope("alpha"),
            policy.id().clone(),
            policy.version().clone(),
            test_auth_tag(),
        ),
        SafeCode::parse("scheduled_expiry").unwrap(),
        vec![target("evidence-a")],
    )
    .unwrap()
}

#[derive(Clone)]
struct FixedClock(PersistedTimestamp);
impl RetentionClock for FixedClock {
    fn now(&self) -> PersistedTimestamp {
        self.0.clone()
    }
}

#[derive(Default)]
struct SagaState {
    prepared: Option<PreparedRetention>,
    finalized: Option<FinalizedRetention>,
}

struct FakeRepository {
    state: Mutex<SagaState>,
    fail_finalize_once: AtomicBool,
}
impl Default for FakeRepository {
    fn default() -> Self {
        Self {
            state: Mutex::new(SagaState::default()),
            fail_finalize_once: AtomicBool::new(false),
        }
    }
}

impl FakeRepository {
    fn pending(&self) -> bool {
        self.state.lock().unwrap().prepared.is_some()
            && self.state.lock().unwrap().finalized.is_none()
    }
}

impl RetentionRepository for FakeRepository {
    fn dry_run<'a>(
        &'a self,
        request: RetentionRequest,
        evaluated_at: PersistedTimestamp,
    ) -> RepositoryFuture<'a, Result<RetentionPlan, RetentionError>> {
        Box::pin(async move {
            let state = self.state.lock().unwrap();
            if let Some(existing) = state.prepared.as_ref() {
                if existing.request() != &request {
                    return Err(RetentionError::Conflict);
                }
                return Ok(existing.plan().clone());
            }
            RetentionPlan::new(
                request.operation_id().clone(),
                retention_request_digest(&request),
                evaluated_at,
                request
                    .targets()
                    .iter()
                    .map(|target| {
                        RetentionPlanTarget::new(
                            target.evidence_id().clone(),
                            OpaqueId::parse(format!("handle-{}", target.evidence_id().as_str()))
                                .unwrap(),
                            RawSha256::parse("22".repeat(32)).unwrap(),
                            Sensitivity::Restricted,
                            EvidencePriorAvailability::Available,
                            None,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        })
    }
    fn prepare<'a>(
        &'a self,
        prepared: PreparedRetention,
    ) -> RepositoryFuture<'a, Result<graphhelm_events::RetentionPrepareOutcome, RetentionError>>
    {
        Box::pin(async move {
            let mut state = self.state.lock().unwrap();
            if let Some(finalized) = state.finalized.clone() {
                return Ok(graphhelm_events::RetentionPrepareOutcome::Finalized(
                    finalized,
                ));
            }
            if let Some(existing) = state.prepared.clone() {
                if existing.request() != prepared.request() {
                    return Err(RetentionError::Conflict);
                }
                return Ok(graphhelm_events::RetentionPrepareOutcome::Prepared(
                    existing,
                ));
            }
            state.prepared = Some(prepared.clone());
            Ok(graphhelm_events::RetentionPrepareOutcome::Prepared(
                prepared,
            ))
        })
    }
    fn finalize<'a>(
        &'a self,
        prepared: PreparedRetention,
        receipts: Vec<RevocationReceipt>,
        completed_at: PersistedTimestamp,
        authentication_tag: AuthenticationTag,
    ) -> RepositoryFuture<'a, Result<FinalizedRetention, RetentionError>> {
        Box::pin(async move {
            if self.fail_finalize_once.swap(false, Ordering::SeqCst) {
                return Err(RetentionError::Storage);
            }
            let mut state = self.state.lock().unwrap();
            if let Some(finalized) = state.finalized.clone() {
                return Ok(finalized);
            }
            if state.prepared.as_ref() != Some(&prepared) {
                return Err(RetentionError::Conflict);
            }
            let finalized =
                FinalizedRetention::new(prepared, receipts, completed_at, authentication_tag)?;
            state.finalized = Some(finalized.clone());
            Ok(finalized)
        })
    }
    fn pending<'a>(
        &'a self,
        _scope: RepositoryScope,
        limit: u32,
    ) -> RepositoryFuture<'a, Result<Vec<PreparedRetention>, RetentionError>> {
        Box::pin(async move {
            if limit == 0 {
                return Err(RetentionError::LimitExceeded);
            }
            let state = self.state.lock().unwrap();
            Ok(if state.finalized.is_none() {
                state.prepared.clone().into_iter().collect()
            } else {
                vec![]
            })
        })
    }
    fn change_legal_hold<'a>(
        &'a self,
        _change: LegalHoldChange,
    ) -> RepositoryFuture<'a, Result<graphhelm_events::LegalHoldReceipt, RetentionError>> {
        Box::pin(async { Err(RetentionError::LegalHold) })
    }
    fn cleanup<'a>(
        &'a self,
        _request: graphhelm_events::CleanupRequest,
    ) -> RepositoryFuture<'a, Result<CleanupReceipt, RetentionError>> {
        Box::pin(async { Err(RetentionError::Ineligible) })
    }
}

struct FakeKeyProvider {
    repository: Arc<FakeRepository>,
    fail_once: AtomicBool,
    calls: AtomicUsize,
    logical_revokes: AtomicUsize,
    receipts: Mutex<BTreeMap<String, RevocationReceipt>>,
    verify_calls: AtomicUsize,
    fail_receipt_verify: AtomicBool,
}
impl FakeKeyProvider {
    fn new(repository: Arc<FakeRepository>) -> Self {
        Self {
            repository,
            fail_once: AtomicBool::new(true),
            calls: AtomicUsize::new(0),
            logical_revokes: AtomicUsize::new(0),
            receipts: Mutex::new(BTreeMap::new()),
            verify_calls: AtomicUsize::new(0),
            fail_receipt_verify: AtomicBool::new(false),
        }
    }
}
impl KeyProvider for FakeKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async move {
            KeyProviderMetadata::new(
                "test-key",
                "test",
                "1",
                u64::try_from(self.logical_revokes.load(Ordering::SeqCst)).unwrap(),
            )
        })
    }
    fn wrap<'a>(&'a self, _: WrapKeyRequest) -> RepositoryFuture<'a, Result<WrappedKey, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn unwrap<'a>(&'a self, _: WrappedKey) -> RepositoryFuture<'a, Result<SecretBytes, KeyError>> {
        Box::pin(async { Err(KeyError::Unavailable) })
    }
    fn revoke<'a>(
        &'a self,
        request: RevokeKeyRequest,
    ) -> RepositoryFuture<'a, Result<RevocationReceipt, KeyError>> {
        Box::pin(async move {
            assert!(
                self.repository.pending(),
                "revoke must happen after durable pending"
            );
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(receipt) = self
                .receipts
                .lock()
                .unwrap()
                .get(request.idempotency_key())
                .cloned()
            {
                return Ok(receipt);
            }
            if self.fail_once.swap(false, Ordering::SeqCst) {
                return Err(KeyError::Unavailable);
            }
            let epoch = self.logical_revokes.fetch_add(1, Ordering::SeqCst) + 1;
            let receipt = RevocationReceipt::new(
                request.handle(),
                request.idempotency_key(),
                u64::try_from(epoch).unwrap(),
                AuthenticationTag::new("test-key", "hmac-sha256", vec![7; 32])?,
            )?;
            self.receipts
                .lock()
                .unwrap()
                .insert(request.idempotency_key().to_owned(), receipt.clone());
            Ok(receipt)
        })
    }
    fn authenticate<'a>(
        &'a self,
        _: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async { AuthenticationTag::new("test-key", "hmac-sha256", vec![7; 32]) })
    }
    fn verify<'a>(
        &'a self,
        _: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async move {
            let call = self.verify_calls.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_receipt_verify.load(Ordering::SeqCst) && call == 2 {
                Err(KeyError::Integrity)
            } else {
                Ok(())
            }
        })
    }
}

#[test]
fn unauthenticated_provider_receipt_never_finalizes() {
    let repository = Arc::new(FakeRepository::default());
    let provider = Arc::new(FakeKeyProvider::new(repository.clone()));
    provider.fail_once.store(false, Ordering::SeqCst);
    provider.fail_receipt_verify.store(true, Ordering::SeqCst);
    let service = RetentionService::new(
        repository.clone(),
        provider,
        Arc::new(FixedClock(
            PersistedTimestamp::parse("2026-08-11T12:00:00Z").unwrap(),
        )),
    );
    assert_eq!(
        block_on(service.execute(request())).unwrap_err(),
        RetentionError::Integrity
    );
    assert!(repository.pending());
}

#[test]
fn multi_target_revocation_uses_one_stable_provider_identity_per_target() {
    let repository = Arc::new(FakeRepository::default());
    let provider = Arc::new(FakeKeyProvider::new(repository.clone()));
    provider.fail_once.store(false, Ordering::SeqCst);
    let service = RetentionService::new(
        repository,
        provider.clone(),
        Arc::new(FixedClock(
            PersistedTimestamp::parse("2026-08-11T12:00:00Z").unwrap(),
        )),
    );
    let single = request();
    let multiple = RetentionRequest::new(
        single.scope().clone(),
        single.operation_id().clone(),
        single.idempotency_key().clone(),
        single.policy().clone(),
        single.authority().clone(),
        single.reason_code().clone(),
        vec![target("evidence-a"), target("evidence-b")],
    )
    .unwrap();

    let finalized = block_on(service.execute(multiple)).unwrap();
    assert_eq!(finalized.receipts().len(), 2);
    assert_ne!(
        finalized.receipts()[0].idempotency_key(),
        finalized.receipts()[1].idempotency_key()
    );
    assert_eq!(provider.logical_revokes.load(Ordering::SeqCst), 2);
    let mut regressing = finalized.receipts().to_vec();
    regressing.reverse();
    assert_eq!(
        FinalizedRetention::new(
            finalized.prepared().clone(),
            regressing,
            finalized.completed_at().clone(),
            test_auth_tag(),
        )
        .unwrap_err(),
        RetentionError::Invalid
    );
}

#[test]
fn provider_revocation_identity_is_scoped_globally() {
    let operation = OpaqueId::parse("operation-1").unwrap();
    let evidence = EvidenceId::parse("evidence-a").unwrap();
    assert_ne!(
        provider_revocation_idempotency_key(&scope("alpha"), &operation, &evidence),
        provider_revocation_idempotency_key(&scope("beta"), &operation, &evidence),
    );
}

#[test]
fn crash_after_provider_revoke_retries_without_a_second_logical_revoke() {
    let repository = Arc::new(FakeRepository::default());
    repository.fail_finalize_once.store(true, Ordering::SeqCst);
    let provider = Arc::new(FakeKeyProvider::new(repository.clone()));
    provider.fail_once.store(false, Ordering::SeqCst);
    let service = RetentionService::new(
        repository.clone(),
        provider.clone(),
        Arc::new(FixedClock(
            PersistedTimestamp::parse("2026-08-11T12:00:00Z").unwrap(),
        )),
    );
    assert_eq!(
        block_on(service.execute(request())).unwrap_err(),
        RetentionError::Storage
    );
    assert!(repository.pending());
    assert_eq!(
        block_on(service.execute(request()))
            .unwrap()
            .receipts()
            .len(),
        1
    );
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(provider.logical_revokes.load(Ordering::SeqCst), 1);
}

#[test]
fn provider_failure_stays_pending_and_exact_retry_finalizes_once() {
    let repository = Arc::new(FakeRepository::default());
    let provider = Arc::new(FakeKeyProvider::new(repository.clone()));
    let service = RetentionService::new(
        repository.clone(),
        provider.clone(),
        Arc::new(FixedClock(
            PersistedTimestamp::parse("2026-08-11T12:00:00Z").unwrap(),
        )),
    );

    assert_eq!(
        block_on(service.execute(request())).unwrap_err(),
        RetentionError::KeyUnavailable
    );
    assert!(repository.pending());
    let finalized = block_on(service.execute(request())).unwrap();
    assert_eq!(finalized.receipts().len(), 1);
    assert_eq!(finalized.authentication_tag().algorithm(), "hmac-sha256");
    assert!(!repository.pending());
    let inconsistent_receipt = RevocationReceipt::new(
        "different-handle",
        finalized.receipts()[0].idempotency_key(),
        finalized.receipts()[0].epoch(),
        test_auth_tag(),
    )
    .unwrap();
    assert_eq!(
        FinalizedRetention::new(
            finalized.prepared().clone(),
            vec![inconsistent_receipt],
            finalized.completed_at().clone(),
            test_auth_tag(),
        )
        .unwrap_err(),
        RetentionError::Invalid
    );
    let exact = block_on(service.execute(request())).unwrap();
    assert_eq!(exact, finalized);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert_eq!(provider.logical_revokes.load(Ordering::SeqCst), 1);
    let mut divergent = request();
    divergent = RetentionRequest::new(
        divergent.scope().clone(),
        divergent.operation_id().clone(),
        divergent.idempotency_key().clone(),
        divergent.policy().clone(),
        divergent.authority().clone(),
        divergent.reason_code().clone(),
        vec![target("evidence-b")],
    )
    .unwrap();
    assert_eq!(
        block_on(service.execute(divergent)).unwrap_err(),
        RetentionError::Conflict
    );
    assert_eq!(provider.logical_revokes.load(Ordering::SeqCst), 1);
    let hold = LegalHoldChange::new(
        scope("alpha"),
        OpaqueId::parse("hold-1").unwrap(),
        EvidenceId::parse("evidence-a").unwrap(),
        OpaqueId::parse("authority-compliance").unwrap(),
        SafeCode::parse("legal_request").unwrap(),
        true,
        PersistedTimestamp::parse("2026-08-11T12:00:00Z").unwrap(),
        test_auth_tag(),
    );
    assert_eq!(
        block_on(service.change_legal_hold(hold)).unwrap_err(),
        RetentionError::LegalHold
    );
}

#[test]
fn retention_request_is_bounded_and_canonicalizes_target_order() {
    let policy = RetentionPolicy::new(
        OpaqueId::parse("policy-standard").unwrap(),
        SemanticVersion::parse("1.0.0").unwrap(),
        "standard",
        86_400,
        3_600,
    )
    .unwrap();
    let authority = RetentionAuthority::new(
        OpaqueId::parse("authority-compliance").unwrap(),
        scope("alpha"),
        policy.id().clone(),
        policy.version().clone(),
        test_auth_tag(),
    );
    let request = RetentionRequest::new(
        scope("alpha"),
        OpaqueId::parse("operation-1").unwrap(),
        OpaqueId::parse("retention-request-1").unwrap(),
        policy,
        authority,
        SafeCode::parse("scheduled_expiry").unwrap(),
        vec![target("evidence-z"), target("evidence-a")],
    )
    .unwrap();

    assert_eq!(request.targets()[0].evidence_id().as_str(), "evidence-a");
    assert_eq!(request.targets()[1].evidence_id().as_str(), "evidence-z");

    let wrong_scope_authority = RetentionAuthority::new(
        OpaqueId::parse("authority-compliance").unwrap(),
        scope("foreign"),
        request.policy().id().clone(),
        request.policy().version().clone(),
        test_auth_tag(),
    );
    assert_eq!(
        RetentionRequest::new(
            scope("alpha"),
            OpaqueId::parse("operation-foreign").unwrap(),
            OpaqueId::parse("retention-request-foreign").unwrap(),
            request.policy().clone(),
            wrong_scope_authority,
            SafeCode::parse("scheduled_expiry").unwrap(),
            vec![target("evidence-a")],
        )
        .unwrap_err(),
        RetentionError::Scope
    );
    let wrong_version_authority = RetentionAuthority::new(
        OpaqueId::parse("authority-compliance").unwrap(),
        scope("alpha"),
        request.policy().id().clone(),
        SemanticVersion::parse("2.0.0").unwrap(),
        test_auth_tag(),
    );
    assert_eq!(
        RetentionRequest::new(
            scope("alpha"),
            OpaqueId::parse("operation-version").unwrap(),
            OpaqueId::parse("retention-request-version").unwrap(),
            request.policy().clone(),
            wrong_version_authority,
            SafeCode::parse("scheduled_expiry").unwrap(),
            vec![target("evidence-a")],
        )
        .unwrap_err(),
        RetentionError::Scope
    );

    let too_many = (0..=10_000)
        .map(|index| target(&format!("evidence-{index}")))
        .collect();
    assert!(
        RetentionRequest::new(
            scope("alpha"),
            OpaqueId::parse("operation-2").unwrap(),
            OpaqueId::parse("retention-request-2").unwrap(),
            RetentionPolicy::new(
                OpaqueId::parse("policy-standard").unwrap(),
                SemanticVersion::parse("1.0.0").unwrap(),
                "standard",
                86_400,
                3_600,
            )
            .unwrap(),
            RetentionAuthority::new(
                OpaqueId::parse("authority-compliance").unwrap(),
                scope("alpha"),
                OpaqueId::parse("policy-standard").unwrap(),
                SemanticVersion::parse("1.0.0").unwrap(),
                test_auth_tag(),
            ),
            SafeCode::parse("scheduled_expiry").unwrap(),
            too_many,
        )
        .is_err()
    );
}
