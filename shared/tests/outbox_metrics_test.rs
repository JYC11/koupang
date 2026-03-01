use serde_json::json;
use shared::outbox::{
    OutboxInsert, claim_batch, collect_outbox_metrics, insert_outbox_event, mark_published,
    mark_retry_or_failed,
};
use shared::test_utils::db::TestDb;
use uuid::Uuid;

fn test_insert(aggregate_id: Uuid) -> OutboxInsert {
    OutboxInsert {
        aggregate_type: "Order".to_string(),
        aggregate_id,
        event_type: "OrderCreated".to_string(),
        event_id: Uuid::now_v7(),
        topic: "orders.events".to_string(),
        partition_key: aggregate_id.to_string(),
        payload: json!({"test": true}),
        metadata: None,
    }
}

#[tokio::test]
async fn collect_metrics_empty_table() {
    let db = TestDb::start("./tests/migrations").await;

    let metrics = collect_outbox_metrics(&db.pool).await.unwrap();

    assert_eq!(metrics.pending_count, 0);
    assert_eq!(metrics.failed_count, 0);
    assert_eq!(metrics.published_count, 0);
    assert!(metrics.oldest_pending_age_secs.is_none());
}

#[tokio::test]
async fn collect_metrics_with_mixed_statuses() {
    let db = TestDb::start("./tests/migrations").await;

    // Insert 3 pending events (different aggregates so we can claim them all)
    let agg1 = Uuid::now_v7();
    let agg2 = Uuid::now_v7();
    let agg3 = Uuid::now_v7();
    insert_outbox_event(&db.pool, &test_insert(agg1))
        .await
        .unwrap();
    insert_outbox_event(&db.pool, &test_insert(agg2))
        .await
        .unwrap();
    insert_outbox_event(&db.pool, &test_insert(agg3))
        .await
        .unwrap();

    // Publish one
    let batch = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    mark_published(&db.pool, batch[0].id).await.unwrap();

    // Fail one (set max_retries=1 so a single retry transitions to failed)
    let batch = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    sqlx::query("UPDATE outbox_events SET max_retries = 1 WHERE id = $1")
        .bind(batch[0].id)
        .execute(&db.pool)
        .await
        .unwrap();
    mark_retry_or_failed(&db.pool, batch[0].id, "permanent error")
        .await
        .unwrap();

    // 1 still pending, 1 published, 1 failed
    let metrics = collect_outbox_metrics(&db.pool).await.unwrap();

    assert_eq!(metrics.pending_count, 1);
    assert_eq!(metrics.published_count, 1);
    assert_eq!(metrics.failed_count, 1);
    assert!(metrics.oldest_pending_age_secs.is_some());
    assert!(metrics.oldest_pending_age_secs.unwrap() >= 0.0);
}
