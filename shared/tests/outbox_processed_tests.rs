use shared::outbox::{cleanup_processed_events, is_event_processed, mark_event_processed};
use shared::test_utils::db::TestDb;
use uuid::Uuid;

#[tokio::test]
async fn is_event_processed_returns_false_for_unknown() {
    let db = TestDb::start("./tests/migrations").await;

    let result = is_event_processed(&db.pool, Uuid::now_v7()).await.unwrap();
    assert!(!result);
}

#[tokio::test]
async fn mark_and_check_processed() {
    let db = TestDb::start("./tests/migrations").await;

    let event_id = Uuid::now_v7();

    // Before marking — should be false
    assert!(!is_event_processed(&db.pool, event_id).await.unwrap());

    // Mark it
    mark_event_processed(&db.pool, event_id, "OrderCreated", "order")
        .await
        .unwrap();

    // After marking — should be true
    assert!(is_event_processed(&db.pool, event_id).await.unwrap());
}

#[tokio::test]
async fn mark_processed_is_idempotent() {
    let db = TestDb::start("./tests/migrations").await;

    let event_id = Uuid::now_v7();

    // Mark twice — second call should not error (ON CONFLICT DO NOTHING)
    mark_event_processed(&db.pool, event_id, "OrderCreated", "order")
        .await
        .unwrap();
    mark_event_processed(&db.pool, event_id, "OrderCreated", "order")
        .await
        .unwrap();

    assert!(is_event_processed(&db.pool, event_id).await.unwrap());
}

#[tokio::test]
async fn cleanup_deletes_old_events() {
    let db = TestDb::start("./tests/migrations").await;

    let event_id = Uuid::now_v7();

    // Insert an event
    mark_event_processed(&db.pool, event_id, "OrderCreated", "order")
        .await
        .unwrap();

    // Backdate the processed_at to 1 hour ago
    sqlx::query("UPDATE processed_events SET processed_at = NOW() - INTERVAL '1 hour' WHERE event_id = $1")
        .bind(event_id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Cleanup with max_age = 0 seconds — should delete everything
    let deleted = cleanup_processed_events(&db.pool, 0).await.unwrap();
    assert_eq!(deleted, 1);

    // Event should no longer exist
    assert!(!is_event_processed(&db.pool, event_id).await.unwrap());
}
