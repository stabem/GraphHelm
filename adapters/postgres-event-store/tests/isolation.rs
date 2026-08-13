mod support;

use graphhelm_events::{AsyncEventRepository, ReadStart, ReadStreamRequest};
use sqlx::Acquire;

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn cross_scope_identical_ids_do_not_collide_or_leak() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope_a = support::scope("scope-a");
        let scope_b = support::scope("scope-b");
        database
            .repository
            .append_atomic(support::prepared(
                scope_a.clone(),
                "same-stream",
                &["same-key"],
            ))
            .await
            .unwrap();
        database
            .repository
            .append_atomic(support::prepared(
                scope_b.clone(),
                "same-stream",
                &["same-key"],
            ))
            .await
            .unwrap();
        for scope in [scope_a, scope_b] {
            let page = database
                .repository
                .read_stream(
                    ReadStreamRequest::new(scope, "same-stream".into(), ReadStart::Beginning, 10)
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(page.events.len(), 1);
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM graphhelm_events")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(count, 2);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn direct_sql_without_or_with_wrong_scope_sees_no_rows() {
    support::runtime().block_on(async {
        let database=support::TestDatabase::new().await; let scope=support::scope("direct");
        database.repository.append_atomic(support::prepared(scope,"stream-direct", &["key-direct"])).await.unwrap();
        let absent:i64=sqlx::query_scalar("SELECT count(*) FROM graphhelm_events").fetch_one(&database.runtime_pool).await.unwrap(); assert_eq!(absent,0);
        let mut tx=database.runtime_pool.begin().await.unwrap(); sqlx::query("SELECT set_config('graphhelm.workspace_id','foreign',true),set_config('graphhelm.project_id','foreign',true),set_config('graphhelm.execution_id','',true)").execute(&mut *tx).await.unwrap();
        let wrong:i64=sqlx::query_scalar("SELECT count(*) FROM graphhelm_events").fetch_one(&mut *tx).await.unwrap(); assert_eq!(wrong,0); tx.rollback().await.unwrap();
        database.cleanup().await;
    });
}

/// A transaction-local GUC reverts to the empty string at transaction end rather than becoming
/// unset, so a scope predicate that compares directly against `current_setting` degrades from
/// matching nothing to matching every empty-scope row on a reused pooled connection. The policies
/// use `NULLIF` so an empty setting resolves to NULL, and the schema additionally forbids the rows
/// such a predicate could ever have exposed. This asserts the structural half from an identity that
/// bypasses RLS entirely, which is the only way an empty-scope row could otherwise be created.
#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn an_empty_scope_row_cannot_exist_even_for_an_rls_bypassing_identity() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let rejected = sqlx::query(
            "INSERT INTO graphhelm_evidence (workspace_id,project_id,execution_id,evidence_id,record) \
             VALUES ('','','','evidence-empty-scope','{}'::jsonb)",
        )
        .execute(database.admin_pool())
        .await;
        assert!(
            rejected.is_err(),
            "the schema must reject an empty workspace or project scope"
        );
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn pool_reuse_clears_commit_rollback_error_and_cancel_scope() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new().await;
        let scope = support::scope("leak");
        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-leak",
                &["key-leak"],
            ))
            .await
            .unwrap();
        let mut connection = database.runtime_pool.acquire().await.unwrap();
        for commit in [true, false] {
            let mut transaction = connection.begin().await.unwrap();
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
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM graphhelm_events")
                .fetch_one(&mut *transaction)
                .await
                .unwrap();
            assert_eq!(count, 1);
            if commit {
                transaction.commit().await.unwrap();
            } else {
                transaction.rollback().await.unwrap();
            }
        }

        let mut transaction = connection.begin().await.unwrap();
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
        assert!(
            sqlx::query("SELECT 1/0")
                .execute(&mut *transaction)
                .await
                .is_err()
        );
        transaction.rollback().await.unwrap();

        let mut transaction = connection.begin().await.unwrap();
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
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(20),
                sqlx::query("SELECT pg_sleep(0.2)").execute(&mut *transaction),
            )
            .await
            .is_err()
        );
        transaction.rollback().await.unwrap();

        let unscoped: i64 = sqlx::query_scalar("SELECT count(*) FROM graphhelm_events")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(unscoped, 0);
        drop(connection);
        database.cleanup().await;
    });
}
