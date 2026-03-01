use serde_json::json;
use shared::outbox::{insert_outbox_event, OutboxInsert};
use shared::test_utils::db::TestDb;
use uuid::Uuid;

fn test_insert(agg_id: Uuid) -> OutboxInsert {
    OutboxInsert {
        aggregate_type: "Order".to_string(),
        aggregate_id: agg_id,
        event_type: "OrderCreated".to_string(),
        event_id: Uuid::now_v7(),
        topic: "orders.events".to_string(),
        partition_key: agg_id.to_string(),
        payload: json!({"test": true}),
        metadata: None,
    }
}

#[tokio::test]
async fn outbox_migrations_run_successfully() {
    let db = TestDb::start("./tests/migrations").await;

    // Verify outbox_events table exists and has expected columns
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM outbox_events")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0);

    // Verify the trigger function exists
    let trigger_exists: (bool,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM pg_trigger WHERE tgname = 'outbox_events_after_insert'
        )",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(trigger_exists.0);
}

#[tokio::test]
async fn processed_events_migration_runs_successfully() {
    let db = TestDb::start("./tests/migrations").await;

    // Verify processed_events table exists
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM processed_events")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(row.0, 0);
}

#[tokio::test]
async fn outbox_status_check_constraint_enforced() {
    let db = TestDb::start("./tests/migrations").await;

    // Valid status values should work
    let valid_id = uuid::Uuid::now_v7();
    let event_id = uuid::Uuid::now_v7();
    let agg_id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO outbox_events (id, aggregate_type, aggregate_id, event_type, event_id, topic, partition_key, payload, status)
         VALUES ($1, 'Order', $2, 'OrderCreated', $3, 'orders.events', $4, '{}'::jsonb, 'pending')",
    )
    .bind(valid_id)
    .bind(agg_id)
    .bind(event_id)
    .bind(agg_id.to_string())
    .execute(&db.pool)
    .await
    .unwrap();

    // Invalid status should be rejected by CHECK constraint
    let bad_id = uuid::Uuid::now_v7();
    let bad_event_id = uuid::Uuid::now_v7();
    let result = sqlx::query(
        "INSERT INTO outbox_events (id, aggregate_type, aggregate_id, event_type, event_id, topic, partition_key, payload, status)
         VALUES ($1, 'Order', $2, 'OrderCreated', $3, 'orders.events', $4, '{}'::jsonb, 'invalid')",
    )
    .bind(bad_id)
    .bind(agg_id)
    .bind(bad_event_id)
    .bind(agg_id.to_string())
    .execute(&db.pool)
    .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("chk_outbox_status"), "Expected check constraint violation, got: {err}");
}

// ── Status transition trigger tests ───────────────────────────────

#[tokio::test]
async fn status_transition_trigger_exists() {
    let db = TestDb::start("./tests/migrations").await;

    let exists: (bool,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1 FROM pg_trigger WHERE tgname = 'outbox_enforce_status_transition'
        )",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(exists.0, "status transition trigger should exist");
}

// ── Valid transitions ─────────────────────────────────────────────

#[tokio::test]
async fn transition_pending_to_pending_allowed() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    // pending → pending (retry: increment retry_count, stays pending)
    let result = sqlx::query(
        "UPDATE outbox_events SET status = 'pending', retry_count = retry_count + 1 WHERE id = $1",
    )
    .bind(row.id)
    .execute(&db.pool)
    .await;
    assert!(result.is_ok(), "pending → pending should be allowed");
}

#[tokio::test]
async fn transition_pending_to_published_allowed() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    let result = sqlx::query("UPDATE outbox_events SET status = 'published' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_ok(), "pending → published should be allowed");
}

#[tokio::test]
async fn transition_pending_to_failed_allowed() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    let result = sqlx::query("UPDATE outbox_events SET status = 'failed' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_ok(), "pending → failed should be allowed");
}

#[tokio::test]
async fn transition_published_to_published_allowed() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    // First move to published
    sqlx::query("UPDATE outbox_events SET status = 'published' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    // published → published (idempotent mark_published)
    let result = sqlx::query("UPDATE outbox_events SET status = 'published' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_ok(), "published → published should be allowed (idempotent)");
}

// ── Invalid transitions ──────────────────────────────────────────

#[tokio::test]
async fn transition_published_to_pending_rejected() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    sqlx::query("UPDATE outbox_events SET status = 'published' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let result = sqlx::query("UPDATE outbox_events SET status = 'pending' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("invalid outbox status transition: published"),
        "should reject published → pending, got: {err}"
    );
}

#[tokio::test]
async fn transition_published_to_failed_rejected() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    sqlx::query("UPDATE outbox_events SET status = 'published' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let result = sqlx::query("UPDATE outbox_events SET status = 'failed' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("invalid outbox status transition: published"),
        "should reject published → failed, got: {err}"
    );
}

#[tokio::test]
async fn transition_failed_to_pending_rejected() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    sqlx::query("UPDATE outbox_events SET status = 'failed' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let result = sqlx::query("UPDATE outbox_events SET status = 'pending' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("invalid outbox status transition: failed"),
        "should reject failed → pending, got: {err}"
    );
}

#[tokio::test]
async fn transition_failed_to_published_rejected() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    sqlx::query("UPDATE outbox_events SET status = 'failed' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let result = sqlx::query("UPDATE outbox_events SET status = 'published' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("invalid outbox status transition: failed"),
        "should reject failed → published, got: {err}"
    );
}

/// Self-transitions are allowed (idempotent no-ops), same as published → published.
#[tokio::test]
async fn transition_failed_to_failed_allowed() {
    let db = TestDb::start("./tests/migrations").await;
    let row = insert_outbox_event(&db.pool, &test_insert(Uuid::now_v7()))
        .await
        .unwrap();

    sqlx::query("UPDATE outbox_events SET status = 'failed' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    let result = sqlx::query("UPDATE outbox_events SET status = 'failed' WHERE id = $1")
        .bind(row.id)
        .execute(&db.pool)
        .await;
    assert!(result.is_ok(), "failed → failed should be allowed (self-transition)");
}
