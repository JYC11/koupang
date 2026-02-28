use shared::test_utils::db::TestDb;

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
