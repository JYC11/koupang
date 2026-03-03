use serde_json::json;
use shared::outbox::{
    OutboxInsert, claim_batch, cleanup_published, delete_published, insert_outbox_event,
    mark_published, mark_retry_or_failed, oldest_unpublished_age_secs, outbox_lag,
    release_stale_locks,
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
    assert_eq!(row.status, shared::outbox::OutboxStatus::Pending);
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

    let row: (
        String,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<String>,
    ) = sqlx::query_as("SELECT status, published_at, locked_by FROM outbox_events WHERE id = $1")
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

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE id = $1")
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
    let next_retry: (bool,) =
        sqlx::query_as("SELECT next_retry_at > NOW() FROM outbox_events WHERE id = $1")
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

    let status: (String,) = sqlx::query_as("SELECT status FROM outbox_events WHERE id = $1")
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
    sqlx::query("UPDATE outbox_events SET published_at = NOW() - interval '8 days' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Cleanup events older than 7 days (604800 seconds)
    let deleted = cleanup_published(&db.pool, 604800).await.unwrap();
    assert_eq!(deleted, 1);

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE id = $1")
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
            tokio::spawn(
                async move { claim_batch(&pool, 10, &format!("relay-{i}")).await.unwrap() },
            )
        })
        .collect();

    let mut all_claimed_ids = Vec::new();
    for handle in handles {
        let batch = handle.await.unwrap();
        all_claimed_ids.extend(batch.iter().map(|e| e.id));
    }

    // All 10 events should be claimed exactly once — no duplicates
    assert_eq!(
        all_claimed_ids.len(),
        10,
        "all 10 events should be claimed across relays"
    );
    all_claimed_ids.sort();
    all_claimed_ids.dedup();
    assert_eq!(
        all_claimed_ids.len(),
        10,
        "no duplicate claims should exist"
    );
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
            tokio::spawn(
                async move { claim_batch(&pool, 10, &format!("relay-{i}")).await.unwrap() },
            )
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
    assert_eq!(
        batch3.len(),
        2,
        "relay-2 should pick up the 2 retried events"
    );

    // Verify no overlap with already-published events
    let published_ids: Vec<Uuid> = vec![batch1[0].id, batch1[1].id];
    for event in &batch3 {
        assert!(
            !published_ids.contains(&event.id),
            "should not re-claim a published event"
        );
    }
}

// ── Ordering guarantees ───────────────────────────────────────────

/// The core outbox guarantee: for the same aggregate, events are delivered
/// in insertion order. After publishing event N, event N+1 becomes claimable.
#[tokio::test]
async fn sequential_ordering_across_claim_publish_cycles() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    // Insert 3 events for the same aggregate
    let mut event_ids = Vec::new();
    for i in 0..3 {
        let mut insert = test_insert("t", agg_id);
        insert.event_type = format!("Event{i}");
        let row = insert_outbox_event(&db.pool, &insert).await.unwrap();
        event_ids.push(row.id);
    }

    // Cycle 1: only oldest is claimable
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, event_ids[0]);
    mark_published(&db.pool, batch[0].id).await.unwrap();

    // Cycle 2: second event now claimable
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, event_ids[1]);
    mark_published(&db.pool, batch[0].id).await.unwrap();

    // Cycle 3: third event now claimable
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, event_ids[2]);
    mark_published(&db.pool, batch[0].id).await.unwrap();

    // No more events
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert!(batch.is_empty());
}

/// Mixed aggregates: A has 3 events, B has 1, C has 2.
/// claim_batch should return exactly 3 events (oldest per aggregate).
#[tokio::test]
async fn claim_batch_mixed_aggregate_counts() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_a = Uuid::now_v7();
    let agg_b = Uuid::now_v7();
    let agg_c = Uuid::now_v7();

    let mut expected_ids = Vec::new();

    // A: 3 events — only first should be claimed
    for i in 0..3 {
        let mut insert = test_insert("t", agg_a);
        insert.event_type = format!("A-Event{i}");
        let row = insert_outbox_event(&db.pool, &insert).await.unwrap();
        if i == 0 {
            expected_ids.push(row.id);
        }
    }

    // B: 1 event — should be claimed
    let insert = test_insert("t", agg_b);
    let row = insert_outbox_event(&db.pool, &insert).await.unwrap();
    expected_ids.push(row.id);

    // C: 2 events — only first should be claimed
    for i in 0..2 {
        let mut insert = test_insert("t", agg_c);
        insert.event_type = format!("C-Event{i}");
        let row = insert_outbox_event(&db.pool, &insert).await.unwrap();
        if i == 0 {
            expected_ids.push(row.id);
        }
    }

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 3, "should claim exactly one per aggregate");

    let claimed_ids: Vec<Uuid> = batch.iter().map(|e| e.id).collect();
    for expected in &expected_ids {
        assert!(
            claimed_ids.contains(expected),
            "expected event {expected} to be claimed"
        );
    }
}

// ── Exponential backoff correctness ───────────────────────────────

/// Verify actual backoff intervals: 2^1=2s, 2^2=4s, 2^3=8s.
#[tokio::test]
async fn exponential_backoff_values() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Retry 1: backoff should be ~2 seconds
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "err1")
        .await
        .unwrap();

    let delay_secs: (f64,) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM (next_retry_at - NOW()))::float8 FROM outbox_events WHERE id = $1",
    )
    .bind(row.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    // 2^1 = 2 seconds, allow 0.5s tolerance for query execution time
    assert!(
        delay_secs.0 > 1.0 && delay_secs.0 < 3.0,
        "retry 1 backoff should be ~2s, got {:.1}s",
        delay_secs.0
    );

    // Make immediately retryable for next test
    sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Retry 2: backoff should be ~4 seconds
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "err2")
        .await
        .unwrap();

    let delay_secs: (f64,) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM (next_retry_at - NOW()))::float8 FROM outbox_events WHERE id = $1",
    )
    .bind(row.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(
        delay_secs.0 > 3.0 && delay_secs.0 < 5.5,
        "retry 2 backoff should be ~4s, got {:.1}s",
        delay_secs.0
    );

    // Make immediately retryable again
    sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Retry 3: backoff should be ~8 seconds
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "err3")
        .await
        .unwrap();

    let delay_secs: (f64,) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM (next_retry_at - NOW()))::float8 FROM outbox_events WHERE id = $1",
    )
    .bind(row.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(
        delay_secs.0 > 7.0 && delay_secs.0 < 9.5,
        "retry 3 backoff should be ~8s, got {:.1}s",
        delay_secs.0
    );
}

/// Backoff caps at 2^10 = 1024 seconds regardless of retry count.
#[tokio::test]
async fn exponential_backoff_caps_at_2_pow_10() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Set retry_count to 15 (well past cap) and max_retries to 20 so it doesn't fail
    sqlx::query("UPDATE outbox_events SET retry_count = 15, max_retries = 20 WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "err")
        .await
        .unwrap();

    let delay_secs: (f64,) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM (next_retry_at - NOW()))::float8 FROM outbox_events WHERE id = $1",
    )
    .bind(row.id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    // 2^10 = 1024, LEAST(16, 10) = 10, so capped at 1024s
    assert!(
        delay_secs.0 > 1000.0 && delay_secs.0 < 1050.0,
        "backoff should cap at ~1024s, got {:.1}s",
        delay_secs.0
    );
}

// ── Progressive retry exhaustion ──────────────────────────────────

/// Walk an event through retries 0 → max_retries, verify state at each step.
#[tokio::test]
async fn progressive_retry_through_exhaustion() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Set max_retries to 3 for a shorter test
    sqlx::query("UPDATE outbox_events SET max_retries = 3 WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    for retry in 1..=3 {
        // Make claimable
        sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() WHERE id = $1")
            .bind(row.id)
            .execute(&db.pool)
            .await
            .unwrap();

        let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
        assert_eq!(batch.len(), 1, "retry {retry}: event should be claimable");

        let error_msg = format!("failure #{retry}");
        mark_retry_or_failed(&db.pool, batch[0].id, &error_msg)
            .await
            .unwrap();

        let (status, count, err): (String, i32, Option<String>) = sqlx::query_as(
            "SELECT status, retry_count, last_error FROM outbox_events WHERE id = $1",
        )
        .bind(row.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();

        assert_eq!(count, retry, "retry_count should be {retry}");
        assert_eq!(err.as_deref(), Some(error_msg.as_str()));

        if retry < 3 {
            assert_eq!(status, "pending", "retry {retry}: should stay pending");
        } else {
            assert_eq!(
                status, "failed",
                "retry {retry}: should transition to failed"
            );
        }
    }
}

/// max_retries=0: first retry immediately transitions to failed.
#[tokio::test]
async fn max_retries_zero_immediate_failure() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    sqlx::query("UPDATE outbox_events SET max_retries = 0 WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "instant fail")
        .await
        .unwrap();

    let status: (String,) = sqlx::query_as("SELECT status FROM outbox_events WHERE id = $1")
        .bind(row.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(status.0, "failed", "max_retries=0 should fail immediately");
}

// ── State machine invariants ──────────────────────────────────────

/// Published events are never returned by claim_batch.
#[tokio::test]
async fn published_events_are_not_reclaimable() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_published(&db.pool, batch[0].id).await.unwrap();

    // Try to claim again — should get nothing
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert!(batch.is_empty(), "published events must not be reclaimed");

    // Even from a different relay
    let batch = claim_batch(&db.pool, 10, "relay-2").await.unwrap();
    assert!(
        batch.is_empty(),
        "published events must not be reclaimed by any relay"
    );
}

/// Failed events (retries exhausted) are never returned by claim_batch.
#[tokio::test]
async fn failed_events_are_not_reclaimable() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    let row = insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Force to failed state
    sqlx::query("UPDATE outbox_events SET max_retries = 1 WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "permanent")
        .await
        .unwrap();

    // Verify it's failed
    let status: (String,) = sqlx::query_as("SELECT status FROM outbox_events WHERE id = $1")
        .bind(row.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(status.0, "failed");

    // Try to claim — should get nothing
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert!(batch.is_empty(), "failed events must not be reclaimed");
}

/// Failed events behind pending events do not block the aggregate.
/// If aggregate A has events [failed, pending], the pending event
/// should still be claimable (the failed one is terminal).
#[tokio::test]
async fn failed_event_does_not_block_subsequent_pending_events() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    // Insert event 1 (will become failed)
    let mut insert1 = test_insert("t", agg_id);
    insert1.event_type = "WillFail".to_string();
    let row1 = insert_outbox_event(&db.pool, &insert1).await.unwrap();

    // Insert event 2 (should remain pending)
    let mut insert2 = test_insert("t", agg_id);
    insert2.event_type = "ShouldSucceed".to_string();
    let _row2 = insert_outbox_event(&db.pool, &insert2).await.unwrap();

    // Force event 1 to failed
    sqlx::query("UPDATE outbox_events SET status = 'failed', retry_count = 10, max_retries = 10 WHERE id = $1")
        .bind(row1.id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Claim — event 2 should be claimable since DISTINCT ON picks oldest pending
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(
        batch.len(),
        1,
        "pending event after failed should be claimable"
    );
    assert_eq!(batch[0].event_type, "ShouldSucceed");
}

/// claim_batch on empty table returns empty vec (not an error).
#[tokio::test]
async fn claim_batch_empty_table() {
    let db = TestDb::start("./tests/migrations").await;

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert!(batch.is_empty());
}

/// claim_batch only returns pending events, not published or failed.
#[tokio::test]
async fn claim_batch_only_returns_pending() {
    let db = TestDb::start("./tests/migrations").await;

    // Create 3 events for different aggregates
    let agg_pending = Uuid::now_v7();
    let agg_published = Uuid::now_v7();
    let agg_failed = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_pending))
        .await
        .unwrap();
    let pub_row = insert_outbox_event(&db.pool, &test_insert("t", agg_published))
        .await
        .unwrap();
    let fail_row = insert_outbox_event(&db.pool, &test_insert("t", agg_failed))
        .await
        .unwrap();

    // Manually set statuses
    sqlx::query(
        "UPDATE outbox_events SET status = 'published', published_at = NOW() WHERE id = $1",
    )
    .bind(pub_row.id)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE outbox_events SET status = 'failed' WHERE id = $1")
        .bind(fail_row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1, "only the pending event should be claimed");
    assert_eq!(batch[0].aggregate_id, agg_pending);
}

// ── Recovery path ─────────────────────────────────────────────────

/// Full lifecycle: lock → stale → release → re-claim succeeds.
#[tokio::test]
async fn stale_lock_release_then_reclaim() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Relay 1 claims
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    let event_id = batch[0].id;

    // Relay 1 crashes (simulated: lock goes stale)
    sqlx::query("UPDATE outbox_events SET locked_at = NOW() - interval '5 minutes' WHERE id = $1")
        .bind(event_id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Maintenance releases stale locks
    let released = release_stale_locks(&db.pool, 60).await.unwrap();
    assert_eq!(released, 1);

    // Relay 2 can now claim the same event
    let batch = claim_batch(&db.pool, 10, "relay-2").await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, event_id);
    assert_eq!(batch[0].locked_by.as_deref(), Some("relay-2"));
}

/// Fresh locks (recently acquired) are NOT released by stale lock cleanup.
#[tokio::test]
async fn release_stale_locks_ignores_fresh_locks() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Claim (locked_at = NOW())
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);

    // Release stale locks with 60s timeout — should NOT release this fresh lock
    let released = release_stale_locks(&db.pool, 60).await.unwrap();
    assert_eq!(released, 0, "fresh locks should not be released");

    // Verify still locked
    let locked: (Option<String>,) =
        sqlx::query_as("SELECT locked_by FROM outbox_events WHERE id = $1")
            .bind(batch[0].id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(locked.0.as_deref(), Some("relay-1"));
}

// ── Cleanup precision ─────────────────────────────────────────────

/// Cleanup only deletes published events; pending and failed are untouched.
#[tokio::test]
async fn cleanup_ignores_pending_and_failed() {
    let db = TestDb::start("./tests/migrations").await;

    let pending_row = insert_outbox_event(&db.pool, &test_insert("t", Uuid::now_v7()))
        .await
        .unwrap();
    let failed_row = insert_outbox_event(&db.pool, &test_insert("t", Uuid::now_v7()))
        .await
        .unwrap();

    // Force timestamps into the past
    sqlx::query("UPDATE outbox_events SET created_at = NOW() - interval '30 days' WHERE id = $1")
        .bind(pending_row.id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE outbox_events SET status = 'failed', created_at = NOW() - interval '30 days' WHERE id = $1",
    )
    .bind(failed_row.id)
    .execute(&db.pool)
    .await
    .unwrap();

    // Cleanup with 0s max_age (delete everything published)
    let deleted = cleanup_published(&db.pool, 0).await.unwrap();
    assert_eq!(deleted, 0, "no published events exist to clean up");

    // Both events still exist
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM outbox_events")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count.0, 2);
}

/// Recently published events are NOT cleaned up.
#[tokio::test]
async fn cleanup_spares_recent_published() {
    let db = TestDb::start("./tests/migrations").await;

    let row = insert_outbox_event(&db.pool, &test_insert("t", Uuid::now_v7()))
        .await
        .unwrap();
    mark_published(&db.pool, row.id).await.unwrap();

    // Cleanup events older than 7 days — this one is fresh
    let deleted = cleanup_published(&db.pool, 604800).await.unwrap();
    assert_eq!(
        deleted, 0,
        "recently published events should survive cleanup"
    );

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE id = $1")
        .bind(row.id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

// ── Error message and metadata preservation ───────────────────────

/// Each retry overwrites last_error with the latest error message.
#[tokio::test]
async fn last_error_overwritten_on_each_retry() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    // Retry 1
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "kafka connection refused")
        .await
        .unwrap();

    let err: (Option<String>,) =
        sqlx::query_as("SELECT last_error FROM outbox_events WHERE id = $1")
            .bind(batch[0].id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(err.0.as_deref(), Some("kafka connection refused"));

    // Make retryable
    sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() WHERE id = $1")
        .bind(batch[0].id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Retry 2 with different error
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "kafka topic not found")
        .await
        .unwrap();

    let err: (Option<String>,) =
        sqlx::query_as("SELECT last_error FROM outbox_events WHERE id = $1")
            .bind(batch[0].id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(
        err.0.as_deref(),
        Some("kafka topic not found"),
        "last_error should reflect the most recent failure"
    );
}

/// Metadata (trace context) survives through the claim cycle intact.
#[tokio::test]
async fn metadata_preserved_through_claim() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();
    let trace_ctx = json!({"trace_id": "abc123", "span_id": "def456"});

    let mut insert = test_insert("t", agg_id);
    insert.metadata = Some(trace_ctx.clone());
    insert_outbox_event(&db.pool, &insert).await.unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch[0].metadata,
        Some(trace_ctx),
        "metadata should be preserved after claim"
    );
}

/// Payload (event envelope) is preserved exactly through claim and publish.
#[tokio::test]
async fn payload_preserved_through_lifecycle() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();
    let payload = json!({
        "order_id": agg_id.to_string(),
        "items": [{"sku": "ABC-123", "qty": 2, "price": "29.99"}],
        "total": "59.98"
    });

    let mut insert = test_insert("t", agg_id);
    insert.payload = payload.clone();
    insert_outbox_event(&db.pool, &insert).await.unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(
        batch[0].payload, payload,
        "payload must survive claim intact"
    );

    mark_published(&db.pool, batch[0].id).await.unwrap();

    // Read back from DB — still intact
    let row: (serde_json::Value,) =
        sqlx::query_as("SELECT payload FROM outbox_events WHERE id = $1")
            .bind(batch[0].id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row.0, payload, "payload must survive publish intact");
}

// ── Idempotency ───────────────────────────────────────────────────

/// mark_published called twice on the same event does not error.
#[tokio::test]
async fn mark_published_is_idempotent() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    insert_outbox_event(&db.pool, &test_insert("t", agg_id))
        .await
        .unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    let event_id = batch[0].id;

    // Publish twice — should not error
    mark_published(&db.pool, event_id).await.unwrap();
    mark_published(&db.pool, event_id).await.unwrap();

    let status: (String,) = sqlx::query_as("SELECT status FROM outbox_events WHERE id = $1")
        .bind(event_id)
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(status.0, "published");
}

/// delete_published on nonexistent event does not error (0 rows affected).
#[tokio::test]
async fn delete_published_nonexistent_is_noop() {
    let db = TestDb::start("./tests/migrations").await;
    let phantom_id = Uuid::now_v7();

    // Should not error — just deletes 0 rows
    delete_published(&db.pool, phantom_id).await.unwrap();
}

// ── High contention ───────────────────────────────────────────────

/// 20 concurrent relays compete for 3 events — exactly 3 claimed, no duplicates.
#[tokio::test]
async fn high_contention_many_relays_few_events() {
    let db = TestDb::start("./tests/migrations").await;

    for _ in 0..3 {
        insert_outbox_event(&db.pool, &test_insert("t", Uuid::now_v7()))
            .await
            .unwrap();
    }

    let pool = db.pool.clone();
    let handles: Vec<_> = (0..20)
        .map(|i| {
            let pool = pool.clone();
            tokio::spawn(
                async move { claim_batch(&pool, 10, &format!("relay-{i}")).await.unwrap() },
            )
        })
        .collect();

    let mut all_claimed = Vec::new();
    for handle in handles {
        all_claimed.extend(handle.await.unwrap());
    }

    assert_eq!(
        all_claimed.len(),
        3,
        "exactly 3 events should be claimed total"
    );

    let mut ids: Vec<Uuid> = all_claimed.iter().map(|e| e.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 3, "no duplicate claims");
}

/// Concurrent claims interleaved with publishes: relay-1 claims and publishes
/// while relay-2 tries to claim the same aggregate's next event.
#[tokio::test]
async fn concurrent_claim_interleaved_with_publish() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    // Insert 2 events for the same aggregate
    let mut ids = Vec::new();
    for i in 0..2 {
        let mut insert = test_insert("t", agg_id);
        insert.event_type = format!("Event{i}");
        let row = insert_outbox_event(&db.pool, &insert).await.unwrap();
        ids.push(row.id);
    }

    // Relay-1 claims event 0
    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].id, ids[0]);

    // While locked, relay-2 gets nothing
    let batch2 = claim_batch(&db.pool, 10, "relay-2").await.unwrap();
    assert!(batch2.is_empty());

    // Relay-1 publishes event 0
    mark_published(&db.pool, ids[0]).await.unwrap();

    // Now relay-2 can claim event 1
    let batch3 = claim_batch(&db.pool, 10, "relay-2").await.unwrap();
    assert_eq!(batch3.len(), 1);
    assert_eq!(batch3[0].id, ids[1]);
    assert_eq!(batch3[0].locked_by.as_deref(), Some("relay-2"));
}

// ── Edge cases ────────────────────────────────────────────────────

/// Very large payload round-trips correctly.
#[tokio::test]
async fn large_payload_round_trip() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    // Generate a payload with 1000 items
    let items: Vec<serde_json::Value> = (0..1000)
        .map(|i| json!({"item_id": i, "name": format!("Product {i}"), "price": "9.99"}))
        .collect();
    let payload = json!({"items": items});

    let mut insert = test_insert("t", agg_id);
    insert.payload = payload.clone();
    let row = insert_outbox_event(&db.pool, &insert).await.unwrap();

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch[0].id, row.id);
    assert_eq!(batch[0].payload, payload);
}

/// Multiple events inserted within the same transaction are all visible after commit.
#[tokio::test]
async fn events_inserted_in_transaction_visible_after_commit() {
    let db = TestDb::start("./tests/migrations").await;
    let agg_id = Uuid::now_v7();

    // Insert 3 events in a single transaction
    let mut tx = db.pool.begin().await.unwrap();
    for i in 0..3 {
        let mut insert = test_insert("t", agg_id);
        insert.event_type = format!("TxEvent{i}");
        insert_outbox_event(&mut *tx, &insert).await.unwrap();
    }
    tx.commit().await.unwrap();

    // All 3 should exist, but only oldest is claimable (per-aggregate ordering)
    let lag = outbox_lag(&db.pool).await.unwrap();
    assert_eq!(lag, 3);

    let batch = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch.len(), 1, "only oldest per aggregate");
    assert_eq!(batch[0].event_type, "TxEvent0");
}
