mod support;

use graphhelm_events::{
    AsyncEventRepository, EventRepositoryError, ProjectionGeneration, ProjectionRebuildRequest,
    ProjectionRepository,
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
