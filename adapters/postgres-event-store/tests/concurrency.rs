mod support;

use graphhelm_events::{AsyncEventRepository, ReadStart, ReadStreamRequest};

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn same_stream_concurrent_append_has_exactly_one_winner() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new_with_max_connections(2).await;
        let scope = support::scope("race");
        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-race",
                &["key-race-seed"],
            ))
            .await
            .unwrap();
        let repository_a = database.repository.clone();
        let repository_b = database.repository.clone();
        database
            .repository
            .pause_first_append_after_head_read_for_testing();
        let request_a = graphhelm_events::PreparedAppend::new(
            scope.clone(),
            graphhelm_protocols::OpaqueId::parse("stream-race").unwrap(),
            2,
            vec![support::event("key-race-a")],
            vec![],
            vec![],
        )
        .unwrap();
        let request_b = graphhelm_events::PreparedAppend::new(
            scope,
            graphhelm_protocols::OpaqueId::parse("stream-race").unwrap(),
            2,
            vec![support::event("key-race-b")],
            vec![],
            vec![],
        )
        .unwrap();
        let left = tokio::spawn(async move { repository_a.append_atomic(request_a).await });
        database
            .repository
            .wait_for_paused_head_read_for_testing()
            .await;
        let right = tokio::spawn(async move { repository_b.append_atomic(request_b).await });
        let contender_read = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            database.repository.wait_for_head_reads_for_testing(2),
        )
        .await;
        database.repository.release_paused_head_read_for_testing();
        assert!(
            contender_read.is_err(),
            "contender read the locked stream head before the winner committed"
        );
        let (left, right) = (left.await.unwrap(), right.await.unwrap());
        assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
        let loser = left.err().or_else(|| right.err()).unwrap();
        assert_eq!(loser.code(), "GHE001_SEQUENCE_CONFLICT");
        let event_count: i64 = sqlx::query_scalar("SELECT count(*) FROM graphhelm_events")
            .fetch_one(database.admin_pool())
            .await
            .unwrap();
        assert_eq!(event_count, 2);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn concurrent_exact_retry_returns_the_original_envelopes() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new_with_max_connections(2).await;
        let scope = support::scope("retry-race");
        let request = support::prepared(scope, "stream-retry-race", &["key-retry-race"]);
        database
            .repository
            .pause_first_append_after_head_read_for_testing();
        let left_repository = database.repository.clone();
        let left_request = request.clone();
        let left = tokio::spawn(async move { left_repository.append_atomic(left_request).await });
        database
            .repository
            .wait_for_paused_head_read_for_testing()
            .await;
        let right_repository = database.repository.clone();
        let right = tokio::spawn(async move { right_repository.append_atomic(request).await });
        database.repository.release_paused_head_read_for_testing();
        let left = left.await.unwrap().unwrap();
        let right = right.await.unwrap().unwrap();
        assert_eq!(right, left);
        database.cleanup().await;
    });
}

#[test]
#[ignore = "requires GRAPHHELM_TEST_ADMIN_URL"]
fn read_uses_one_snapshot_while_an_append_commits() {
    support::runtime().block_on(async {
        let database = support::TestDatabase::new_with_max_connections(2).await;
        let scope = support::scope("read-snapshot");
        database
            .repository
            .append_atomic(support::prepared(
                scope.clone(),
                "stream-read-snapshot",
                &["key-read-snapshot-a"],
            ))
            .await
            .unwrap();
        database
            .repository
            .pause_first_append_after_head_read_for_testing();
        let reader = database.repository.clone();
        let read_scope = scope.clone();
        let read = tokio::spawn(async move {
            reader
                .read_stream(
                    ReadStreamRequest::new(
                        read_scope,
                        "stream-read-snapshot".into(),
                        ReadStart::Beginning,
                        10,
                    )
                    .unwrap(),
                )
                .await
        });
        database
            .repository
            .wait_for_paused_head_read_for_testing()
            .await;
        database
            .repository
            .append_atomic(
                graphhelm_events::PreparedAppend::new(
                    scope,
                    graphhelm_protocols::OpaqueId::parse("stream-read-snapshot").unwrap(),
                    2,
                    vec![support::event("key-read-snapshot-b")],
                    vec![],
                    vec![],
                )
                .unwrap(),
            )
            .await
            .unwrap();
        database.repository.release_paused_head_read_for_testing();
        let page = read.await.unwrap().unwrap();
        assert_eq!(page.events.len(), 1);
        assert_eq!(page.head.unwrap().next_sequence, 2);
        database.cleanup().await;
    });
}
