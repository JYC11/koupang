use shared::outbox::dedup;
use shared::test_utils::redis::TestRedis;
use uuid::Uuid;

#[tokio::test]
async fn unpublished_event_returns_false() {
    let redis = TestRedis::start().await;
    let event_id = Uuid::now_v7();

    assert!(!dedup::is_published(&redis.conn, &event_id).await);
}

#[tokio::test]
async fn published_event_returns_true() {
    let redis = TestRedis::start().await;
    let event_id = Uuid::now_v7();

    dedup::mark_published(&redis.conn, &event_id).await;
    assert!(dedup::is_published(&redis.conn, &event_id).await);
}

#[tokio::test]
async fn different_events_are_independent() {
    let redis = TestRedis::start().await;
    let published_id = Uuid::now_v7();
    let other_id = Uuid::now_v7();

    dedup::mark_published(&redis.conn, &published_id).await;

    assert!(dedup::is_published(&redis.conn, &published_id).await);
    assert!(!dedup::is_published(&redis.conn, &other_id).await);
}
