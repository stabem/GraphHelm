mod support;

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use chrono::Utc;
use graphhelm_events::{
    AsyncEventRepository, AuthenticateRequest, AuthenticationTag, CleanupRequest, EvidenceRead,
    EvidenceRepository, KeyError, KeyProvider, KeyProviderMetadata, LegalHoldChange,
    PreparedAppend, PreparedRetention, RepositoryFuture, RetentionAuthority, RetentionClock,
    RetentionError, RetentionPolicy, RetentionRepository, RetentionRequest, RetentionService,
    RetentionTarget, RevocationReceipt, RevokeKeyRequest, SecretBytes, VerifyAuthenticationRequest,
    WrapKeyRequest, WrappedKey, legal_hold_authentication_bytes,
    retention_authority_authentication_bytes,
};
use graphhelm_postgres_event_store::PostgresEventStore;
use graphhelm_protocols::{
    ExecutionId, OpaqueId, PersistedTimestamp, ProjectId, RepositoryScope, SafeCode,
    SemanticVersion, WorkspaceId,
};
use sha2::{Digest, Sha256};
use sqlx::Row;

struct Clock(PersistedTimestamp);
impl RetentionClock for Clock {
    fn now(&self) -> PersistedTimestamp {
        self.0.clone()
    }
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn forged_prepared_receipt_is_rejected_before_pending_state() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let evidence_scope = RepositoryScope::new(
            WorkspaceId::parse("ws-retention").unwrap(),
            ProjectId::parse("prj-retention").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&evidence_scope, "key-forged");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database
            .repository
            .append_atomic(
                PreparedAppend::new(
                    evidence_scope.clone(),
                    OpaqueId::parse("stream-forged").unwrap(),
                    1,
                    vec![event],
                    evidence,
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let provider = Arc::new(RetentionKeyProvider::default());
        let store = PostgresEventStore::from_pool(database.runtime_pool.clone(), provider)
            .await
            .unwrap();
        let now = PersistedTimestamp::from_datetime(Utc::now()).unwrap();
        let request = retention_request(
            &evidence_scope,
            evidence_id,
            "forged-operation",
            "forged-idempotency",
        );
        let forged_authority = RetentionAuthority::new(
            request.authority().id().clone(),
            request.scope().clone(),
            request.policy().id().clone(),
            request.policy().version().clone(),
            AuthenticationTag::new("retention-key", "hmac-sha256", vec![0; 32]).unwrap(),
        );
        let unauthorized = RetentionRequest::new(
            request.scope().clone(),
            request.operation_id().clone(),
            request.idempotency_key().clone(),
            request.policy().clone(),
            forged_authority,
            request.reason_code().clone(),
            request.targets().to_vec(),
        )
        .unwrap();
        assert_eq!(
            store.dry_run(unauthorized, now.clone()).await.unwrap_err(),
            RetentionError::Integrity
        );
        let plan = store.dry_run(request.clone(), now.clone()).await.unwrap();
        let forged = PreparedRetention::new(
            request,
            plan,
            now,
            0,
            AuthenticationTag::new("retention-key", "hmac-sha256", vec![0; 32]).unwrap(),
        )
        .unwrap();
        assert_eq!(
            store.prepare(forged).await.unwrap_err(),
            RetentionError::Integrity
        );
        let operations: i64 =
            sqlx::query_scalar("SELECT count(*) FROM graphhelm_retention_operations")
                .fetch_one(database.admin_pool())
                .await
                .unwrap();
        assert_eq!(operations, 0);
        database.cleanup().await;
    });
}

#[derive(Default)]
struct RetentionKeyProvider {
    epoch: AtomicU64,
    fail_once: AtomicBool,
    fail_finalize_auth_once: AtomicBool,
    revoked: Mutex<BTreeMap<String, RevocationReceipt>>,
}
fn tag(bytes: &[u8]) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(b"retention-test-key");
    hash.update(bytes);
    hash.finalize().to_vec()
}
fn receipt_bytes(handle: &str, key: &str, epoch: u64) -> Vec<u8> {
    let mut bytes = b"graphhelm-revocation-receipt-v1".to_vec();
    for value in [handle, key] {
        bytes.extend_from_slice(&u32::try_from(value.len()).unwrap().to_be_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }
    bytes.extend_from_slice(&epoch.to_be_bytes());
    bytes
}
impl KeyProvider for RetentionKeyProvider {
    fn metadata<'a>(&'a self) -> RepositoryFuture<'a, Result<KeyProviderMetadata, KeyError>> {
        Box::pin(async move {
            KeyProviderMetadata::new(
                "retention-key",
                "test",
                "1",
                self.epoch.load(Ordering::SeqCst),
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
            let key = format!("{}:{}", request.handle(), request.idempotency_key());
            if let Some(receipt) = self.revoked.lock().unwrap().get(&key).cloned() {
                return Ok(receipt);
            }
            if self.fail_once.swap(false, Ordering::SeqCst) {
                return Err(KeyError::Unavailable);
            }
            let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
            let receipt = RevocationReceipt::new(
                request.handle(),
                request.idempotency_key(),
                epoch,
                AuthenticationTag::new(
                    "retention-key",
                    "hmac-sha256",
                    tag(&receipt_bytes(
                        request.handle(),
                        request.idempotency_key(),
                        epoch,
                    )),
                )?,
            )?;
            self.revoked.lock().unwrap().insert(key, receipt.clone());
            Ok(receipt)
        })
    }
    fn authenticate<'a>(
        &'a self,
        request: AuthenticateRequest,
    ) -> RepositoryFuture<'a, Result<AuthenticationTag, KeyError>> {
        Box::pin(async move {
            if request.purpose() == "retention-finalized"
                && self.fail_finalize_auth_once.swap(false, Ordering::SeqCst)
            {
                return Err(KeyError::Unavailable);
            }
            AuthenticationTag::new("retention-key", "hmac-sha256", tag(request.bytes()))
        })
    }
    fn verify<'a>(
        &'a self,
        request: VerifyAuthenticationRequest,
    ) -> RepositoryFuture<'a, Result<(), KeyError>> {
        Box::pin(async move {
            if request.tag().bytes() == tag(request.bytes()) {
                Ok(())
            } else {
                Err(KeyError::Integrity)
            }
        })
    }
}

fn retention_request(
    scope: &RepositoryScope,
    evidence_id: graphhelm_protocols::EvidenceId,
    operation: &str,
    idempotency: &str,
) -> RetentionRequest {
    retention_request_with_cleanup_delay(scope, evidence_id, operation, idempotency, 0)
}
fn retention_request_with_cleanup_delay(
    scope: &RepositoryScope,
    evidence_id: graphhelm_protocols::EvidenceId,
    operation: &str,
    idempotency: &str,
    cleanup_delay_seconds: u64,
) -> RetentionRequest {
    let policy = RetentionPolicy::new(
        OpaqueId::parse("policy-standard").unwrap(),
        SemanticVersion::parse("1.0.0").unwrap(),
        "standard",
        0,
        cleanup_delay_seconds,
    )
    .unwrap();
    let authority_id = OpaqueId::parse("authority-compliance").unwrap();
    let authority_tag = AuthenticationTag::new(
        "retention-key",
        "hmac-sha256",
        tag(&retention_authority_authentication_bytes(
            &authority_id,
            scope,
            &policy,
        )),
    )
    .unwrap();
    RetentionRequest::new(
        scope.clone(),
        OpaqueId::parse(operation).unwrap(),
        OpaqueId::parse(idempotency).unwrap(),
        policy.clone(),
        RetentionAuthority::new(
            authority_id,
            scope.clone(),
            policy.id().clone(),
            policy.version().clone(),
            authority_tag,
        ),
        SafeCode::parse("scheduled_expiry").unwrap(),
        vec![RetentionTarget::new(evidence_id).unwrap()],
    )
    .unwrap()
}
fn legal_hold_change(
    scope: RepositoryScope,
    hold_id: OpaqueId,
    evidence_id: graphhelm_protocols::EvidenceId,
    authority: OpaqueId,
    reason_code: SafeCode,
    placed: bool,
    changed_at: PersistedTimestamp,
) -> LegalHoldChange {
    let unsigned = LegalHoldChange::new(
        scope.clone(),
        hold_id.clone(),
        evidence_id.clone(),
        authority.clone(),
        reason_code.clone(),
        placed,
        changed_at.clone(),
        AuthenticationTag::new("retention-key", "hmac-sha256", vec![0; 32]).unwrap(),
    );
    LegalHoldChange::new(
        scope,
        hold_id,
        evidence_id,
        authority,
        reason_code,
        placed,
        changed_at,
        AuthenticationTag::new(
            "retention-key",
            "hmac-sha256",
            tag(&legal_hold_authentication_bytes(&unsigned)),
        )
        .unwrap(),
    )
}
#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn retention_migration_is_complete_scoped_and_forced_rls() {
    support::runtime().block_on(async {
        let harness = support::TestDatabase::new().await;
        let rows = sqlx::query(
            "SELECT c.relname,c.relrowsecurity,c.relforcerowsecurity FROM pg_class c \
             JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' \
             AND c.relname = ANY($1) ORDER BY c.relname",
        )
        .bind(vec![
            "graphhelm_cleanup_receipts",
            "graphhelm_evidence_tombstones",
            "graphhelm_legal_holds",
            "graphhelm_retention_operations",
            "graphhelm_retention_policies",
            "graphhelm_retention_targets",
        ])
        .fetch_all(harness.admin_pool())
        .await
        .unwrap();
        assert_eq!(rows.len(), 6);
        assert!(
            rows.iter().all(|row| row.get::<bool, _>("relrowsecurity")
                && row.get::<bool, _>("relforcerowsecurity"))
        );
        let can_mutate_hold_history: bool = sqlx::query_scalar(
            "SELECT has_table_privilege(current_user,'graphhelm_legal_holds','UPDATE,DELETE,TRUNCATE')",
        )
        .fetch_one(&harness.runtime_pool)
        .await
        .unwrap();
        assert!(!can_mutate_hold_history);
        harness.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn oversized_hold_history_fails_before_authentication_work() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = RepositoryScope::new(
            WorkspaceId::parse("ws-hold-limit").unwrap(),
            ProjectId::parse("prj-hold-limit").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&scope, "key-hold-limit");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database.repository.append_atomic(PreparedAppend::new(scope.clone(),OpaqueId::parse("stream-hold-limit").unwrap(),1,vec![event],evidence,vec![]).unwrap()).await.unwrap();
        sqlx::query("INSERT INTO graphhelm_legal_holds(workspace_id,project_id,execution_id,hold_id,evidence_id,authority,reason_code,placed,changed_at,authentication_key_id,authentication_algorithm,authentication_tag) SELECT $1,$2,$3,'hold-'||g,$4,'authority-legal','legal_request',true,'2026-08-11T00:00:00Z','retention-key','hmac-sha256',decode('00','hex') FROM generate_series(1,20001) g")
            .bind(scope.workspace_id().as_str()).bind(scope.project_id().as_str()).bind(scope.execution_id().unwrap().as_str()).bind(evidence_id.as_str()).execute(database.admin_pool()).await.unwrap();
        let provider=Arc::new(RetentionKeyProvider::default());
        let store=PostgresEventStore::from_pool(database.runtime_pool.clone(),provider).await.unwrap();
        let request=retention_request(&scope,evidence_id,"hold-limit-operation","hold-limit-idempotency");
        assert_eq!(store.dry_run(request,PersistedTimestamp::from_datetime(Utc::now()).unwrap()).await.unwrap_err(),RetentionError::LimitExceeded);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn committed_hold_wins_a_prepare_snapshot_race() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new_with_max_connections(2).await;
        let scope = RepositoryScope::new(
            WorkspaceId::parse("ws-hold-race").unwrap(),
            ProjectId::parse("prj-hold-race").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&scope, "key-hold-race");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database.repository.append_atomic(PreparedAppend::new(scope.clone(),OpaqueId::parse("stream-hold-race").unwrap(),1,vec![event],evidence,vec![]).unwrap()).await.unwrap();
        let provider = Arc::new(RetentionKeyProvider::default());
        let store = Arc::new(PostgresEventStore::from_pool(database.runtime_pool.clone(),provider.clone()).await.unwrap());
        let request = retention_request(&scope,evidence_id.clone(),"race-operation","race-idempotency");
        let service = Arc::new(RetentionService::new(store,provider,Arc::new(Clock(PersistedTimestamp::from_datetime(Utc::now()).unwrap()))));
        let changed_at = PersistedTimestamp::from_datetime(Utc::now()).unwrap();
        let hold = legal_hold_change(scope.clone(),OpaqueId::parse("race-hold").unwrap(),evidence_id.clone(),OpaqueId::parse("authority-legal").unwrap(),SafeCode::parse("legal_request").unwrap(),true,changed_at);

        let mut hold_tx = database.runtime_pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('graphhelm.workspace_id',$1,true),set_config('graphhelm.project_id',$2,true),set_config('graphhelm.execution_id',$3,true)").bind(scope.workspace_id().as_str()).bind(scope.project_id().as_str()).bind(scope.execution_id().unwrap().as_str()).execute(&mut *hold_tx).await.unwrap();
        sqlx::query("SELECT evidence_id FROM graphhelm_evidence WHERE evidence_id=$1 FOR UPDATE").bind(evidence_id.as_str()).fetch_one(&mut *hold_tx).await.unwrap();
        let running = {
            let service = service.clone();
            let request = request.clone();
            tokio::spawn(async move { service.execute(request).await })
        };
        let mut blocked = false;
        for _ in 0..200 {
            blocked = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%graphhelm_evidence%')").fetch_one(database.admin_pool()).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if !blocked {
            hold_tx.rollback().await.unwrap();
            panic!("prepare did not reach the Evidence row lock: {:?}",running.await.unwrap());
        }
        sqlx::query("UPDATE graphhelm_evidence SET hold_revision=hold_revision+1 WHERE evidence_id=$1").bind(evidence_id.as_str()).execute(&mut *hold_tx).await.unwrap();
        sqlx::query("INSERT INTO graphhelm_legal_holds(workspace_id,project_id,execution_id,hold_id,evidence_id,authority,reason_code,placed,changed_at,authentication_key_id,authentication_algorithm,authentication_tag) VALUES($1,$2,$3,$4,$5,$6,$7,true,$8,$9,$10,$11)")
            .bind(scope.workspace_id().as_str()).bind(scope.project_id().as_str()).bind(scope.execution_id().unwrap().as_str()).bind(hold.hold_id().as_str()).bind(evidence_id.as_str()).bind(hold.authority().as_str()).bind(hold.reason_code().as_str()).bind(hold.changed_at().as_datetime().to_rfc3339_opts(chrono::SecondsFormat::AutoSi,true)).bind(hold.authentication_tag().key_id()).bind(hold.authentication_tag().algorithm()).bind(hold.authentication_tag().bytes()).execute(&mut *hold_tx).await.unwrap();
        hold_tx.commit().await.unwrap();
        // Losing this race is retryable contention, not a storage fault: the very next line shows
        // the retry reaching the correct terminal answer. Reporting it as `Storage` told callers a
        // recoverable operation had failed permanently.
        assert_eq!(running.await.unwrap().unwrap_err(),RetentionError::Conflict);
        assert_eq!(service.execute(request).await.unwrap_err(),RetentionError::LegalHold);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn concurrent_prepare_finalize_has_one_durable_outcome() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new_with_max_connections(4).await;
        let scope = RepositoryScope::new(
            WorkspaceId::parse("ws-retention-race").unwrap(),
            ProjectId::parse("prj-retention-race").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&scope, "key-retention-race");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database.repository.append_atomic(PreparedAppend::new(scope.clone(),OpaqueId::parse("stream-retention-race").unwrap(),1,vec![event],evidence,vec![]).unwrap()).await.unwrap();
        let provider = Arc::new(RetentionKeyProvider::default());
        let store = Arc::new(PostgresEventStore::from_pool(database.runtime_pool.clone(),provider.clone()).await.unwrap());
        let request = retention_request(&scope,evidence_id,"concurrent-operation","concurrent-idempotency");
        let service = Arc::new(RetentionService::new(store,provider.clone(),Arc::new(Clock(PersistedTimestamp::from_datetime(Utc::now()).unwrap()))));
        let first = { let service=service.clone(); let request=request.clone(); tokio::spawn(async move { service.execute(request).await }) };
        let second = { let service=service.clone(); let request=request.clone(); tokio::spawn(async move { service.execute(request).await }) };
        let outcomes = [first.await.unwrap(),second.await.unwrap()];
        assert!(outcomes.iter().any(Result::is_ok));
        assert!(outcomes.iter().filter_map(|outcome| outcome.as_ref().err()).all(|error| matches!(error,RetentionError::Storage|RetentionError::Conflict)));
        let durable = service.execute(request).await.unwrap();
        assert!(outcomes.iter().filter_map(|outcome| outcome.as_ref().ok()).all(|outcome| outcome==&durable));
        assert_eq!(provider.epoch.load(Ordering::SeqCst),1);
        let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM graphhelm_retention_operations),(SELECT count(*) FROM graphhelm_retention_targets),(SELECT count(*) FROM graphhelm_evidence_tombstones)").fetch_one(database.admin_pool()).await.unwrap();
        assert_eq!(counts,(1,1,1));
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn recreated_service_reconciles_revoked_but_unfinalized_operation() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = RepositoryScope::new(
            WorkspaceId::parse("ws-revoke-crash").unwrap(),
            ProjectId::parse("prj-revoke-crash").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&scope, "key-revoke-crash");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database.repository.append_atomic(PreparedAppend::new(scope.clone(),OpaqueId::parse("stream-revoke-crash").unwrap(),1,vec![event],evidence,vec![]).unwrap()).await.unwrap();
        let provider = Arc::new(RetentionKeyProvider::default());
        provider.fail_finalize_auth_once.store(true,Ordering::SeqCst);
        let store = Arc::new(PostgresEventStore::from_pool(database.runtime_pool.clone(),provider.clone()).await.unwrap());
        let request = retention_request(&scope,evidence_id,"revoke-crash-operation","revoke-crash-idempotency");
        let service = RetentionService::new(store,provider.clone(),Arc::new(Clock(PersistedTimestamp::from_datetime(Utc::now()).unwrap())));
        assert_eq!(service.execute(request).await.unwrap_err(),RetentionError::KeyUnavailable);
        assert_eq!(provider.epoch.load(Ordering::SeqCst),1);
        let recreated_store=Arc::new(PostgresEventStore::from_pool(database.runtime_pool.clone(),provider.clone()).await.unwrap());
        let recreated=RetentionService::new(recreated_store,provider.clone(),Arc::new(Clock(PersistedTimestamp::from_datetime(Utc::now()).unwrap())));
        let finalized=recreated.reconcile(scope,10).await.unwrap();
        assert_eq!(finalized.len(),1);
        assert_eq!(provider.epoch.load(Ordering::SeqCst),1);
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM graphhelm_retention_operations WHERE state='finalized'),(SELECT count(*) FROM graphhelm_evidence_tombstones)").fetch_one(database.admin_pool()).await.unwrap();
        assert_eq!(counts,(1,1));
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn nonzero_cleanup_delay_blocks_early_physical_deletion() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = RepositoryScope::new(
            WorkspaceId::parse("ws-cleanup-delay").unwrap(),
            ProjectId::parse("prj-cleanup-delay").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&scope, "key-cleanup-delay");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database
            .repository
            .append_atomic(
                PreparedAppend::new(
                    scope.clone(),
                    OpaqueId::parse("stream-cleanup-delay").unwrap(),
                    1,
                    vec![event],
                    evidence,
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let provider = Arc::new(RetentionKeyProvider::default());
        let store = Arc::new(
            PostgresEventStore::from_pool(database.runtime_pool.clone(), provider.clone())
                .await
                .unwrap(),
        );
        let now = PersistedTimestamp::from_datetime(Utc::now()).unwrap();
        let service = RetentionService::new(store.clone(), provider, Arc::new(Clock(now.clone())));
        let finalized = service
            .execute(retention_request_with_cleanup_delay(
                &scope,
                evidence_id.clone(),
                "cleanup-delay-operation",
                "cleanup-delay-idempotency",
                3600,
            ))
            .await
            .unwrap();
        let cleanup = CleanupRequest::new(
            scope,
            OpaqueId::parse("cleanup-too-early").unwrap(),
            OpaqueId::parse("cleanup-too-early-idempotency").unwrap(),
            vec![evidence_id.clone()],
            now,
        )
        .unwrap();
        assert_eq!(
            store.cleanup(cleanup).await.unwrap_err(),
            RetentionError::Ineligible
        );
        let record: serde_json::Value =
            sqlx::query_scalar("SELECT record FROM graphhelm_evidence WHERE evidence_id=$1")
                .bind(evidence_id.as_str())
                .fetch_one(database.admin_pool())
                .await
                .unwrap();
        assert_ne!(record, serde_json::json!({}));
        assert_eq!(
            finalized
                .prepared()
                .request()
                .policy()
                .cleanup_delay_seconds(),
            3600
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn restart_reconciles_pending_and_late_hold_cannot_resurrect() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let evidence_scope = RepositoryScope::new(
            WorkspaceId::parse("ws-reconcile").unwrap(),
            ProjectId::parse("prj-reconcile").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&evidence_scope, "key-reconcile");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database
            .repository
            .append_atomic(
                PreparedAppend::new(
                    evidence_scope.clone(),
                    OpaqueId::parse("stream-reconcile").unwrap(),
                    1,
                    vec![event],
                    evidence,
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let provider = Arc::new(RetentionKeyProvider::default());
        provider.fail_once.store(true, Ordering::SeqCst);
        let store = Arc::new(
            PostgresEventStore::from_pool(database.runtime_pool.clone(), provider.clone())
                .await
                .unwrap(),
        );
        let now = PersistedTimestamp::from_datetime(Utc::now()).unwrap();
        let service = RetentionService::new(
            store.clone(),
            provider.clone(),
            Arc::new(Clock(now.clone())),
        );
        let request = retention_request(
            &evidence_scope,
            evidence_id.clone(),
            "reconcile-operation",
            "reconcile-idempotency",
        );
        assert_eq!(
            service.execute(request).await.unwrap_err(),
            RetentionError::KeyUnavailable
        );
        assert_eq!(
            store
                .get_sealed(evidence_scope.clone(), evidence_id.clone())
                .await
                .unwrap(),
            EvidenceRead::Unavailable(graphhelm_events::EvidenceUnavailableReason::ErasurePending)
        );
        let late_hold = legal_hold_change(
            evidence_scope.clone(),
            OpaqueId::parse("hold-late").unwrap(),
            evidence_id.clone(),
            OpaqueId::parse("authority-legal").unwrap(),
            SafeCode::parse("legal_request").unwrap(),
            true,
            now.clone(),
        );
        assert_eq!(
            service.change_legal_hold(late_hold).await.unwrap_err(),
            RetentionError::Conflict
        );
        drop(service);
        drop(store);
        let restarted = Arc::new(
            PostgresEventStore::from_pool(database.runtime_pool.clone(), provider.clone())
                .await
                .unwrap(),
        );
        let restarted_service =
            RetentionService::new(restarted.clone(), provider.clone(), Arc::new(Clock(now)));
        assert_eq!(
            restarted_service
                .reconcile(evidence_scope.clone(), 10)
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            restarted
                .get_sealed(evidence_scope, evidence_id)
                .await
                .unwrap(),
            EvidenceRead::Unavailable(graphhelm_events::EvidenceUnavailableReason::Erased)
        );
        assert_eq!(provider.epoch.load(Ordering::SeqCst), 1);
        let events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM graphhelm_events WHERE stream_id='retention-events'",
        )
        .fetch_one(database.admin_pool())
        .await
        .unwrap();
        assert_eq!(events, 2);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn committed_hold_blocks_prepare_and_release_restores_eligibility() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let evidence_scope = RepositoryScope::new(
            WorkspaceId::parse("ws-hold").unwrap(),
            ProjectId::parse("prj-hold").unwrap(),
            Some(ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (event, evidence) = support::valid_graph_publication(&evidence_scope, "key-hold");
        let evidence_id = evidence[0].reference().evidence_id().clone();
        database
            .repository
            .append_atomic(
                PreparedAppend::new(
                    evidence_scope.clone(),
                    OpaqueId::parse("stream-hold").unwrap(),
                    1,
                    vec![event],
                    evidence,
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let provider = Arc::new(RetentionKeyProvider::default());
        let store = Arc::new(
            PostgresEventStore::from_pool(database.runtime_pool.clone(), provider.clone())
                .await
                .unwrap(),
        );
        let now = PersistedTimestamp::from_datetime(Utc::now()).unwrap();
        let service = RetentionService::new(store.clone(), provider, Arc::new(Clock(now.clone())));
        let hold = legal_hold_change(
            evidence_scope.clone(),
            OpaqueId::parse("hold-one").unwrap(),
            evidence_id.clone(),
            OpaqueId::parse("authority-legal").unwrap(),
            SafeCode::parse("legal_request").unwrap(),
            true,
            now.clone(),
        );
        service.change_legal_hold(hold).await.unwrap();
        assert!(sqlx::query("UPDATE graphhelm_legal_holds SET reason_code='tampered' WHERE hold_id='hold-one'").execute(database.admin_pool()).await.is_err());
        let request = retention_request(
            &evidence_scope,
            evidence_id,
            "hold-operation",
            "hold-idempotency",
        );
        assert_eq!(
            service.execute(request.clone()).await.unwrap_err(),
            RetentionError::LegalHold
        );
        let released_at =
            PersistedTimestamp::from_datetime(*now.as_datetime() + chrono::TimeDelta::seconds(1))
                .unwrap();
        let release = legal_hold_change(
            evidence_scope,
            OpaqueId::parse("hold-one").unwrap(),
            request.targets()[0].evidence_id().clone(),
            OpaqueId::parse("authority-legal").unwrap(),
            SafeCode::parse("legal_request").unwrap(),
            false,
            released_at,
        );
        service.change_legal_hold(release).await.unwrap();
        service.execute(request).await.unwrap();
        let holds: i64 =
            sqlx::query_scalar("SELECT count(*) FROM graphhelm_legal_holds placed WHERE placed.placed AND NOT EXISTS (SELECT 1 FROM graphhelm_legal_holds released WHERE released.hold_id=placed.hold_id AND NOT released.placed)")
                .fetch_one(database.admin_pool())
                .await
                .unwrap();
        assert_eq!(holds, 0);
        let events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM graphhelm_events WHERE stream_id='retention-events'",
        )
        .fetch_one(database.admin_pool())
        .await
        .unwrap();
        assert_eq!(events, 4);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn erasure_is_pending_before_revoke_and_finalizes_once() {
    support::runtime().block_on(async {
        let database=support::TestDatabase::new().await;
        let evidence_scope=RepositoryScope::new(WorkspaceId::parse("ws-retention").unwrap(),ProjectId::parse("prj-retention").unwrap(),Some(ExecutionId::parse("execution-fixture").unwrap()));
        let (event,evidence)=support::valid_graph_publication(&evidence_scope,"key-retention");
        let evidence_id=evidence[0].reference().evidence_id().clone();
        database.repository.append_atomic(PreparedAppend::new(evidence_scope.clone(),OpaqueId::parse("stream-retention").unwrap(),1,vec![event],evidence,vec![]).unwrap()).await.unwrap();
        let provider=Arc::new(RetentionKeyProvider::default());
        let store=Arc::new(PostgresEventStore::from_pool(database.runtime_pool.clone(),provider.clone()).await.unwrap());
        let now=PersistedTimestamp::from_datetime(Utc::now()).unwrap();
        let policy=RetentionPolicy::new(OpaqueId::parse("policy-standard").unwrap(),SemanticVersion::parse("1.0.0").unwrap(),"standard",0,0).unwrap();
        let authority_id=OpaqueId::parse("authority-compliance").unwrap();
        let authority_tag=AuthenticationTag::new("retention-key","hmac-sha256",tag(&retention_authority_authentication_bytes(&authority_id,&evidence_scope,&policy))).unwrap();
        let request=RetentionRequest::new(evidence_scope.clone(),OpaqueId::parse("erase-operation").unwrap(),OpaqueId::parse("erase-idempotency").unwrap(),policy.clone(),RetentionAuthority::new(authority_id,evidence_scope.clone(),policy.id().clone(),policy.version().clone(),authority_tag),SafeCode::parse("scheduled_expiry").unwrap(),vec![RetentionTarget::new(evidence_id.clone()).unwrap()]).unwrap();
        let service=RetentionService::new(store.clone(),provider.clone(),Arc::new(Clock(now.clone())));
        let plan=service.dry_run(request.clone()).await.unwrap();
        assert!(plan.is_eligible());
        let first=match service.execute(request.clone()).await { Ok(value)=>value,Err(error)=>{let operations:i64=sqlx::query_scalar("SELECT count(*) FROM graphhelm_retention_operations").fetch_one(database.admin_pool()).await.unwrap();let events:i64=sqlx::query_scalar("SELECT count(*) FROM graphhelm_events WHERE stream_id='retention-events'").fetch_one(database.admin_pool()).await.unwrap();let pending=store.pending(evidence_scope.clone(),10).await;panic!("execute failed: {error:?}; operations={operations}; events={events}; epoch={}; pending={pending:?}",provider.epoch.load(Ordering::SeqCst));}};
        let retry_store=Arc::new(PostgresEventStore::from_pool(database.runtime_pool.clone(),provider.clone()).await.unwrap());
        let retry_service=RetentionService::new(retry_store,provider.clone(),Arc::new(Clock(now.clone())));
        let retry=retry_service.execute(request).await.unwrap();
        assert_eq!(first,retry);
        assert_eq!(provider.epoch.load(Ordering::SeqCst),1);
        assert!(sqlx::query("UPDATE graphhelm_retention_operations SET authority='tampered' WHERE operation_id='erase-operation'").execute(database.admin_pool()).await.is_err());
        assert!(sqlx::query("UPDATE graphhelm_retention_targets SET ciphertext_sha256=$1 WHERE operation_id='erase-operation'").bind("00".repeat(32)).execute(database.admin_pool()).await.is_err());
        assert!(sqlx::query("UPDATE graphhelm_retention_policies SET retention_class='ephemeral'").execute(database.admin_pool()).await.is_err());
        assert!(sqlx::query("UPDATE graphhelm_evidence SET record='{}'::jsonb WHERE evidence_id=$1").bind(evidence_id.as_str()).execute(database.admin_pool()).await.is_err());
        assert_eq!(store.get_sealed(evidence_scope,evidence_id.clone()).await.unwrap(),EvidenceRead::Unavailable(graphhelm_events::EvidenceUnavailableReason::Erased));
        let cleanup=CleanupRequest::new(
            first.prepared().request().scope().clone(),
            OpaqueId::parse("cleanup-operation").unwrap(),
            OpaqueId::parse("cleanup-idempotency").unwrap(),
            vec![first.prepared().plan().targets()[0].evidence_id().clone()],
            first.completed_at().clone(),
        ).unwrap();
        let cleanup_receipt=store.cleanup(cleanup.clone()).await.unwrap();
        assert_eq!(store.cleanup(cleanup.clone()).await.unwrap(),cleanup_receipt);
        assert!(sqlx::query("UPDATE graphhelm_cleanup_receipts SET ciphertext_sha256=$1").bind("00".repeat(32)).execute(database.admin_pool()).await.is_err());
        let divergent_cleanup=CleanupRequest::new(
            cleanup.scope().clone(),
            OpaqueId::parse("different-cleanup-operation").unwrap(),
            cleanup.idempotency_key().clone(),
            cleanup.evidence_ids().to_vec(),
            cleanup.requested_at().clone(),
        ).unwrap();
        assert_eq!(store.cleanup(divergent_cleanup).await.unwrap_err(),graphhelm_events::RetentionError::Conflict);
        sqlx::query("ALTER TABLE graphhelm_cleanup_receipts DISABLE TRIGGER graphhelm_cleanup_receipts_immutable").execute(database.admin_pool()).await.unwrap();
        sqlx::query("UPDATE graphhelm_cleanup_receipts SET deleted_at='2026-08-11T00:00:00Z'").execute(database.admin_pool()).await.unwrap();
        sqlx::query("ALTER TABLE graphhelm_cleanup_receipts ENABLE TRIGGER graphhelm_cleanup_receipts_immutable").execute(database.admin_pool()).await.unwrap();
        assert_eq!(store.cleanup(cleanup).await.unwrap_err(),RetentionError::Integrity);
        let record:serde_json::Value=sqlx::query_scalar("SELECT record FROM graphhelm_evidence WHERE evidence_id=$1").bind(evidence_id.as_str()).fetch_one(database.admin_pool()).await.unwrap();
        assert_eq!(record,serde_json::json!({}));
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM graphhelm_evidence_tombstones").fetch_one(database.admin_pool()).await.unwrap();
        assert_eq!(count,1);
        assert!(sqlx::query("UPDATE graphhelm_evidence_tombstones SET reason_code='tampered'").execute(database.admin_pool()).await.is_err());
        let tombstone:(String,String,String)=sqlx::query_as("SELECT classification,retention_class,prior_state FROM graphhelm_evidence_tombstones WHERE evidence_id=$1").bind(evidence_id.as_str()).fetch_one(database.admin_pool()).await.unwrap();
        assert_eq!(tombstone,("internal".to_owned(),"standard".to_owned(),"available".to_owned()));
        let persisted=sqlx::query_scalar::<_,String>("SELECT string_agg(envelope::text,'') FROM graphhelm_events WHERE stream_id='retention-events'").fetch_one(database.admin_pool()).await.unwrap();
        assert!(!persisted.contains("plaintext"));
        assert!(!persisted.contains("C:\\"));
        let mut wrong=database.runtime_pool.begin().await.unwrap();
        sqlx::query("SELECT set_config('graphhelm.workspace_id','ws-wrong',true),set_config('graphhelm.project_id','prj-wrong',true),set_config('graphhelm.execution_id','execution-fixture',true)").execute(&mut *wrong).await.unwrap();
        for table in ["graphhelm_retention_operations","graphhelm_retention_targets","graphhelm_evidence_tombstones","graphhelm_cleanup_receipts"] {
            let query=format!("SELECT count(*) FROM {table}");
            let hidden:i64=sqlx::query_scalar(sqlx::AssertSqlSafe(query)).fetch_one(&mut *wrong).await.unwrap();
            assert_eq!(hidden,0,"{table} leaked across RLS scope");
        }
        wrong.rollback().await.unwrap();
        let event_count:i64=sqlx::query_scalar("SELECT count(*) FROM graphhelm_events WHERE stream_id='retention-events'").fetch_one(database.admin_pool()).await.unwrap();
        assert_eq!(event_count,3);
        let event_scopes:Vec<String>=sqlx::query_scalar("SELECT envelope#>>'{kind,data,evidenceScope,executionId}' FROM graphhelm_events WHERE stream_id='retention-events' ORDER BY sequence").fetch_all(database.admin_pool()).await.unwrap();
        assert_eq!(event_scopes,vec!["execution-fixture".to_owned();3]);
        database.cleanup().await;
    });
}
