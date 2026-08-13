mod support;

use std::sync::Arc;

use graphhelm_events::AsyncEventRepository;
use graphhelm_postgres_event_store::PostgresEventStore;
use sqlx::Row;

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn migration_enforces_rls_and_runtime_role_separation() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let rows = sqlx::query("SELECT relrowsecurity, relforcerowsecurity FROM pg_class WHERE relname LIKE 'graphhelm_%' AND relkind='r'")
            .fetch_all(database.admin_pool()).await.unwrap();
        assert_eq!(rows.len(), 16);
        assert!(rows.iter().all(|row| row.get::<bool,_>("relrowsecurity") && row.get::<bool,_>("relforcerowsecurity")));
        let bypass: bool = sqlx::query_scalar("SELECT rolbypassrls FROM pg_roles WHERE rolname=current_user").fetch_one(&database.runtime_pool).await.unwrap();
        assert!(!bypass);
        let owns: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_class WHERE relname LIKE 'graphhelm_%' AND pg_get_userbyid(relowner)=current_user)").fetch_one(&database.runtime_pool).await.unwrap();
        assert!(!owns);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn migration_is_ledgered_and_runtime_history_is_immutable() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        graphhelm_postgres_event_store::migrate(database.admin_pool())
            .await
            .unwrap();
        let migration_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE version=1")
                .fetch_one(database.admin_pool())
                .await
                .unwrap();
        assert_eq!(migration_count, 1);

        let dangerous: bool = sqlx::query_scalar(
            "SELECT has_schema_privilege(current_user,'public','CREATE') OR \
             has_database_privilege(current_user,current_database(),'CREATE') OR \
             has_database_privilege(current_user,current_database(),'TEMP') OR \
             has_table_privilege(current_user,'graphhelm_events','UPDATE, DELETE, TRUNCATE')",
        )
        .fetch_one(&database.runtime_pool)
        .await
        .unwrap();
        assert!(!dangerous);

        let database_name: String = sqlx::query_scalar("SELECT current_database()")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "GRANT CREATE ON DATABASE {database_name} TO {}",
            database.role_name()
        )))
        .execute(database.admin_pool())
        .await
        .unwrap();
        assert_eq!(
            graphhelm_postgres_event_store::verify_runtime_role(&database.runtime_pool)
                .await
                .unwrap_err()
                .code(),
            "GHE004_INVALID_EVENT"
        );
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "REVOKE CREATE ON DATABASE {database_name} FROM {}",
            database.role_name()
        )))
        .execute(database.admin_pool())
        .await
        .unwrap();

        let scope = support::scope("locked-row");
        let request = support::prepared(scope.clone(), "stream-locked-row", &["key-locked-row"]);
        assert!(graphhelm_events::validate_prepared_append(&request).is_ok());
        database.repository.append_atomic(request).await.unwrap();
        let mut transaction = database.runtime_pool.begin().await.unwrap();
        sqlx::query(
            "SELECT set_config('graphhelm.workspace_id',$1,true), \
             set_config('graphhelm.project_id',$2,true), \
             set_config('graphhelm.execution_id',$3,true)",
        )
        .bind(scope.workspace_id().as_str())
        .bind(scope.project_id().as_str())
        .bind(scope.execution_id().unwrap().as_str())
        .execute(&mut *transaction)
        .await
        .unwrap();
        let forged_insert = sqlx::query(
            "INSERT INTO graphhelm_streams \
             (workspace_id,project_id,execution_id,stream_id,next_sequence,last_event_hash) \
             VALUES ($1,$2,$3,'stream-forged',9, \
             'sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff')",
        )
        .bind(scope.workspace_id().as_str())
        .bind(scope.project_id().as_str())
        .bind(scope.execution_id().unwrap().as_str())
        .execute(&mut *transaction)
        .await
        .unwrap_err();
        assert_eq!(
            forged_insert
                .as_database_error()
                .and_then(|value| value.code())
                .as_deref(),
            Some("55000")
        );
        transaction.rollback().await.unwrap();

        let mut transaction = database.runtime_pool.begin().await.unwrap();
        sqlx::query(
            "SELECT set_config('graphhelm.workspace_id',$1,true), \
             set_config('graphhelm.project_id',$2,true), \
             set_config('graphhelm.execution_id',$3,true)",
        )
        .bind(scope.workspace_id().as_str())
        .bind(scope.project_id().as_str())
        .bind(scope.execution_id().unwrap().as_str())
        .execute(&mut *transaction)
        .await
        .unwrap();
        let forged = sqlx::query(
            "UPDATE graphhelm_streams SET next_sequence=next_sequence+10, \
             last_event_hash='sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff'",
        )
        .execute(&mut *transaction)
        .await
        .unwrap_err();
        assert_eq!(
            forged
                .as_database_error()
                .and_then(|value| value.code())
                .as_deref(),
            Some("55000")
        );
        transaction.rollback().await.unwrap();
        let error = sqlx::query("DELETE FROM graphhelm_events")
            .execute(database.admin_pool())
            .await
            .unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|value| value.code())
                .as_deref(),
            Some("55000")
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn startup_rejects_unknown_or_failed_migration_history() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        sqlx::query(
            "INSERT INTO _sqlx_migrations \
             (version,description,installed_on,success,checksum,execution_time) \
             VALUES (5,'unknown',now(),false,$1,0)",
        )
        .bind(vec![0_u8; 32])
        .execute(database.admin_pool())
        .await
        .unwrap();
        let result = PostgresEventStore::from_pool(
            database.runtime_pool.clone(),
            Arc::new(support::TestKeyProvider),
        )
        .await;
        assert_eq!(result.err().unwrap().code(), "GHE005_INTEGRITY_FAILURE");
        database.cleanup().await;
    });
}
