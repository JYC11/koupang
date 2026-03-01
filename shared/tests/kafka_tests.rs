use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use shared::events::{
    AggregateType, EventEnvelope, EventMetadata, EventPublisher, EventType, KafkaAdmin,
    KafkaEventPublisher, SourceService, TopicSpec,
};
use shared::test_utils::kafka::{TestConsumer, TestKafka};

fn unique_topic() -> String {
    format!("test-{}", Uuid::now_v7())
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

// ── Admin tests ──────────────────────────────────────────────────────

#[tokio::test]
async fn admin_creates_topic() {
    let kafka = TestKafka::start().await;
    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();

    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();
}

#[tokio::test]
async fn admin_ensure_topics_is_idempotent() {
    let kafka = TestKafka::start().await;
    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();

    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();
}

#[tokio::test]
async fn admin_ensure_topics_empty_is_noop() {
    let kafka = TestKafka::start().await;
    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();
    admin.ensure_topics(&[]).await.unwrap();
}

#[tokio::test]
async fn admin_topic_with_config() {
    let kafka = TestKafka::start().await;
    let admin = KafkaAdmin::new(&kafka.kafka_config()).unwrap();

    let topic = unique_topic();
    let spec = TopicSpec::new(&topic, 1, 1).with_config("retention.ms", "86400000");
    admin.ensure_topics(&[spec]).await.unwrap();
}

// ── Publisher tests ──────────────────────────────────────────────────

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
    let expected_event_id = envelope.metadata.event_id.to_string();
    let expected_agg_id = envelope.metadata.aggregate_id.to_string();
    let expected_correlation_id = envelope.metadata.correlation_id.clone().unwrap();
    let expected_causation_id = envelope.metadata.causation_id.unwrap().to_string();

    publisher.publish(&topic, &envelope).await.unwrap();

    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;

    // Key is aggregate_id
    assert_eq!(msg.key, expected_agg_id);

    // Payload round-trips as EventEnvelope
    let received = msg.envelope();
    assert_eq!(received.metadata.event_type, EventType::OrderCreated);
    assert_eq!(received.metadata.aggregate_type, AggregateType::Order);
    assert_eq!(received.metadata.source_service, SourceService::Order);

    // All 7 headers present
    assert_eq!(msg.headers["event_id"], expected_event_id);
    assert_eq!(msg.headers["event_type"], "OrderCreated");
    assert_eq!(msg.headers["aggregate_type"], "Order");
    assert_eq!(msg.headers["aggregate_id"], expected_agg_id);
    assert_eq!(msg.headers["source_service"], "order");
    assert_eq!(msg.headers["correlation_id"], expected_correlation_id);
    assert_eq!(msg.headers["causation_id"], expected_causation_id);
}

#[tokio::test]
async fn headers_omit_optional_fields_when_none() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Envelope with NO correlation_id or causation_id
    let metadata = EventMetadata::new(
        EventType::OrderCreated,
        AggregateType::Order,
        Uuid::now_v7(),
        SourceService::Order,
    );
    let envelope = EventEnvelope::new(metadata, json!({"minimal": true}));

    let publisher = KafkaEventPublisher::new(&config).unwrap();
    publisher.publish(&topic, &envelope).await.unwrap();

    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;

    // Required headers present
    assert!(msg.headers.contains_key("event_id"));
    assert!(msg.headers.contains_key("event_type"));
    assert!(msg.headers.contains_key("aggregate_type"));
    assert!(msg.headers.contains_key("aggregate_id"));
    assert!(msg.headers.contains_key("source_service"));

    // Optional headers absent (not empty strings)
    assert!(
        !msg.headers.contains_key("correlation_id"),
        "correlation_id header should be absent when None"
    );
    assert!(
        !msg.headers.contains_key("causation_id"),
        "causation_id header should be absent when None"
    );
}

#[tokio::test]
async fn concurrent_publishers_same_topic() {
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    let publisher: Arc<dyn EventPublisher> = Arc::new(KafkaEventPublisher::new(&config).unwrap());

    // Spawn 10 concurrent publish tasks
    let mut handles = Vec::new();
    for i in 0..10 {
        let pub_clone = Arc::clone(&publisher);
        let topic_clone = topic.clone();
        handles.push(tokio::spawn(async move {
            let metadata = EventMetadata::new(
                EventType::OrderCreated,
                AggregateType::Order,
                Uuid::now_v7(),
                SourceService::Order,
            );
            let envelope = EventEnvelope::new(metadata, json!({"seq": i}));
            pub_clone
                .publish(&topic_clone, &envelope)
                .await
                .expect("concurrent publish should succeed");
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Consume all 10 messages
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let mut seen = std::collections::HashSet::new();
    for _ in 0..10 {
        let msg = consumer.recv().await;
        let env = msg.envelope();
        let seq = env.payload["seq"].as_i64().unwrap();
        assert!(seen.insert(seq), "duplicate seq {seq}");
    }
    assert_eq!(seen.len(), 10);
}

#[tokio::test]
async fn publisher_is_send_sync() {
    let kafka = TestKafka::start().await;
    let publisher = KafkaEventPublisher::new(&kafka.kafka_config()).unwrap();

    // Prove KafkaEventPublisher satisfies Send + Sync as a trait object
    let publisher: Arc<dyn EventPublisher> = Arc::new(publisher);

    let topic = unique_topic();
    let envelope = test_envelope();

    let handle = tokio::spawn(async move {
        let _ = publisher.publish(&topic, &envelope).await;
    });

    handle.await.unwrap();
}
