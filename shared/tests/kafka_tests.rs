use std::sync::Arc;
use std::time::Duration;

use rdkafka::Message;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Headers};
use serde_json::json;
use tokio_stream::StreamExt;
use uuid::Uuid;

use shared::events::{
    AggregateType, EventEnvelope, EventMetadata, EventPublisher, EventType, KafkaAdmin,
    KafkaEventPublisher, SourceService, TopicSpec,
};
use shared::test_utils::kafka::TestKafka;

fn unique_topic() -> String {
    format!("test-{}", Uuid::now_v7())
}

fn test_consumer(bootstrap_servers: &str, topic: &str) -> StreamConsumer {
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("group.id", &format!("test-group-{}", Uuid::now_v7()))
        .set("auto.offset.reset", "earliest")
        .create()
        .unwrap();
    consumer.subscribe(&[topic]).unwrap();
    consumer
}

/// Consume the first message, retrying on transient broker transport errors.
async fn consume_first_message(consumer: &StreamConsumer) -> BorrowedMessage<'_> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let remaining = deadline - tokio::time::Instant::now();
        let result = tokio::time::timeout(remaining, consumer.stream().next()).await;
        match result {
            Ok(Some(Ok(msg))) => return msg,
            Ok(Some(Err(_))) => {
                // Transient error (e.g. BrokerTransportFailure) — retry
                tokio::time::sleep(Duration::from_millis(200)).await;
                continue;
            }
            Ok(None) => panic!("consumer stream ended unexpectedly"),
            Err(_) => panic!("timed out waiting for message (30s)"),
        }
    }
}

fn test_envelope() -> EventEnvelope {
    let metadata = EventMetadata::new(
        EventType::OrderCreated,
        AggregateType::Order,
        Uuid::now_v7(),
        SourceService::Order,
    )
    .with_correlation_id("trace-abc-123")
    .with_causation_id(Uuid::now_v7());
    EventEnvelope::new(metadata, json!({"item": "widget", "qty": 3}))
}

#[tokio::test]
async fn admin_creates_topic() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let topic = unique_topic();
    let spec = TopicSpec::new(&topic, 1, 1);
    admin.ensure_topics(&[spec]).await.unwrap();
}

#[tokio::test]
async fn admin_ensure_topics_is_idempotent() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let topic = unique_topic();
    let spec1 = TopicSpec::new(&topic, 1, 1);
    admin.ensure_topics(&[spec1]).await.unwrap();

    // Second call with same topic should not error
    let spec2 = TopicSpec::new(&topic, 1, 1);
    admin.ensure_topics(&[spec2]).await.unwrap();
}

#[tokio::test]
async fn admin_ensure_topics_empty_is_noop() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    admin.ensure_topics(&[]).await.unwrap();
}

#[tokio::test]
async fn admin_topic_with_config() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let topic = unique_topic();
    let spec = TopicSpec::new(&topic, 1, 1).with_config("retention.ms", "86400000");
    admin.ensure_topics(&[spec]).await.unwrap();
}

#[tokio::test]
async fn publish_and_consume_event() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Publish
    let publisher = KafkaEventPublisher::new(&config).unwrap();
    let envelope = test_envelope();
    let expected_key = envelope.partition_key();
    publisher.publish(&topic, &envelope).await.unwrap();

    // Consume
    let consumer = test_consumer(&kafka.bootstrap_servers, &topic);
    let msg = consume_first_message(&consumer).await;

    // Verify key
    let key = std::str::from_utf8(msg.key().unwrap()).unwrap();
    assert_eq!(key, expected_key);

    // Verify payload round-trips
    let payload = std::str::from_utf8(msg.payload().unwrap()).unwrap();
    let decoded: EventEnvelope = serde_json::from_str(payload).unwrap();
    assert_eq!(decoded.metadata.event_type, EventType::OrderCreated);
    assert_eq!(decoded.metadata.aggregate_type, AggregateType::Order);
    assert_eq!(decoded.metadata.source_service, SourceService::Order);
}

#[tokio::test]
async fn headers_contain_all_metadata_fields() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let publisher = KafkaEventPublisher::new(&config).unwrap();
    let envelope = test_envelope();
    let m = &envelope.metadata;
    let expected_event_id = m.event_id.to_string();
    let expected_correlation_id = m.correlation_id.clone().unwrap();
    let expected_causation_id = m.causation_id.unwrap().to_string();

    publisher.publish(&topic, &envelope).await.unwrap();

    // Consume and check headers
    let consumer = test_consumer(&kafka.bootstrap_servers, &topic);
    let msg = consume_first_message(&consumer).await;

    let headers = msg.headers().expect("no headers on message");

    let get_header = |key: &str| -> String {
        for i in 0..headers.count() {
            let h = headers.try_get(i).unwrap();
            if h.key == key {
                return std::str::from_utf8(h.value.unwrap()).unwrap().to_string();
            }
        }
        panic!("header '{key}' not found");
    };

    assert_eq!(get_header("event_id"), expected_event_id);
    assert_eq!(get_header("event_type"), "OrderCreated");
    assert_eq!(get_header("aggregate_type"), "Order");
    assert_eq!(get_header("source_service"), "order");
    assert_eq!(get_header("correlation_id"), expected_correlation_id);
    assert_eq!(get_header("causation_id"), expected_causation_id);
    // aggregate_id header is present
    let _ = get_header("aggregate_id");
}

#[tokio::test]
async fn publisher_is_send_sync() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let publisher = KafkaEventPublisher::new(&config).unwrap();

    // Prove KafkaEventPublisher satisfies Send + Sync as a trait object
    let publisher: Arc<dyn EventPublisher> = Arc::new(publisher);

    let topic = unique_topic();
    let envelope = test_envelope();

    let handle = tokio::spawn(async move {
        // This won't actually succeed (topic doesn't exist) but proves the bounds
        let _ = publisher.publish(&topic, &envelope).await;
    });

    handle.await.unwrap();
}
