mod support;

use std::sync::Arc;

use graphhelm_events::{
    AsyncEventRepository, EventRepositoryError, PreparedAppend, ProjectionGeneration,
    ProjectionRebuildRequest, ProjectionRebuilder, ProjectionRepository, RepositoryFuture,
    StreamHead,
};
use graphhelm_postgres_event_store::PostgresEventStore;
use graphhelm_protocols::{
    ActorId, EventKind, ExecutionMode, ExecutionStarted, NewEvent, NodeOutcome,
    NodeOutcomeRecorded, NodeState, OpaqueId, PersistedActor, PersistedActorType, RepositoryScope,
    Sensitivity, WireHash,
};
use sqlx::Row;

fn request(
    scope: graphhelm_protocols::RepositoryScope,
    stream: &str,
    generation: u64,
) -> ProjectionRebuildRequest {
    ProjectionRebuildRequest::new(
        scope,
        stream.to_owned(),
        "execution-projection".to_owned(),
        1,
        generation,
        10,
    )
    .unwrap()
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn generation_checkpoints_resume_and_activate_exactly() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("projection-roundtrip");
        let stream = "stream-projection-roundtrip";
        let events = database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                stream,
                &["key-projection-one"],
            ))
            .await
            .unwrap();
        let head = database
            .repository
            .stream_head(scope.clone(), stream.to_owned())
            .await
            .unwrap();
        let rebuild = request(scope.clone(), stream, 1);
        let mut generation = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            1,
        )
        .unwrap();
        generation.apply_page(&events).unwrap();

        database
            .repository
            .save_generation(generation.clone())
            .await
            .unwrap();
        database
            .repository
            .save_generation(generation.clone())
            .await
            .unwrap();
        let mut divergent_json = serde_json::to_value(&generation).unwrap();
        divergent_json["projection"]["proposedDrafts"] = serde_json::json!(["divergent-state"]);
        let divergent: ProjectionGeneration = serde_json::from_value(divergent_json).unwrap();
        assert!(matches!(
            database.repository.save_generation(divergent).await,
            Err(EventRepositoryError::Integrity)
        ));
        assert_eq!(
            database.repository.load_generation(&rebuild).await.unwrap(),
            Some(generation.clone())
        );
        database
            .repository
            .swap_active(generation.clone(), head)
            .await
            .unwrap();
        assert_eq!(
            database
                .repository
                .load_active(
                    scope,
                    stream.to_owned(),
                    "execution-projection".to_owned(),
                    1,
                )
                .await
                .unwrap(),
            Some(generation)
        );

        let checkpoint_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM graphhelm_projection_checkpoints")
                .fetch_one(database.admin_pool())
                .await
                .unwrap();
        assert_eq!(checkpoint_count, 1);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn unsupported_projection_format_is_never_interpreted() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("projection-old-format");
        let stream = "stream-projection-old-format";
        let generation = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            1,
        )
        .unwrap();
        database
            .repository
            .save_generation(generation)
            .await
            .unwrap();
        sqlx::query(
            "ALTER TABLE graphhelm_projection_checkpoints DISABLE TRIGGER \
             graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE graphhelm_projection_checkpoints DROP CONSTRAINT \
             graphhelm_projection_checkpoints_format_version_check",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query("UPDATE graphhelm_projection_checkpoints SET format_version=0")
            .execute(database.admin_pool())
            .await
            .unwrap();
        assert!(matches!(
            database
                .repository
                .load_generation(&request(scope, stream, 1))
                .await,
            Err(EventRepositoryError::UnsupportedFormat)
        ));
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn stale_source_head_never_replaces_the_old_active_generation() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("projection-stale");
        let stream = "stream-projection-stale";
        let first = database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                stream,
                &["key-projection-first"],
            ))
            .await
            .unwrap();
        let first_head = database
            .repository
            .stream_head(scope.clone(), stream.to_owned())
            .await
            .unwrap();
        let mut old = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            1,
        )
        .unwrap();
        old.apply_page(&first).unwrap();
        database
            .repository
            .save_generation(old.clone())
            .await
            .unwrap();
        database
            .repository
            .swap_active(old.clone(), first_head.clone())
            .await
            .unwrap();

        let second = database
            .repository
            .append_atomic(
                graphhelm_events::PreparedAppend::new(
                    scope.clone(),
                    graphhelm_protocols::OpaqueId::parse(stream).unwrap(),
                    2,
                    vec![support::event("key-projection-second")],
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let current_head = database
            .repository
            .stream_head(scope.clone(), stream.to_owned())
            .await
            .unwrap();
        let mut replacement = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            2,
        )
        .unwrap();
        replacement.apply_page(&first).unwrap();
        replacement.apply_page(&second).unwrap();
        database
            .repository
            .save_generation(replacement.clone())
            .await
            .unwrap();

        assert!(matches!(
            database
                .repository
                .swap_active(replacement.clone(), first_head)
                .await,
            Err(EventRepositoryError::SequenceConflict)
        ));
        assert_eq!(
            database
                .repository
                .load_active(
                    scope.clone(),
                    stream.to_owned(),
                    "execution-projection".to_owned(),
                    1,
                )
                .await
                .unwrap(),
            Some(old)
        );
        database
            .repository
            .swap_active(replacement.clone(), current_head)
            .await
            .unwrap();
        assert_eq!(
            database
                .repository
                .load_active(
                    scope,
                    stream.to_owned(),
                    "execution-projection".to_owned(),
                    1
                )
                .await
                .unwrap(),
            Some(replacement)
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn projection_rows_are_scoped_immutable_and_corruption_fails_closed() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("projection-guard");
        let stream = "stream-projection-guard";
        let generation = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            1,
        )
        .unwrap();
        database
            .repository
            .save_generation(generation.clone())
            .await
            .unwrap();
        database
            .repository
            .swap_active(generation.clone(), None)
            .await
            .unwrap();

        let stale = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            2,
        )
        .unwrap();
        database.repository.save_generation(stale).await.unwrap();
        let committed = database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                stream,
                &["key-projection-guard"],
            ))
            .await
            .unwrap();
        let mut scoped = database.runtime_pool.begin().await.unwrap();
        sqlx::query(
            "SELECT set_config('graphhelm.workspace_id',$1,true), \
             set_config('graphhelm.project_id',$2,true), \
             set_config('graphhelm.execution_id',$3,true)",
        )
        .bind(scope.workspace_id().as_str())
        .bind(scope.project_id().as_str())
        .bind(scope.execution_id().unwrap().as_str())
        .execute(&mut *scoped)
        .await
        .unwrap();
        assert!(
            sqlx::query(
                "UPDATE graphhelm_projection_active SET generation=2,last_sequence=0 \
                 WHERE stream_id=$1 AND projection_name='execution-projection'",
            )
            .bind(stream)
            .execute(&mut *scoped)
            .await
            .is_err()
        );
        scoped.rollback().await.unwrap();

        let mut authentic = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            3,
        )
        .unwrap();
        authentic.apply_page(&committed).unwrap();
        let mut forged_json = serde_json::to_value(&authentic).unwrap();
        forged_json["projection"]["proposedDrafts"] = serde_json::json!(["forged-success"]);
        let forged: ProjectionGeneration = serde_json::from_value(forged_json).unwrap();
        assert!(matches!(
            database.repository.save_generation(forged.clone()).await,
            Err(EventRepositoryError::Integrity)
        ));
        database
            .repository
            .save_generation(authentic)
            .await
            .unwrap();
        sqlx::query(
            "ALTER TABLE graphhelm_projection_checkpoints DISABLE TRIGGER \
             graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE graphhelm_projection_checkpoints SET state=$1 \
             WHERE stream_id=$2 AND projection_name='execution-projection' AND generation=3",
        )
        .bind(serde_json::to_value(&forged).unwrap())
        .bind(stream)
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "ALTER TABLE graphhelm_projection_checkpoints ENABLE TRIGGER \
             graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        let current_head = database
            .repository
            .stream_head(scope.clone(), stream.to_owned())
            .await
            .unwrap();
        assert!(matches!(
            database.repository.swap_active(forged, current_head).await,
            Err(EventRepositoryError::Integrity)
        ));
        assert!(matches!(
            database
                .repository
                .load_generation(&request(scope.clone(), stream, 3))
                .await,
            Err(EventRepositoryError::Integrity)
        ));
        assert_eq!(
            database
                .repository
                .load_active(
                    scope.clone(),
                    stream.to_owned(),
                    "execution-projection".to_owned(),
                    1,
                )
                .await
                .unwrap(),
            Some(generation)
        );
        let mut forged_active = database.runtime_pool.begin().await.unwrap();
        sqlx::query(
            "SELECT set_config('graphhelm.workspace_id',$1,true), \
             set_config('graphhelm.project_id',$2,true), \
             set_config('graphhelm.execution_id',$3,true)",
        )
        .bind(scope.workspace_id().as_str())
        .bind(scope.project_id().as_str())
        .bind(scope.execution_id().unwrap().as_str())
        .execute(&mut *forged_active)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE graphhelm_projection_active SET generation=3,last_sequence=1 \
             WHERE stream_id=$1 AND projection_name='execution-projection'",
        )
        .bind(stream)
        .execute(&mut *forged_active)
        .await
        .unwrap();
        forged_active.commit().await.unwrap();
        assert!(matches!(
            database
                .repository
                .load_active(
                    scope.clone(),
                    stream.to_owned(),
                    "execution-projection".to_owned(),
                    1,
                )
                .await,
            Err(EventRepositoryError::Integrity)
        ));

        let unscoped: i64 =
            sqlx::query_scalar("SELECT count(*) FROM graphhelm_projection_checkpoints")
                .fetch_one(&database.runtime_pool)
                .await
                .unwrap();
        assert_eq!(unscoped, 0);
        let rows = sqlx::query(
            "SELECT relname,relrowsecurity,relforcerowsecurity FROM pg_class \
             WHERE relname IN ('graphhelm_projection_checkpoints','graphhelm_projection_active') \
             ORDER BY relname",
        )
        .fetch_all(database.admin_pool())
        .await
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|row| {
            row.get::<bool, _>("relrowsecurity") && row.get::<bool, _>("relforcerowsecurity")
        }));
        assert!(
            sqlx::query("UPDATE graphhelm_projection_checkpoints SET state='{}'::jsonb")
                .execute(database.admin_pool())
                .await
                .is_err()
        );
        assert!(
            sqlx::query("DELETE FROM graphhelm_projection_active")
                .execute(database.admin_pool())
                .await
                .is_err()
        );

        sqlx::query(
            "ALTER TABLE graphhelm_projection_checkpoints DISABLE TRIGGER \
             graphhelm_projection_checkpoints_immutable",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        sqlx::query(
            "UPDATE graphhelm_projection_checkpoints \
             SET state=jsonb_build_object('watermark','old-format')",
        )
        .execute(database.admin_pool())
        .await
        .unwrap();
        assert!(matches!(
            database
                .repository
                .load_generation(&request(scope, stream, 1))
                .await,
            Err(EventRepositoryError::Integrity | EventRepositoryError::UnsupportedFormat)
        ));
        database.cleanup().await;
    });
}

/// One `ExecutionStarted` and the two `NodeOutcomeRecorded` hops of one dispatch, so the generation
/// resumed below carries
/// real execution state (`executionId`, `mode`, `nodeAttempts`, `lastOutcome`,
/// `identicalOutcomes`) rather than only the pre-04b fields.
fn execution_events(execution: &str) -> Vec<NewEvent> {
    let execution_id = OpaqueId::parse(execution).unwrap();
    let actor_running = PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system.test").unwrap(),
    );
    let actor = PersistedActor::new(
        PersistedActorType::System,
        ActorId::parse("system.test").unwrap(),
    );
    vec![
        NewEvent::new(
            OpaqueId::parse("key-watermark-started").unwrap(),
            actor.clone(),
            Sensitivity::Internal,
            EventKind::ExecutionStarted(ExecutionStarted {
                execution_id: execution_id.clone(),
                graph_version: 1,
                graph_hash: WireHash::parse(format!("sha256:{}", "a".repeat(64))).unwrap(),
                mode: ExecutionMode::Supervised,
            }),
            Vec::new(),
            Vec::new(),
        ),
        NewEvent::new(
            OpaqueId::parse("key-watermark-outcome").unwrap(),
            actor.clone(),
            Sensitivity::Internal,
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                executor: None,
                execution_id: execution_id.clone(),
                node_id: OpaqueId::parse("start").unwrap(),
                outcome: NodeOutcome::Started,
                next_state: NodeState::Queued,
                reason: None,
            }),
            Vec::new(),
            Vec::new(),
        ),
        // Dispatch is two hops: Ready -> Queued, then Queued -> Running. Only the second is an
        // attempt, so emitting just the first would leave nodeAttempts at zero.
        NewEvent::new(
            OpaqueId::parse("key-watermark-running").unwrap(),
            actor_running,
            Sensitivity::Internal,
            EventKind::NodeOutcomeRecorded(NodeOutcomeRecorded {
                executor: None,
                execution_id,
                node_id: OpaqueId::parse("start").unwrap(),
                outcome: NodeOutcome::Started,
                next_state: NodeState::Running,
                reason: None,
            }),
            Vec::new(),
            Vec::new(),
        ),
    ]
}

/// A `ProjectionRepository` that always answers `load_generation` with a fixed, pre-built
/// generation, regardless of the identity the caller actually requested. It stands in for a
/// hypothetically buggy or compromised storage layer, which is the only way to exercise the
/// domain-level guard in `ProjectionRebuilder::rebuild` in isolation: the real Postgres adapter's
/// own `load_generation` cannot return a mismatched generation (its query filters by the exact
/// requested identity, and `decode_generation` additionally rejects a stored row whose embedded
/// watermark disagrees with its own columns), so that guard is otherwise unreachable through the
/// adapter alone. Everything else delegates to the real adapter so `save_generation`/`load_active`/
/// `swap_active` still run the genuine authenticated code path.
struct MismatchedProjectionRepository {
    inner: Arc<PostgresEventStore>,
    stored: ProjectionGeneration,
}

impl ProjectionRepository for MismatchedProjectionRepository {
    fn load_generation<'a>(
        &'a self,
        _request: &'a ProjectionRebuildRequest,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>> {
        Box::pin(async move { Ok(Some(self.stored.clone())) })
    }

    fn save_generation<'a>(
        &'a self,
        generation: ProjectionGeneration,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>> {
        self.inner.save_generation(generation)
    }

    fn load_active<'a>(
        &'a self,
        scope: RepositoryScope,
        stream_id: String,
        projection_name: String,
        projection_version: u32,
    ) -> RepositoryFuture<'a, Result<Option<ProjectionGeneration>, EventRepositoryError>> {
        self.inner
            .load_active(scope, stream_id, projection_name, projection_version)
    }

    fn swap_active<'a>(
        &'a self,
        generation: ProjectionGeneration,
        expected_source_head: Option<StreamHead>,
    ) -> RepositoryFuture<'a, Result<(), EventRepositoryError>> {
        self.inner.swap_active(generation, expected_source_head)
    }
}

/// `GHPROJ001_WATERMARK_MISMATCH` was shipped in milestone 03 with no test ever observed to
/// produce it (see the event-evidence-store SDD record, `final-review-findings.md`).
/// It is raised in exactly one place,
/// `ProjectionRebuilder::rebuild` (`core/events/src/projection.rs`), when a stored generation's
/// identity does not match the one requested to resume — a defense-in-depth guard against a
/// storage layer that hands back the wrong generation. This test drives that guard directly with
/// `MismatchedProjectionRepository`, and proves that a generation carrying real execution state
/// (D-022 mode, attempt counts, outcome runs) is rejected exactly the same as an old-format one:
/// adding fields to `ExecutionProjection` did not loosen the identity check that guards the swap.
#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn a_generation_with_mismatched_identity_is_rejected_as_watermark_mismatch_even_with_execution_state()
 {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("projection-watermark-mismatch");
        let stream = "stream-projection-watermark-mismatch";

        let events = database
            .repository
            .append_atomic(
                PreparedAppend::new(
                    scope.clone(),
                    OpaqueId::parse(stream).unwrap(),
                    1,
                    execution_events(scope.execution_id().unwrap().as_str()),
                    Vec::new(),
                    Vec::new(),
                )
                .unwrap(),
            )
            .await
            .unwrap();

        // The generation storage actually hands back describes generation 2, carrying execution
        // state folded from the real history above, while the request below asks to resume
        // generation 1. That is precisely a "cursor/hash/version cannot safely resume" case.
        let mut stored = ProjectionGeneration::new(
            scope.clone(),
            stream.to_owned(),
            "execution-projection".to_owned(),
            1,
            2,
        )
        .unwrap();
        stored.apply_page(&events).unwrap();
        assert!(stored.projection().execution_id.is_some());
        assert_eq!(stored.projection().mode, Some(ExecutionMode::Supervised));
        assert_eq!(stored.projection().node_attempts.get("start"), Some(&1));

        let projections = Arc::new(MismatchedProjectionRepository {
            inner: database.repository.clone(),
            stored,
        });
        let rebuilder = ProjectionRebuilder::new(database.repository.clone(), projections);
        let mismatched_request = request(scope, stream, 1);

        let error = rebuilder.rebuild(mismatched_request).await.unwrap_err();
        assert!(matches!(error, EventRepositoryError::WatermarkMismatch));
        assert_eq!(error.code(), "GHPROJ001_WATERMARK_MISMATCH");
        database.cleanup().await;
    });
}
