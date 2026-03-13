//! Integration tests for consumer metrics collection.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use shared::events::{
    AggregateType, ConsumerConfig, ConsumerMetricsCollector, EventEnvelope, EventHandler,
    EventMetadata, EventPublisher, EventType, KafkaAdmin, KafkaEventConsumer, KafkaEventPublisher,
    MockEventHandler, SourceService, TopicSpec,
};
use shared::test_utils::db::TestDb;
use shared::test_utils::kafka::TestKafka;

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

async fn publish_to_topic(kafka: &TestKafka, topic: &str, envelope: &EventEnvelope) {
    let publisher = KafkaEventPublisher::new(&kafka.kafka_config()).unwrap();
    publisher.publish(topic, envelope).await.unwrap();
}

/// Spawn consumer with metrics handle returned.
fn spawn_consumer_with_metrics(
    kafka: &TestKafka,
    db: &TestDb,
    topic: &str,
    handler: Arc<dyn EventHandler>,
) -> (
    CancellationToken,
    tokio::task::JoinHandle<()>,
    Arc<ConsumerMetricsCollector>,
) {
    let config = ConsumerConfig {
        retry_base_delay: Duration::from_millis(50),
        retry_max_delay: Duration::from_millis(200),
        auto_create_dlq_topics: false,
        processed_events_cleanup_interval: Duration::from_secs(3600),
        processed_events_max_age: Duration::from_secs(7 * 24 * 3600),
        ..ConsumerConfig::new(
            format!("test-consumer-{}", Uuid::now_v7()),
            vec![topic.to_string()],
        )
    };

    let consumer =
        KafkaEventConsumer::new(&kafka.kafka_config(), config, handler, db.pool.clone()).unwrap();

    let metrics = consumer.metrics();
    let shutdown = CancellationToken::new();
    let s = shutdown.clone();
    let handle = tokio::spawn(async move { consumer.run(s).await });

    (shutdown, handle, metrics)
}

async fn wait_for<F, Fut>(timeout: Duration, interval: Duration, f: F)
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if f().await {
            return;
        }
        tokio::time::sleep(interval).await;
    }
    panic!("wait_for timed out after {timeout:?}");
}

#[tokio::test]
async fn metrics_record_successful_processing() {
    let kafka = TestKafka::start().await;
    let db = TestDb::start(MIGRATIONS).await;
    let topic = unique_topic();

    let handler = Arc::new(MockEventHandler::new());
    let (shutdown, handle, metrics) =
        spawn_consumer_with_metrics(&kafka, &db, &topic, handler.clone());

    // Publish 2 events
    let env1 = order_envelope(Uuid::now_v7());
    let env2 = order_envelope(Uuid::now_v7());
    publish_to_topic(&kafka, &topic, &env1).await;
    publish_to_topic(&kafka, &topic, &env2).await;

    // Wait for both to be processed
    wait_for(Duration::from_secs(15), Duration::from_millis(100), || {
        let m = metrics.snapshot();
        async move { m.events_processed >= 2 }
    })
    .await;

    let snap = metrics.snapshot();
    assert_eq!(snap.events_processed, 2);
    assert_eq!(snap.events_retried, 0);
    assert_eq!(snap.events_sent_to_dlq, 0);
    assert_eq!(snap.total_events, 2);
    assert!(snap.avg_processing_duration_ms > 0.0);

    shutdown.cancel();
    let _ = handle.await;
}

#[tokio::test]
async fn metrics_record_retries_and_dlq() {
    let kafka = TestKafka::start().await;
    let db = TestDb::start(MIGRATIONS).await;
    let topic = unique_topic();
    let dlq_topic = format!("{topic}.dlq");

    // Pre-create DLQ topic
    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin
        .ensure_topics(&[TopicSpec::new(&dlq_topic, 1, 1)])
        .await
        .unwrap();

    // Handler that returns transient errors for all attempts (0..=max_retries=3 → 4 calls)
    let handler = Arc::new(MockEventHandler::new());
    for _ in 0..4 {
        handler.push_result(Err(shared::events::HandlerError::transient("test failure")));
    }
    let (shutdown, handle, metrics) =
        spawn_consumer_with_metrics(&kafka, &db, &topic, handler.clone());

    let env = order_envelope(Uuid::now_v7());
    publish_to_topic(&kafka, &topic, &env).await;

    // Wait for event to land in DLQ (retries exhausted, default max_retries=3)
    wait_for(Duration::from_secs(15), Duration::from_millis(100), || {
        let m = metrics.snapshot();
        async move { m.events_sent_to_dlq >= 1 }
    })
    .await;

    let snap = metrics.snapshot();
    assert_eq!(snap.events_processed, 0);
    assert_eq!(snap.events_sent_to_dlq, 1);
    // 3 retries (attempts 0,1,2 fail, then attempt 3 = max_retries → DLQ)
    assert_eq!(snap.events_retried, 3);
    assert_eq!(snap.total_events, 1);

    shutdown.cancel();
    let _ = handle.await;
}
