mod support;

use graphhelm_events::{
    ArtifactCatalog, ArtifactRegistration, AsyncEventRepository, AuthenticatedCheckpoint,
    EvidenceRead, EvidenceRepository, KeyProvider, ReadStart, ReadStreamRequest,
    VerifyRangeRequest,
};
use graphhelm_postgres_event_store::PostgresFailpoint;
use graphhelm_protocols::{
    ArtifactId, ArtifactLocator, ArtifactReference, EventHash, EvidenceId, EvidenceReference,
    MediaType, OpaqueId, RawSha256, SemanticVersion,
};
use serde::Serialize;

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn append_is_atomic_across_event_evidence_and_refs() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("atomic");
        let evidence = support::sealed(scope.clone(), "evidence-atomic");
        let mut event = support::event("key-atomic");
        event.evidence_refs.push(evidence.reference().clone());
        let request = graphhelm_events::PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-atomic").unwrap(),
            1,
            vec![event],
            vec![evidence],
            vec![],
        )
        .unwrap();
        database
            .repository
            .set_failpoint_for_testing(PostgresFailpoint::AfterEvidenceBeforeEvent);
        assert_eq!(
            database
                .repository
                .append_atomic(request)
                .await
                .unwrap_err()
                .code(),
            "GHE008_STORAGE_FAILURE"
        );
        database
            .repository
            .set_failpoint_for_testing(PostgresFailpoint::None);
        for table in [
            "graphhelm_streams",
            "graphhelm_idempotency",
            "graphhelm_events",
            "graphhelm_evidence",
        ] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(database.admin_pool())
                    .await
                    .unwrap();
            assert_eq!(count, 0, "{table}");
        }
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn orphan_prepared_evidence_is_rejected_before_sql() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("orphan-evidence");
        let request = graphhelm_events::PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-orphan-evidence").unwrap(),
            1,
            vec![support::event("key-orphan-evidence")],
            vec![support::sealed(
                support::scope("orphan-evidence"),
                "evidence-orphan",
            )],
            vec![],
        )
        .unwrap();

        assert_eq!(
            database
                .repository
                .append_atomic(request)
                .await
                .unwrap_err()
                .code(),
            "GHE004_INVALID_EVENT"
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM graphhelm_streams")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn empty_stream_is_empty_and_tampered_head_is_rejected() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("head-integrity");
        let page = database
            .repository
            .read_stream(
                ReadStreamRequest::new(
                    scope.clone(),
                    "stream-head-integrity".into(),
                    ReadStart::Beginning,
                    10,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(page.events.is_empty());
        assert!(page.head.is_none());

        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-head-integrity",
                &["key-head-integrity"],
            ))
            .await
            .unwrap();
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE graphhelm_streams SET last_event_hash=$1")
            .bind(format!("sha256:{}", "ff".repeat(32)))
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .stream_head(scope, "stream-head-integrity".into())
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn exact_retry_precedes_sequence_and_divergent_retry_fails() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("retry");
        let request = support::prepared(scope.clone(), "stream-retry", &["key-retry"]);
        let first = database
            .repository
            .append_atomic(request.clone())
            .await
            .unwrap();
        let retry = database
            .repository
            .append_atomic(request.clone())
            .await
            .unwrap();
        assert_eq!(retry, first);
        let mut changed = support::event("key-retry");
        changed.sensitivity = graphhelm_protocols::Sensitivity::Restricted;
        let divergent = graphhelm_events::PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-retry").unwrap(),
            1,
            vec![changed],
            vec![],
            vec![],
        )
        .unwrap();
        assert_eq!(
            database
                .repository
                .append_atomic(divergent)
                .await
                .unwrap_err()
                .code(),
            "GHE003_IDEMPOTENCY_CONFLICT"
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM graphhelm_events")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(count, 1);
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE graphhelm_idempotency SET event_count=2")
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .append_atomic(request)
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn foreign_or_missing_evidence_and_artifact_refs_roll_back() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("missing");
        let mut event = support::event("key-missing");
        event.evidence_refs.push(EvidenceReference::new(
            EvidenceId::parse("missing-evidence").unwrap(),
            RawSha256::parse("11".repeat(32)).unwrap(),
            RawSha256::parse("22".repeat(32)).unwrap(),
        ));
        let request = graphhelm_events::PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-missing").unwrap(),
            1,
            vec![event],
            vec![],
            vec![],
        )
        .unwrap();
        assert_eq!(
            database
                .repository
                .append_atomic(request)
                .await
                .unwrap_err()
                .code(),
            "GHE004_INVALID_EVENT"
        );
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM graphhelm_streams")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn evidence_and_artifact_reads_revalidate_embedded_identity_and_digests() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("read-integrity");
        let evidence = support::sealed(scope.clone(), "evidence-read-integrity");
        let digest = "aa".repeat(32);
        let artifact = ArtifactReference::new(
            ArtifactId::parse("artifact-read-integrity").unwrap(),
            ArtifactLocator::parse(format!("artifact://sha256/{digest}")).unwrap(),
            RawSha256::parse(digest).unwrap(),
            MediaType::parse("application/json").unwrap(),
            7,
            graphhelm_protocols::Sensitivity::Internal,
            SemanticVersion::parse("1.0.0").unwrap(),
        )
        .unwrap();
        let mut event = support::event("producer-read-integrity");
        event.evidence_refs.push(evidence.reference().clone());
        event.artifact_refs.push(artifact.clone());
        let request = graphhelm_events::PreparedAppend::new(
            scope.clone(),
            OpaqueId::parse("stream-read-integrity").unwrap(),
            1,
            vec![event],
            vec![evidence.clone()],
            vec![ArtifactRegistration::new(artifact.clone(), "producer-read-integrity").unwrap()],
        )
        .unwrap();
        database.repository.append_atomic(request).await.unwrap();
        assert_eq!(
            database
                .repository
                .get_sealed(scope.clone(), evidence.reference().evidence_id().clone())
                .await
                .unwrap(),
            EvidenceRead::Available(evidence.clone())
        );
        assert_eq!(
            database
                .repository
                .resolve(scope.clone(), artifact.artifact_id().clone())
                .await
                .unwrap(),
            Some(artifact.clone())
        );

        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE graphhelm_evidence SET record=jsonb_set(record,'{ciphertext}', \
             '[1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1]'::jsonb)",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE graphhelm_artifacts SET reference=jsonb_set(reference,'{locator}', \
             to_jsonb($$artifact://sha256/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb$$::text))",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .get_sealed(scope.clone(), evidence.reference().evidence_id().clone())
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        assert_eq!(
            database
                .repository
                .resolve(scope.clone(), artifact.artifact_id().clone())
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        let mut reuse = support::event("reuse-tampered-evidence");
        reuse.evidence_refs.push(evidence.reference().clone());
        reuse.artifact_refs.push(artifact.clone());
        let request = graphhelm_events::PreparedAppend::new(
            scope,
            OpaqueId::parse("stream-read-integrity").unwrap(),
            2,
            vec![reuse],
            vec![],
            vec![],
        )
        .unwrap();
        assert_eq!(
            database
                .repository
                .append_atomic(request)
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn oversized_database_envelope_fails_closed() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("oversized-db-envelope");
        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-oversized-db-envelope",
                &["key-oversized-db-envelope"],
            ))
            .await
            .unwrap();
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE graphhelm_events SET envelope=jsonb_set( \
             envelope,'{kind,sourceSha256}',to_jsonb(repeat('a',4194305)))",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        scope,
                        "stream-oversized-db-envelope".into(),
                        ReadStart::Beginning,
                        10,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn read_preserves_the_full_page_across_internal_chunks() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("chunked-page");
        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-chunked-page",
                &[
                    "chunk-1", "chunk-2", "chunk-3", "chunk-4", "chunk-5", "chunk-6", "chunk-7",
                    "chunk-8", "chunk-9", "chunk-10",
                ],
            ))
            .await
            .unwrap();
        let page = database
            .repository
            .read_stream(
                ReadStreamRequest::new(
                    scope,
                    "stream-chunked-page".into(),
                    ReadStart::Beginning,
                    10,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(page.events.len(), 10);
        assert_eq!(page.events.first().unwrap().sequence, 1);
        assert_eq!(page.events.last().unwrap().sequence, 10);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn cursor_tamper_and_scope_or_stream_mismatch_fail_closed() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("cursor");
        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-cursor",
                &["key-cursor-a", "key-cursor-b"],
            ))
            .await
            .unwrap();
        let page = database
            .repository
            .read_stream(
                ReadStreamRequest::new(
                    scope.clone(),
                    "stream-cursor".into(),
                    ReadStart::Beginning,
                    1,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let cursor = page.next_cursor.unwrap();
        let mut tampered = cursor.into_bytes();
        let first = tampered[0];
        tampered[0] = if first == b'a' { b'b' } else { b'a' };
        let tampered = String::from_utf8(tampered).unwrap();
        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        scope.clone(),
                        "stream-cursor".into(),
                        ReadStart::Cursor(tampered),
                        1,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        let page = database
            .repository
            .read_stream(
                ReadStreamRequest::new(
                    scope.clone(),
                    "stream-cursor".into(),
                    ReadStart::Beginning,
                    1,
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        support::scope("foreign"),
                        "stream-cursor".into(),
                        ReadStart::Cursor(page.next_cursor.unwrap()),
                        1,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn chain_corruption_fails_before_read_return() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("corrupt");
        let request = support::prepared(scope.clone(), "stream-corrupt", &["key-corrupt"]);
        database
            .repository
            .append_atomic(request.clone())
            .await
            .unwrap();
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE graphhelm_events SET \
             envelope=jsonb_set(envelope,'{actor,id}',to_jsonb('system.corrupt'::text))",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        scope,
                        "stream-corrupt".into(),
                        ReadStart::Beginning,
                        10,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        assert_eq!(
            database
                .repository
                .append_atomic(request)
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn deleted_tail_is_rejected_against_the_authoritative_head() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("tail-anchor");
        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-tail-anchor",
                &["request-tail-a", "request-tail-b"],
            ))
            .await
            .unwrap();
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("DELETE FROM graphhelm_events WHERE sequence=2")
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();

        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        scope,
                        "stream-tail-anchor".into(),
                        ReadStart::Beginning,
                        10,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn verify_range_rejects_100001_before_sql() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let err =
            VerifyRangeRequest::new(support::scope("range"), "stream-range".into(), 1, 100_001)
                .unwrap_err();
        assert_eq!(err.code(), "GHE006_LIMIT_EXCEEDED");
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn checkpoints_are_authenticated_and_exact_scope() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("checkpoint");
        let events = database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-checkpoint",
                &["key-checkpoint"],
            ))
            .await
            .unwrap();
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Bytes<'a> {
            scope: &'a graphhelm_protocols::RepositoryScope,
            stream_id: &'a str,
            sequence: u64,
            event_hash: &'a str,
            repository_format_version: u32,
            created_at: chrono::DateTime<chrono::Utc>,
            key_version: &'a str,
            provider_epoch: u64,
            active_graph: Option<()>,
        }
        let bytes = support::canonical_bytes(&Bytes {
            scope: &scope,
            stream_id: "stream-checkpoint",
            sequence: 1,
            event_hash: events[0].event_hash.as_str(),
            repository_format_version: 1,
            created_at,
            key_version: "1",
            provider_epoch: 0,
            active_graph: None,
        });
        let tag = support::TestKeyProvider
            .authenticate(
                graphhelm_events::AuthenticateRequest::new("graphhelm-checkpoint-v1", bytes)
                    .unwrap(),
            )
            .await
            .unwrap();
        let checkpoint = AuthenticatedCheckpoint {
            scope: scope.clone(),
            stream_id: "stream-checkpoint".into(),
            sequence: 1,
            event_hash: EventHash::parse(events[0].event_hash.to_string()).unwrap(),
            repository_format_version: 1,
            created_at,
            key_version: "1".into(),
            provider_epoch: 0,
            active_graph: None,
            tag,
        };
        let mut invalid = checkpoint.clone();
        invalid.tag =
            graphhelm_events::AuthenticationTag::new("test-key", "hmac-sha256", vec![0_u8; 32])
                .unwrap();
        assert_eq!(
            database
                .repository
                .append_checkpoint(invalid)
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database
            .repository
            .append_checkpoint(checkpoint.clone())
            .await
            .unwrap();
        assert_eq!(
            database
                .repository
                .latest_checkpoint(scope, "stream-checkpoint".into())
                .await
                .unwrap(),
            Some(checkpoint)
        );
        let mut rewritten = events[0].clone();
        rewritten.actor = graphhelm_protocols::PersistedActor::new(
            graphhelm_protocols::PersistedActorType::System,
            graphhelm_protocols::ActorId::parse("system.rewritten").unwrap(),
        );
        rewritten.event_hash = EventHash::parse(
            graphhelm_events::compute_event_hash(&rewritten, rewritten.previous_hash.as_str())
                .unwrap(),
        )
        .unwrap();
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE graphhelm_events SET event_hash=$1,envelope=$2")
            .bind(rewritten.event_hash.as_str())
            .bind(serde_json::to_value(&rewritten).unwrap())
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE graphhelm_streams SET last_event_hash=$1")
            .bind(rewritten.event_hash.as_str())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        rewritten.scope.clone(),
                        "stream-checkpoint".into(),
                        ReadStart::Beginning,
                        10,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn checkpoint_anchor_rejects_later_prefix_corruption() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("checkpoint-prefix");
        let events = database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-checkpoint-prefix",
                &["key-checkpoint-prefix-a", "key-checkpoint-prefix-b"],
            ))
            .await
            .unwrap();
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Bytes<'a> {
            scope: &'a graphhelm_protocols::RepositoryScope,
            stream_id: &'a str,
            sequence: u64,
            event_hash: &'a str,
            repository_format_version: u32,
            created_at: chrono::DateTime<chrono::Utc>,
            key_version: &'a str,
            provider_epoch: u64,
            active_graph: Option<()>,
        }
        let bytes = support::canonical_bytes(&Bytes {
            scope: &scope,
            stream_id: "stream-checkpoint-prefix",
            sequence: 2,
            event_hash: events[1].event_hash.as_str(),
            repository_format_version: 1,
            created_at,
            key_version: "1",
            provider_epoch: 0,
            active_graph: None,
        });
        let tag = support::TestKeyProvider
            .authenticate(
                graphhelm_events::AuthenticateRequest::new("graphhelm-checkpoint-v1", bytes)
                    .unwrap(),
            )
            .await
            .unwrap();
        let checkpoint = AuthenticatedCheckpoint {
            scope: scope.clone(),
            stream_id: "stream-checkpoint-prefix".into(),
            sequence: 2,
            event_hash: events[1].event_hash.clone(),
            repository_format_version: 1,
            created_at,
            key_version: "1".into(),
            provider_epoch: 0,
            active_graph: None,
            tag,
        };
        database
            .repository
            .append_checkpoint(checkpoint)
            .await
            .unwrap();
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query(
            "UPDATE graphhelm_events SET envelope=jsonb_set(envelope,'{actor,id}',to_jsonb('system.corrupt'::text)) WHERE sequence=1",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        scope,
                        "stream-checkpoint-prefix".into(),
                        ReadStart::Beginning,
                        1,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn authenticated_head_rejects_rehashed_checkpoint_suffix() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("checkpoint-suffix-head");
        let events = database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-checkpoint-suffix-head",
                &["key-suffix-a", "key-suffix-b"],
            ))
            .await
            .unwrap();
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Bytes<'a> {
            scope: &'a graphhelm_protocols::RepositoryScope,
            stream_id: &'a str,
            sequence: u64,
            event_hash: &'a str,
            repository_format_version: u32,
            created_at: chrono::DateTime<chrono::Utc>,
            key_version: &'a str,
            provider_epoch: u64,
            active_graph: Option<()>,
        }
        let bytes = support::canonical_bytes(&Bytes {
            scope: &scope,
            stream_id: "stream-checkpoint-suffix-head",
            sequence: 1,
            event_hash: events[0].event_hash.as_str(),
            repository_format_version: 1,
            created_at,
            key_version: "1",
            provider_epoch: 0,
            active_graph: None,
        });
        let tag = support::TestKeyProvider
            .authenticate(
                graphhelm_events::AuthenticateRequest::new("graphhelm-checkpoint-v1", bytes)
                    .unwrap(),
            )
            .await
            .unwrap();
        database
            .repository
            .append_checkpoint(AuthenticatedCheckpoint {
                scope: scope.clone(),
                stream_id: "stream-checkpoint-suffix-head".into(),
                sequence: 1,
                event_hash: events[0].event_hash.clone(),
                repository_format_version: 1,
                created_at,
                key_version: "1".into(),
                provider_epoch: 0,
                active_graph: None,
                tag,
            })
            .await
            .unwrap();

        let mut rewritten = events[1].clone();
        rewritten.actor = graphhelm_protocols::PersistedActor::new(
            graphhelm_protocols::PersistedActorType::System,
            graphhelm_protocols::ActorId::parse("system.rehashed-suffix").unwrap(),
        );
        rewritten.event_hash = EventHash::parse(
            graphhelm_events::compute_event_hash(&rewritten, events[0].event_hash.as_str())
                .unwrap(),
        )
        .unwrap();
        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE graphhelm_events SET event_hash=$1,envelope=$2 WHERE sequence=2")
            .bind(rewritten.event_hash.as_str())
            .bind(serde_json::to_value(&rewritten).unwrap())
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE graphhelm_streams SET last_event_hash=$1")
            .bind(rewritten.event_hash.as_str())
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .read_stream(
                    ReadStreamRequest::new(
                        scope,
                        "stream-checkpoint-suffix-head".into(),
                        ReadStart::Beginning,
                        10,
                    )
                    .unwrap(),
                )
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn checkpoint_round_trips_and_authenticates_active_graph() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = graphhelm_protocols::RepositoryScope::new(
            graphhelm_protocols::WorkspaceId::parse("ws-active-checkpoint").unwrap(),
            graphhelm_protocols::ProjectId::parse("prj-active-checkpoint").unwrap(),
            Some(graphhelm_protocols::ExecutionId::parse("execution-fixture").unwrap()),
        );
        let (publication, evidence) =
            support::valid_graph_publication(&scope, "key-active-checkpoint");
        let request = graphhelm_events::PreparedAppend::new(
            scope.clone(),
            OpaqueId::parse("stream-active-checkpoint").unwrap(),
            1,
            vec![publication],
            evidence,
            vec![],
        )
        .unwrap();
        let graphhelm_protocols::EventKind::GraphVersionPublished(payload) =
            &request.events()[0].kind
        else {
            unreachable!();
        };
        assert!(graphhelm_graph::validate_persisted_projection(&payload.version).is_ok());
        assert!(
            graphhelm_graph::validate_publication_evidence_ids(&scope, &payload.version).is_ok()
        );
        assert!(
            graphhelm_graph::validate_evidence_bijection(
                payload.version.content_slots(),
                &request.events()[0].evidence_refs,
            )
            .is_ok()
        );
        for item in request.evidence() {
            graphhelm_events::validate_sealed_evidence(item).unwrap();
        }
        graphhelm_events::validate_prepared_append(&request).unwrap();
        let semantic_hash = payload.version.semantic_hash().to_string();
        let events = database.repository.append_atomic(request).await.unwrap();
        let active = graphhelm_events::ActiveGraphIdentity::new(1, semantic_hash).unwrap();
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-08-11T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Bytes<'a> {
            scope: &'a graphhelm_protocols::RepositoryScope,
            stream_id: &'a str,
            sequence: u64,
            event_hash: &'a str,
            repository_format_version: u32,
            created_at: chrono::DateTime<chrono::Utc>,
            key_version: &'a str,
            provider_epoch: u64,
            active_graph: &'a Option<graphhelm_events::ActiveGraphIdentity>,
        }
        let active_graph = Some(active.clone());
        let bytes = support::canonical_bytes(&Bytes {
            scope: &scope,
            stream_id: "stream-active-checkpoint",
            sequence: 1,
            event_hash: events[0].event_hash.as_str(),
            repository_format_version: 1,
            created_at,
            key_version: "1",
            provider_epoch: 0,
            active_graph: &active_graph,
        });
        let tag = support::TestKeyProvider
            .authenticate(
                graphhelm_events::AuthenticateRequest::new("graphhelm-checkpoint-v1", bytes)
                    .unwrap(),
            )
            .await
            .unwrap();
        let checkpoint = AuthenticatedCheckpoint {
            scope: scope.clone(),
            stream_id: "stream-active-checkpoint".into(),
            sequence: 1,
            event_hash: events[0].event_hash.clone(),
            repository_format_version: 1,
            created_at,
            key_version: "1".into(),
            provider_epoch: 0,
            active_graph,
            tag,
        };
        database
            .repository
            .append_checkpoint(checkpoint.clone())
            .await
            .unwrap();
        assert_eq!(
            database
                .repository
                .latest_checkpoint(scope.clone(), "stream-active-checkpoint".into())
                .await
                .unwrap(),
            Some(checkpoint)
        );
        let predecessor = graphhelm_protocols::PersistedGraphVersionRef::new(
            1,
            graphhelm_protocols::WireHash::parse(active.semantic_hash().to_owned()).unwrap(),
        )
        .unwrap();
        let (successor, successor_evidence) = support::valid_graph_publication_for(
            &scope,
            "key-active-successor",
            2,
            Some(predecessor),
        );
        database
            .repository
            .append_atomic(
                graphhelm_events::PreparedAppend::new(
                    scope.clone(),
                    OpaqueId::parse("stream-active-checkpoint").unwrap(),
                    2,
                    vec![successor],
                    successor_evidence,
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();

        let mut half_null = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *half_null)
            .await
            .unwrap();
        assert!(
            sqlx::query("UPDATE graphhelm_checkpoints SET active_graph_number=NULL")
                .execute(&mut *half_null)
                .await
                .is_err()
        );
        half_null.rollback().await.unwrap();

        let mut transaction = database.admin_pool().begin().await.unwrap();
        sqlx::query("SET LOCAL session_replication_role = replica")
            .execute(&mut *transaction)
            .await
            .unwrap();
        sqlx::query("UPDATE graphhelm_checkpoints SET active_graph_semantic_hash=$1")
            .bind(format!("sha256:{}", "c".repeat(64)))
            .execute(&mut *transaction)
            .await
            .unwrap();
        transaction.commit().await.unwrap();
        assert_eq!(
            database
                .repository
                .latest_checkpoint(scope, "stream-active-checkpoint".into())
                .await
                .unwrap_err()
                .code(),
            "GHE005_INTEGRITY_FAILURE"
        );
        database.cleanup().await;
    });
}
