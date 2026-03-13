//! Integration tests for KafkaEventConsumer with DLQ support.
//!
//! Uses TestKafka + TestDb + unique topics for isolation.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use shared::events::{
    AggregateType, ConsumerConfig, EventEnvelope, EventHandler, EventMetadata, EventPublisher,
    EventType, HandlerError, KafkaAdmin, KafkaEventConsumer, KafkaEventPublisher, MockEventHandler,
    SourceService, TopicSpec,
};
use shared::outbox::{is_event_processed, mark_event_processed};
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

/// Publish an event directly to a Kafka topic (bypasses outbox, for consumer testing).
async fn publish_to_topic(kafka: &TestKafka, topic: &str, envelope: &EventEnvelope) {
    let publisher = KafkaEventPublisher::new(&kafka.kafka_config()).unwrap();
    publisher.publish(topic, envelope).await.unwrap();
}

/// Start a consumer in the background and return (shutdown_token, join_handle).
fn spawn_consumer(
    kafka: &TestKafka,
    db: &TestDb,
    topic: &str,
    handler: Arc<dyn EventHandler>,
) -> (CancellationToken, tokio::task::JoinHandle<()>) {
    let config = ConsumerConfig {
        // Fast retries for tests
        retry_base_delay: Duration::from_millis(50),
        retry_max_delay: Duration::from_millis(200),
        // Disable auto-create DLQ in tests — we create topics explicitly
        auto_create_dlq_topics: false,
        // Short cleanup interval (but we won't test it here)
        processed_events_cleanup_interval: Duration::from_secs(3600),
        processed_events_max_age: Duration::from_secs(7 * 24 * 3600),
        ..ConsumerConfig::new(
            format!("test-consumer-{}", Uuid::now_v7()),
            vec![topic.to_string()],
        )
    };

    let consumer =
        KafkaEventConsumer::new(&kafka.kafka_config(), config, handler, db.pool.clone()).unwrap();

    let shutdown = CancellationToken::new();
    let s = shutdown.clone();
    let handle = tokio::spawn(async move { consumer.run(s).await });

    (shutdown, handle)
}

/// Wait for a condition with timeout.
async fn wait_for<F, Fut>(timeout: Duration, interval: Duration, f: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if f().await {
            return;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("wait_for timed out after {timeout:?}");
        }
        tokio::time::sleep(interval).await;
    }
}

// ── Test 1: Happy path ─────────────────────────────────────────────

#[tokio::test]
async fn consumer_processes_event_and_marks_processed() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let handler = Arc::new(MockEventHandler::new());
    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler.clone());

    // Publish an event
    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;
    publish_to_topic(&kafka, &topic, &envelope).await;

    // Wait for handler to receive the event
    let h = handler.clone();
    wait_for(Duration::from_secs(30), Duration::from_millis(100), || {
        let h = h.clone();
        async move { h.received_count() >= 1 }
    })
    .await;

    // Verify handler was called
    assert_eq!(handler.received_count(), 1);
    assert_eq!(handler.received()[0].metadata.event_id, event_id);

    // Verify processed_events row exists
    assert!(is_event_processed(&db.pool, event_id).await.unwrap());

    shutdown.cancel();
    handle.await.unwrap();
}

// ── Test 2: Idempotency — skips duplicate events ────────────────────

#[tokio::test]
async fn consumer_skips_duplicate_events() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Pre-insert the event as processed
    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;
    mark_event_processed(&db.pool, event_id, "OrderCreated", "test")
        .await
        .unwrap();

    let handler = Arc::new(MockEventHandler::new());
    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler.clone());

    // Publish the same event
    publish_to_topic(&kafka, &topic, &envelope).await;

    // Give consumer time to process (it should skip)
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Handler should NOT have been called — the event was already processed
    assert_eq!(handler.received_count(), 0);

    shutdown.cancel();
    handle.await.unwrap();
}

// ── Test 3: Retries transient errors ────────────────────────────────

#[tokio::test]
async fn consumer_retries_transient_errors() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Handler fails twice (transient), then succeeds
    let handler = Arc::new(MockEventHandler::new());
    handler.push_result(Err(HandlerError::transient("fail 1")));
    handler.push_result(Err(HandlerError::transient("fail 2")));
    // Third call: no queued result → Ok(())

    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler.clone());

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;
    publish_to_topic(&kafka, &topic, &envelope).await;

    // Wait for processing (includes retry backoff)
    let h = handler.clone();
    wait_for(Duration::from_secs(30), Duration::from_millis(100), || {
        let h = h.clone();
        async move { h.received_count() >= 3 }
    })
    .await;

    // Handler was called 3 times total (2 failures + 1 success)
    assert_eq!(handler.received_count(), 3);

    // Event is marked as processed after eventual success
    assert!(is_event_processed(&db.pool, event_id).await.unwrap());

    shutdown.cancel();
    handle.await.unwrap();
}

// ── Test 4: DLQ after exhausting retries ────────────────────────────

#[tokio::test]
async fn consumer_sends_to_dlq_after_exhausting_retries() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();
    let dlq_topic = format!("{topic}.dlq");

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[
            TopicSpec::new(&topic, 1, 1),
            TopicSpec::new(&dlq_topic, 1, 1),
        ])
        .await
        .unwrap();

    // Handler always fails with transient errors (4 times = initial + 3 retries)
    let handler = Arc::new(MockEventHandler::new());
    for _ in 0..4 {
        handler.push_result(Err(HandlerError::transient("always fail")));
    }

    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler.clone());

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;
    publish_to_topic(&kafka, &topic, &envelope).await;

    // Wait for DLQ message
    let dlq_consumer = TestConsumer::new(&kafka.bootstrap_servers, &dlq_topic);
    let dlq_msg = dlq_consumer.recv().await;

    // Verify DLQ headers
    assert_eq!(dlq_msg.headers.get("dlq_original_topic").unwrap(), &topic);
    assert!(dlq_msg.headers.contains_key("dlq_reason"));
    assert!(dlq_msg.headers.contains_key("dlq_retry_count"));
    assert!(dlq_msg.headers.contains_key("dlq_timestamp"));
    assert!(dlq_msg.headers.contains_key("dlq_consumer_group"));

    // Verify the DLQ payload is the original envelope
    let dlq_envelope = dlq_msg.envelope();
    assert_eq!(dlq_envelope.metadata.event_id, event_id);
    assert_eq!(dlq_envelope.metadata.event_type, EventType::OrderCreated);

    // Event should NOT be marked as processed (handler never succeeded)
    assert!(!is_event_processed(&db.pool, event_id).await.unwrap());

    // Handler was called 4 times (1 initial + 3 retries)
    assert_eq!(handler.received_count(), 4);

    shutdown.cancel();
    handle.await.unwrap();
}

// ── Test 5: Permanent error → DLQ immediately ──────────────────────

#[tokio::test]
async fn consumer_sends_permanent_error_to_dlq_immediately() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();
    let dlq_topic = format!("{topic}.dlq");

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[
            TopicSpec::new(&topic, 1, 1),
            TopicSpec::new(&dlq_topic, 1, 1),
        ])
        .await
        .unwrap();

    let handler = Arc::new(MockEventHandler::new());
    handler.push_result(Err(HandlerError::permanent("bad payload")));

    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler.clone());

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;
    publish_to_topic(&kafka, &topic, &envelope).await;

    // DLQ message should appear immediately (no retries)
    let dlq_consumer = TestConsumer::new(&kafka.bootstrap_servers, &dlq_topic);
    let dlq_msg = dlq_consumer.recv().await;

    assert_eq!(dlq_msg.headers["dlq_original_topic"], topic);
    assert!(dlq_msg.headers["dlq_reason"].contains("bad payload"));
    assert_eq!(dlq_msg.headers["dlq_retry_count"], "0");

    // Only one handler call (no retries for permanent errors)
    assert_eq!(handler.received_count(), 1);

    // NOT marked as processed
    assert!(!is_event_processed(&db.pool, event_id).await.unwrap());

    shutdown.cancel();
    handle.await.unwrap();
}

// ── Test 6: Deserialization failure → DLQ with raw bytes ────────────

#[tokio::test]
async fn consumer_handles_deserialization_failure() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();
    let dlq_topic = format!("{topic}.dlq");

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[
            TopicSpec::new(&topic, 1, 1),
            TopicSpec::new(&dlq_topic, 1, 1),
        ])
        .await
        .unwrap();

    let handler = Arc::new(MockEventHandler::new());
    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler.clone());

    // Publish garbage to the topic
    let publisher = KafkaEventPublisher::new(&kafka.kafka_config()).unwrap();
    use rdkafka::config::ClientConfig;
    use rdkafka::producer::{FutureProducer, FutureRecord};
    let raw_producer: FutureProducer = ClientConfig::new()
        .set("bootstrap.servers", &kafka.bootstrap_servers)
        .set("message.timeout.ms", "5000")
        .create()
        .unwrap();
    let garbage = b"not valid json at all";
    let record = FutureRecord::<(), [u8]>::to(&topic).payload(garbage);
    raw_producer
        .send(record, Duration::from_secs(5))
        .await
        .unwrap();

    // DLQ should receive the raw bytes
    let dlq_consumer = TestConsumer::new(&kafka.bootstrap_servers, &dlq_topic);
    let dlq_msg = dlq_consumer.recv().await;

    assert_eq!(dlq_msg.headers["dlq_original_topic"], topic);
    assert!(dlq_msg.headers["dlq_reason"].contains("expected"));
    assert_eq!(dlq_msg.payload, "not valid json at all");

    // Handler was never called
    assert_eq!(handler.received_count(), 0);

    // Suppress unused variable warning
    let _ = publisher;

    shutdown.cancel();
    handle.await.unwrap();
}

// ── Test 7: Graceful shutdown ──────────────────────────────────────

#[tokio::test]
async fn consumer_graceful_shutdown() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let handler = Arc::new(MockEventHandler::new());
    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler.clone());

    // Let consumer start and then shut it down
    tokio::time::sleep(Duration::from_secs(2)).await;
    shutdown.cancel();

    // Consumer should exit cleanly within a reasonable time
    let result = tokio::time::timeout(Duration::from_secs(10), handle).await;
    assert!(result.is_ok(), "consumer should shut down within 10s");
    result.unwrap().unwrap();
}

// ── Test 8: Handler writes in same transaction ─────────────────────

/// A handler that writes a row to the DB inside the transaction.
struct WritingHandler;

#[async_trait::async_trait]
impl EventHandler for WritingHandler {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError> {
        // Write a row to processed_events with a special source_service marker
        // to prove we're in the same transaction as the consumer's mark_event_processed
        sqlx::query(
            "INSERT INTO processed_events (event_id, event_type, source_service)
             VALUES ($1, $2, $3)
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(Uuid::now_v7()) // different ID to not conflict with the consumer's mark
        .bind("HandlerTest")
        .bind(envelope.metadata.aggregate_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| HandlerError::transient(e.to_string()))?;

        Ok(())
    }
}

#[tokio::test]
async fn consumer_handler_writes_in_same_transaction() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let topic = unique_topic();

    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let handler: Arc<dyn EventHandler> = Arc::new(WritingHandler);
    let (shutdown, handle) = spawn_consumer(&kafka, &db, &topic, handler);

    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;
    publish_to_topic(&kafka, &topic, &envelope).await;

    // Wait for the event to be processed
    let pool = db.pool.clone();
    wait_for(
        Duration::from_secs(30),
        Duration::from_millis(100),
        move || {
            let pool = pool.clone();
            async move { is_event_processed(&pool, event_id).await.unwrap_or(false) }
        },
    )
    .await;

    // Verify both the consumer's mark AND the handler's write exist
    assert!(is_event_processed(&db.pool, event_id).await.unwrap());

    // Handler's row should also exist (written in the same committed tx)
    let handler_row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM processed_events WHERE source_service = $1)")
            .bind(agg_id.to_string())
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        handler_row.0,
        "handler's write should be committed atomically"
    );

    shutdown.cancel();
    handle.await.unwrap();
}
