use serde_json::json;
use shared::outbox::{
    claim_batch, cleanup_published, delete_published, insert_outbox_event, mark_published,
    mark_retry_or_failed, oldest_unpublished_age_secs, outbox_lag, release_stale_locks,
    OutboxInsert,
};
use shared::test_utils::db::TestDb;
use uuid::Uuid;

// ── Helper ──────────────────────────────────────────────────────────

fn test_insert(topic: &str, aggregate_id: Uuid) -> OutboxInsert {
    OutboxInsert {
        aggregate_type: "Order".to_string(),
        aggregate_id,
        event_type: "OrderCreated".to_string(),
        event_id: Uuid::now_v7(),
        topic: topic.to_string(),
        partition_key: aggregate_id.to_string(),
        payload: json!({"test": true}),
        metadata: None,
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn insert_outbox_event_creates_row() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();
    let insert = test_insert("orders.events", agg_id);
    let event_id = insert.event_id;

    let row = insert_outbox_event(&db.pool, &insert).await.unwrap();

    assert_eq!(row.aggregate_type, "Order");
    assert_eq!(row.aggregate_id, agg_id);
    assert_eq!(row.event_type, "OrderCreated");
    assert_eq!(row.event_id, event_id);
    assert_eq!(row.topic, "orders.events");
    assert_eq!(row.partition_key, agg_id.to_string());
    assert_eq!(row.payload, json!({"test": true}));
    assert!(row.metadata.is_none());
    assert_eq!(row.status, "pending");
    assert!(row.published_at.is_none());
    assert!(row.locked_by.is_none());
    assert!(row.locked_at.is_none());
    assert_eq!(row.retry_count, 0);
    assert_eq!(row.max_retries, 10);
    assert!(row.last_error.is_none());
}

#[tokio::test]
async fn insert_duplicate_event_id_fails() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();
    let insert = test_insert("orders.events", agg_id);

    // First insert succeeds
    insert_outbox_event(&db.pool, &insert).await.unwrap();

    // Second insert with same event_id should fail (UNIQUE constraint on event_id)
    let result = insert_outbox_event(&db.pool, &insert).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn claim_batch_returns_oldest_per_aggregate() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    // Insert 3 events for the same aggregate — only the oldest should be claimed
    let mut ids = Vec::new();
    for i in 0..3 {
        let mut insert = test_insert("orders.events", agg_id);
        insert.event_type = format!("Event{i}");
        let row = insert_outbox_event(&db.pool, &insert).await.unwrap();
        ids.push(row.id);
    }

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, ids[0], "should claim the oldest event");
    assert_eq!(batch[0].locked_by.as_deref(), Some("relay-1"));
    assert!(batch[0].locked_at.is_some());
}

#[tokio::test]
async fn claim_batch_respects_batch_size() {
    let db = TestDb::start("./tests/migrations").await;

    // Insert events for 5 different aggregates
    for _ in 0..5 {
        let insert = test_insert("orders.events", Uuid::now_v7());
        insert_outbox_event(&db.pool, &insert).await.unwrap();
    }

    let batch = claim_batch(&db.pool, 2, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 2);
}

#[tokio::test]
async fn claim_batch_skips_locked_events() {
    let db = TestDb::start("./tests/migrations").await;
    let agg1 = Uuid::now_v7();
    let agg2 = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg1))
        .await
        .unwrap();
    insert_outbox_event(&db.pool, &test_insert("t", agg2))
        .await
        .unwrap();

    // First claim locks both
    let batch1 = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch1.len(), 2);

    // Second claim should get nothing (both aggregates' oldest events are locked)
    let batch2 = claim_batch(&db.pool, 10, "relay-2").await.unwrap();
    assert!(batch2.is_empty());
}

#[tokio::test]
async fn claim_batch_skips_future_retry() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Push next_retry_at into the future
    sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() + interval '1 hour' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert!(batch.is_empty());
}

#[tokio::test]
async fn mark_published_updates_status() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    let event_id = batch[0].id;

    mark_published(&db.pool, event_id).await.unwrap();

    let row: (String, Option<chrono::DateTime<chrono::Utc>>, Option<String>) = sqlx::query_as(
        "SELECT status, published_at, locked_by FROM outbox_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();

    assert_eq!(row.0, "published");
    assert!(row.1.is_some(), "published_at should be set");
    assert!(row.2.is_none(), "locked_by should be cleared");
}

#[tokio::test]
async fn delete_published_removes_row() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    delete_published(&db.pool, row.id).await.unwrap();

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE id = $1")
            .bind(row.id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(count.0, 0);
}

#[tokio::test]
async fn mark_retry_increments_count() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    let event_id = batch[0].id;

    mark_retry_or_failed(&db.pool, event_id, "kafka timeout")
        .await
        .unwrap();

    let row: (i32, String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT retry_count, status, last_error, locked_by FROM outbox_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();

    assert_eq!(row.0, 1, "retry_count should be 1");
    assert_eq!(row.1, "pending", "status should remain pending");
    assert_eq!(row.2.as_deref(), Some("kafka timeout"));
    assert!(row.3.is_none(), "locked_by should be cleared");

    // Verify next_retry_at is in the future
    let next_retry: (bool,) = sqlx::query_as(
        "SELECT next_retry_at > NOW() FROM outbox_events WHERE id = $1",
    )
    .bind(event_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(next_retry.0, "next_retry_at should be in the future");
}

#[tokio::test]
async fn mark_retry_transitions_to_failed() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Set max_retries to 1 so a single retry exhausts it
    sqlx::query("UPDATE outbox_events SET max_retries = 1 WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    let event_id = batch[0].id;

    mark_retry_or_failed(&db.pool, event_id, "permanent failure")
        .await
        .unwrap();

    let status: (String,) =
        sqlx::query_as("SELECT status FROM outbox_events WHERE id = $1")
            .bind(event_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();

    assert_eq!(status.0, "failed");
}

#[tokio::test]
async fn release_stale_locks_unlocks_old() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    let event_id = batch[0].id;

    // Backdate locked_at to 2 minutes ago
    sqlx::query("UPDATE outbox_events SET locked_at = NOW() - interval '2 minutes' WHERE id = $1")
        .bind(event_id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Release locks older than 60 seconds
    let released = release_stale_locks(&db.pool, 60).await.unwrap();
    assert_eq!(released, 1);

    // Verify the event is now unlocked
    let row: (Option<String>,) =
        sqlx::query_as("SELECT locked_by FROM outbox_events WHERE id = $1")
            .bind(event_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(row.0.is_none());
}

#[tokio::test]
async fn cleanup_published_deletes_old() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Mark published, then backdate published_at
    mark_published(&db.pool, row.id).await.unwrap();
    sqlx::query(
        "UPDATE outbox_events SET published_at = NOW() - interval '8 days' WHERE id = $1",
    )
    .bind(row.id)
    .execute(&db.pool)
    .await
    .unwrap();

    // Cleanup events older than 7 days (604800 seconds)
    let deleted = cleanup_published(&db.pool, 604800).await.unwrap();
    assert_eq!(deleted, 1);

    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE id = $1")
            .bind(row.id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(count.0, 0);
}

#[tokio::test]
async fn outbox_lag_and_oldest_age() {
    let db = TestDb::start("./tests/migrations").await;

    // Empty table: lag = 0, age = None
    let lag = outbox_lag(&db.pool).await.unwrap();
    assert_eq!(lag, 0);

    let age = oldest_unpublished_age_secs(&db.pool).await.unwrap();
    assert!(age.is_none());

    // Insert 3 pending events
    for _ in 0..3 {
        insert_outbox_event(&db.pool, &test_insert("t", Uuid::now_v7()))
            .await
            .unwrap();
    }

    let lag = outbox_lag(&db.pool).await.unwrap();
    assert_eq!(lag, 3);

    let age = oldest_unpublished_age_secs(&db.pool).await.unwrap();
    assert!(age.is_some());
    assert!(age.unwrap() >= 0.0, "age should be non-negative");
}

// ── Concurrency tests ──────────────────────────────────────────────

#[tokio::test]
async fn concurrent_claim_batch_no_overlap() {
    let db = TestDb::start("./tests/migrations").await;

    // Insert events for 10 different aggregates
    for _ in 0..10 {
        insert_outbox_event(&db.pool, &test_insert("t", Uuid::now_v7()))
            .await
            .unwrap();
    }

    // Spawn 5 concurrent relays each claiming batch_size=10
    let pool = db.pool.clone();
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let pool = pool.clone();
            tokio::spawn(async move {
                claim_batch(&pool, 10, &format!("relay-{i}"))
                    .await
                    .unwrap()
            })
        })
        .collect();

    let mut all_claimed_ids = Vec::new();
    for handle in handles {
        let batch = handle.await.unwrap();
        all_claimed_ids.extend(batch.iter().map(|e| e.id));
    }

    // All 10 events should be claimed exactly once — no duplicates
    assert_eq!(all_claimed_ids.len(), 10, "all 10 events should be claimed across relays");
    all_claimed_ids.sort();
    all_claimed_ids.dedup();
    assert_eq!(all_claimed_ids.len(), 10, "no duplicate claims should exist");
}

#[tokio::test]
async fn concurrent_claim_batch_preserves_per_aggregate_ordering() {
    let db = TestDb::start("./tests/migrations").await;

    // Insert 3 events for the same aggregate
    let agg_id = Uuid::now_v7();
    let mut event_ids = Vec::new();
    for i in 0..3 {
        let mut insert = test_insert("t", agg_id);
        insert.event_type = format!("Event{i}");
        let row = insert_outbox_event(&db.pool, &insert).await.unwrap();
        event_ids.push(row.id);
    }

    // Spawn 3 concurrent relays trying to claim
    let pool = db.pool.clone();
    let handles: Vec<_> = (0..3)
        .map(|i| {
            let pool = pool.clone();
            tokio::spawn(async move {
                claim_batch(&pool, 10, &format!("relay-{i}"))
                    .await
                    .unwrap()
            })
        })
        .collect();

    let mut total_claimed = Vec::new();
    for handle in handles {
        total_claimed.extend(handle.await.unwrap());
    }

    // Only the oldest event should be claimed (one relay wins, others get empty)
    assert_eq!(total_claimed.len(), 1, "only one event should be claimable");
    assert_eq!(
        total_claimed[0].id, event_ids[0],
        "the oldest event must be the one claimed"
    );
}

#[tokio::test]
async fn concurrent_claim_and_publish_cycle() {
    let db = TestDb::start("./tests/migrations").await;

    // Insert events for 4 aggregates
    let agg_ids: Vec<Uuid> = (0..4).map(|_| Uuid::now_v7()).collect();
    for agg_id in &agg_ids {
        insert_outbox_event(&db.pool, &test_insert("t", *agg_id))
            .await
            .unwrap();
    }

    // Relay 1 claims all 4
    let batch1 = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch1.len(), 4);

    // Relay 2 gets nothing (all locked)
    let batch2 = claim_batch(&db.pool, 10, "relay-2").await.unwrap();
    assert!(batch2.is_empty());

    // Relay 1 publishes 2 of the 4
    mark_published(&db.pool, batch1[0].id).await.unwrap();
    mark_published(&db.pool, batch1[1].id).await.unwrap();

    // Relay 1 retries the other 2 (simulating failure)
    mark_retry_or_failed(&db.pool, batch1[2].id, "timeout")
        .await
        .unwrap();
    mark_retry_or_failed(&db.pool, batch1[3].id, "timeout")
        .await
        .unwrap();

    // Make the retried events immediately claimable
    sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() WHERE status = 'pending'")
        .execute(&db.pool)
        .await
        .unwrap();

    // Now relay 2 should be able to claim the 2 retried events
    let batch3 = claim_batch(&db.pool, 10, "relay-2").await.unwrap();
    assert_eq!(batch3.len(), 2, "relay-2 should pick up the 2 retried events");

    // Verify no overlap with already-published events
    let published_ids: Vec<Uuid> = vec![batch1[0].id, batch1[1].id];
    for event in &batch3 {
        assert!(
            !published_ids.contains(&event.id),
            "should not re-claim a published event"
        );
    }
}
