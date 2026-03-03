//! Integration tests for OutboxRelay — the background task that reads pending
//! outbox events and publishes them to Kafka.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use shared::errors::AppError;
use shared::events::{
    AggregateType, EventEnvelope, EventMetadata, EventPublisher, EventType, KafkaAdmin,
    KafkaEventPublisher, SourceService, TopicSpec,
};
use shared::outbox::{
    FailureEscalation, OutboxEvent, OutboxInsert, OutboxRelay, RelayConfig, collect_outbox_metrics,
    insert_outbox_event,
};
use shared::test_utils::db::TestDb;
use shared::test_utils::kafka::{TestConsumer, TestKafka};

const MIGRATIONS: &str = "tests/migrations";

fn unique_topic() -> String {
    format!("test-{}", Uuid::now_v7())
}

fn order_envelope(aggregate_id: Uuid) -> EventEnvelope {
    let metadata = EventMetadata::new(
        EventType::OrderCreated,
        AggregateType::Order,
        aggregate_id,
        SourceService::Order,
    );
    EventEnvelope::new(
        metadata,
        json!({"order_id": aggregate_id.to_string(), "total": "99.99"}),
    )
}

fn fast_relay_config() -> RelayConfig {
    RelayConfig {
        poll_interval: Duration::from_millis(50),
        stale_lock_check_interval: Duration::from_secs(1),
        stale_lock_timeout: Duration::from_secs(2),
        cleanup_interval: Duration::from_secs(3600),
        batch_size: 50,
        ..Default::default()
    }
}

// ── Test helpers ────────────────────────────────────────────────────

/// Publisher that fails the first N times, then delegates to a real publisher.
struct FailingPublisher {
    inner: Arc<dyn EventPublisher>,
    remaining_failures: AtomicU32,
}

impl FailingPublisher {
    fn new(inner: Arc<dyn EventPublisher>, fail_count: u32) -> Self {
        Self {
            inner,
            remaining_failures: AtomicU32::new(fail_count),
        }
    }
}

#[async_trait::async_trait]
impl EventPublisher for FailingPublisher {
    async fn publish(&self, topic: &str, envelope: &EventEnvelope) -> Result<(), AppError> {
        let remaining = self.remaining_failures.load(Ordering::SeqCst);
        if remaining > 0 {
            self.remaining_failures.fetch_sub(1, Ordering::SeqCst);
            return Err(AppError::InternalServerError(
                "Simulated publish failure".to_string(),
            ));
        }
        self.inner.publish(topic, envelope).await
    }
}

/// Publisher that always fails (for escalation tests).
struct AlwaysFailPublisher;

#[async_trait::async_trait]
impl EventPublisher for AlwaysFailPublisher {
    async fn publish(&self, _topic: &str, _envelope: &EventEnvelope) -> Result<(), AppError> {
        Err(AppError::InternalServerError(
            "Permanent failure".to_string(),
        ))
    }
}

/// Escalation handler that records failed event IDs.
struct TrackingEscalation {
    failed_ids: Arc<Mutex<Vec<Uuid>>>,
}

impl TrackingEscalation {
    fn new() -> (Self, Arc<Mutex<Vec<Uuid>>>) {
        let ids = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                failed_ids: Arc::clone(&ids),
            },
            ids,
        )
    }
}

#[async_trait::async_trait]
impl FailureEscalation for TrackingEscalation {
    async fn on_permanent_failure(&self, event: &OutboxEvent) -> Result<(), AppError> {
        self.failed_ids.lock().unwrap().push(event.event_id);
        Ok(())
    }
}

/// Start the relay in the background, returning a shutdown token.
fn start_relay(
    pool: shared::db::PgPool,
    publisher: Arc<dyn EventPublisher>,
    config: RelayConfig,
) -> CancellationToken {
    let shutdown = CancellationToken::new();
    let relay = OutboxRelay::new(pool, publisher, config);
    let token = shutdown.clone();
    tokio::spawn(async move { relay.run(token).await });
    shutdown
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn relay_publishes_pending_events() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Insert 3 events with different aggregates
    let ids: Vec<Uuid> = (0..3).map(|_| Uuid::now_v7()).collect();
    for &id in &ids {
        let envelope = order_envelope(id);
        let insert = OutboxInsert::from_envelope(&topic, &envelope);
        insert_outbox_event(&db.pool, &insert).await.unwrap();
    }

    // Start relay
    let publisher = Arc::new(KafkaEventPublisher::new(&config).unwrap());
    let shutdown = start_relay(db.pool.clone(), publisher, fast_relay_config());

    // Consume all 3 from Kafka
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let mut received_ids: Vec<Uuid> = Vec::new();
    for _ in 0..3 {
        let msg = consumer.recv().await;
        received_ids.push(msg.envelope().metadata.aggregate_id);
    }

    shutdown.cancel();

    received_ids.sort();
    let mut expected_ids = ids;
    expected_ids.sort();
    assert_eq!(received_ids, expected_ids);

    // Metrics: 0 pending after relay processed them
    let metrics = collect_outbox_metrics(&db.pool).await.unwrap();
    assert_eq!(metrics.pending_count, 0);
}

#[tokio::test]
async fn relay_delete_on_publish_mode() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let outbox_row = insert_outbox_event(&db.pool, &OutboxInsert::from_envelope(&topic, &envelope))
        .await
        .unwrap();

    let publisher = Arc::new(KafkaEventPublisher::new(&config).unwrap());
    let mut relay_config = fast_relay_config();
    relay_config.delete_on_publish = true;
    let shutdown = start_relay(db.pool.clone(), publisher, relay_config);

    // Wait for relay to publish
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    assert_eq!(msg.envelope().metadata.aggregate_id, agg_id);

    shutdown.cancel();

    // Row should be deleted from DB
    let row_exists: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM outbox_events WHERE id = $1)")
            .bind(outbox_row.id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        !row_exists.0,
        "outbox row should be deleted in delete_on_publish mode"
    );
}

#[tokio::test]
async fn relay_retries_on_publish_failure() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    insert_outbox_event(&db.pool, &OutboxInsert::from_envelope(&topic, &envelope))
        .await
        .unwrap();

    // Publisher that fails once, then succeeds
    let real_publisher = Arc::new(KafkaEventPublisher::new(&config).unwrap());
    let failing_publisher = Arc::new(FailingPublisher::new(real_publisher, 1));
    let shutdown = start_relay(db.pool.clone(), failing_publisher, fast_relay_config());

    // Wait a moment for the first attempt to fail and next_retry_at to be set
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Force next_retry_at to now so relay can pick it up immediately
    sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() WHERE status = 'pending'")
        .execute(&db.pool)
        .await
        .unwrap();

    // The event should eventually be published
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    assert_eq!(msg.envelope().metadata.aggregate_id, agg_id);

    shutdown.cancel();
}

#[tokio::test]
async fn relay_escalates_permanent_failure() {
    let db = TestDb::start(MIGRATIONS).await;

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;

    // Insert with max_retries=1 so it fails permanently on first attempt
    insert_outbox_event(
        &db.pool,
        &OutboxInsert::from_envelope("dummy-topic", &envelope),
    )
    .await
    .unwrap();
    sqlx::query("UPDATE outbox_events SET max_retries = 1")
        .execute(&db.pool)
        .await
        .unwrap();

    let (escalation, failed_ids) = TrackingEscalation::new();
    let mut relay_config = fast_relay_config();
    relay_config.failure_escalation = Some(Arc::new(escalation));

    let publisher: Arc<dyn EventPublisher> = Arc::new(AlwaysFailPublisher);
    let shutdown = start_relay(db.pool.clone(), publisher, relay_config);

    // Wait for the relay to process and escalate
    tokio::time::sleep(Duration::from_millis(500)).await;
    shutdown.cancel();

    let ids = failed_ids.lock().unwrap();
    assert!(
        ids.contains(&event_id),
        "escalation should have recorded the event_id"
    );
}

#[tokio::test]
async fn relay_preserves_per_aggregate_ordering() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Two events for the SAME aggregate
    let agg_id = Uuid::now_v7();

    let created = EventMetadata::new(
        EventType::OrderCreated,
        AggregateType::Order,
        agg_id,
        SourceService::Order,
    );
    let confirmed = EventMetadata::new(
        EventType::OrderConfirmed,
        AggregateType::Order,
        agg_id,
        SourceService::Order,
    );

    insert_outbox_event(
        &db.pool,
        &OutboxInsert::from_envelope(&topic, &EventEnvelope::new(created, json!({"step": 1}))),
    )
    .await
    .unwrap();
    insert_outbox_event(
        &db.pool,
        &OutboxInsert::from_envelope(&topic, &EventEnvelope::new(confirmed, json!({"step": 2}))),
    )
    .await
    .unwrap();

    let publisher = Arc::new(KafkaEventPublisher::new(&config).unwrap());
    let shutdown = start_relay(db.pool.clone(), publisher, fast_relay_config());

    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg1 = consumer.recv().await;
    let msg2 = consumer.recv().await;

    shutdown.cancel();

    assert_eq!(msg1.envelope().metadata.event_type, EventType::OrderCreated);
    assert_eq!(msg1.envelope().payload["step"], 1);
    assert_eq!(
        msg2.envelope().metadata.event_type,
        EventType::OrderConfirmed
    );
    assert_eq!(msg2.envelope().payload["step"], 2);
}

#[tokio::test]
async fn relay_graceful_shutdown() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();

    let publisher = Arc::new(KafkaEventPublisher::new(&config).unwrap());
    let relay = OutboxRelay::new(db.pool.clone(), publisher, fast_relay_config());

    let shutdown = CancellationToken::new();
    let token = shutdown.clone();
    let handle = tokio::spawn(async move { relay.run(token).await });

    // Let it run briefly
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Cancel and verify it stops cleanly (no panic)
    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), handle)
        .await
        .expect("relay should stop within 5s")
        .expect("relay task should not panic");
}

#[tokio::test]
async fn relay_releases_stale_locks() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let outbox_row = insert_outbox_event(&db.pool, &OutboxInsert::from_envelope(&topic, &envelope))
        .await
        .unwrap();

    // Simulate a stale lock from a crashed relay
    sqlx::query(
        "UPDATE outbox_events SET locked_by = 'dead-relay', locked_at = NOW() - interval '10 minutes' WHERE id = $1",
    )
    .bind(outbox_row.id)
    .execute(&db.pool)
    .await
    .unwrap();

    // Start relay with short stale lock timeout
    let publisher = Arc::new(KafkaEventPublisher::new(&config).unwrap());
    let mut relay_config = fast_relay_config();
    relay_config.stale_lock_check_interval = Duration::from_millis(250);
    relay_config.stale_lock_timeout = Duration::from_millis(500);
    let shutdown = start_relay(db.pool.clone(), publisher, relay_config);

    // The stale lock loop should free the event, then the relay loop publishes it
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    assert_eq!(msg.envelope().metadata.aggregate_id, agg_id);

    shutdown.cancel();
}

#[tokio::test]
async fn relay_wakes_on_pg_notification() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Start relay FIRST (with a very long poll interval so it can only be woken by notification)
    let publisher = Arc::new(KafkaEventPublisher::new(&config).unwrap());
    let mut relay_config = fast_relay_config();
    relay_config.poll_interval = Duration::from_secs(60); // won't fire during test
    let shutdown = start_relay(db.pool.clone(), publisher, relay_config);

    // Give relay time to establish PgListener
    tokio::time::sleep(Duration::from_millis(200)).await;

    // NOW insert an event — the PG trigger fires NOTIFY, waking the relay
    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    insert_outbox_event(&db.pool, &OutboxInsert::from_envelope(&topic, &envelope))
        .await
        .unwrap();

    // Should arrive quickly (well under the 60s poll interval)
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = tokio::time::timeout(Duration::from_secs(10), consumer.recv())
        .await
        .expect("event should arrive within 10s via PG notification");
    assert_eq!(msg.envelope().metadata.aggregate_id, agg_id);

    shutdown.cancel();
}
