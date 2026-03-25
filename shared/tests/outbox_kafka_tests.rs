//! Integration tests for the full outbox → Kafka pipeline.
//!
//! These tests exercise the complete relay cycle:
//! insert_outbox_event → claim_batch → KafkaEventPublisher.publish → mark_published → consume

use serde_json::json;
use uuid::Uuid;

use shared::events::{
    AggregateType, EventEnvelope, EventMetadata, EventPublisher, EventType, KafkaAdmin,
    KafkaEventPublisher, SourceService, TopicSpec,
};
use shared::outbox::{
    OutboxInsert, claim_batch, collect_outbox_metrics, delete_published, insert_outbox_event,
    is_event_processed, mark_event_processed, mark_published, mark_retry_or_failed,
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
    )
    .with_correlation_id("trace-relay-test");
    EventEnvelope::new(
        metadata,
        json!({"order_id": aggregate_id.to_string(), "total": "249.99"}),
    )
}

fn payment_envelope(aggregate_id: Uuid) -> EventEnvelope {
    let metadata = EventMetadata::new(
        EventType::PaymentAuthorized,
        AggregateType::Payment,
        aggregate_id,
        SourceService::Payment,
    );
    EventEnvelope::new(
        metadata,
        json!({"payment_id": aggregate_id.to_string(), "amount": "99.50"}),
    )
}

// ── Full relay simulation ────────────────────────────────────────────

#[tokio::test]
async fn relay_full_cycle_insert_claim_publish_consume() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // 1. Insert outbox event (simulates service writing inside a transaction)
    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let insert = OutboxInsert::from_envelope(&topic, &envelope);
    let outbox_row = insert_outbox_event(&db.pool, &insert).await.unwrap();
    assert_eq!(outbox_row.status, shared::outbox::OutboxStatus::Pending);

    // 2. Relay claims the batch
    let claimed = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(claimed[0].id, outbox_row.id);

    // 3. Relay publishes to Kafka using the outbox payload
    let publisher = KafkaEventPublisher::new(&config).unwrap();
    let stored_envelope: EventEnvelope =
        serde_json::from_value(claimed[0].payload.clone()).unwrap();
    publisher.publish(&topic, &stored_envelope).await.unwrap();

    // 4. Relay marks published
    mark_published(&db.pool, outbox_row.id).await.unwrap();

    // 5. Verify the event arrived in Kafka with correct data
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;

    let received = msg.envelope();
    assert_eq!(received.metadata.event_type, EventType::OrderCreated);
    assert_eq!(received.metadata.aggregate_id, agg_id);
    assert_eq!(received.metadata.source_service, SourceService::Order);
    assert_eq!(msg.key, agg_id.to_string());

    // 6. Verify outbox row is now published (not pending)
    let reclaimed = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert!(
        reclaimed.is_empty(),
        "published events should not be reclaimable"
    );
}

// ── Batch publish ────────────────────────────────────────────────────

#[tokio::test]
async fn relay_batch_publishes_multiple_events() {
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

    // Claim and publish all
    let claimed = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(claimed.len(), 3);

    let publisher = KafkaEventPublisher::new(&config).unwrap();
    for event in &claimed {
        let env: EventEnvelope = serde_json::from_value(event.payload.clone()).unwrap();
        publisher.publish(&topic, &env).await.unwrap();
        mark_published(&db.pool, event.id).await.unwrap();
    }

    // Consume all 3 and collect aggregate_ids
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let mut received_ids: Vec<Uuid> = Vec::new();
    for _ in 0..3 {
        let msg = consumer.recv().await;
        let env = msg.envelope();
        received_ids.push(env.metadata.aggregate_id);
    }
    received_ids.sort();

    let mut expected_ids = ids.clone();
    expected_ids.sort();
    assert_eq!(received_ids, expected_ids);
}

// ── Payload fidelity through the full pipeline ──────────────────────

#[tokio::test]
async fn outbox_payload_fidelity_through_kafka() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Create envelope with rich payload
    let agg_id = Uuid::now_v7();
    let causation_id = Uuid::now_v7();
    let metadata = EventMetadata::new(
        EventType::OrderCreated,
        AggregateType::Order,
        agg_id,
        SourceService::Order,
    )
    .with_correlation_id("trace-fidelity-test")
    .with_causation_id(causation_id);

    let payload = json!({
        "order_id": agg_id.to_string(),
        "buyer_id": Uuid::now_v7().to_string(),
        "total": "1299.99",
        "currency": "KRW",
        "items": [
            {"sku": "SKU-001", "qty": 2, "price": "499.99"},
            {"sku": "SKU-002", "qty": 1, "price": "300.01"}
        ]
    });
    let original_envelope = EventEnvelope::new(metadata, payload);

    // Pipeline: insert → claim → publish → consume
    let insert = OutboxInsert::from_envelope(&topic, &original_envelope);
    insert_outbox_event(&db.pool, &insert).await.unwrap();

    let claimed = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    let stored_envelope: EventEnvelope =
        serde_json::from_value(claimed[0].payload.clone()).unwrap();

    let publisher = KafkaEventPublisher::new(&config).unwrap();
    publisher.publish(&topic, &stored_envelope).await.unwrap();
    mark_published(&db.pool, claimed[0].id).await.unwrap();

    // Verify every field survived the round-trip
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    let received = msg.envelope();

    assert_eq!(
        received.metadata.event_id,
        original_envelope.metadata.event_id
    );
    assert_eq!(received.metadata.event_type, EventType::OrderCreated);
    assert_eq!(received.metadata.aggregate_type, AggregateType::Order);
    assert_eq!(received.metadata.aggregate_id, agg_id);
    assert_eq!(received.metadata.source_service, SourceService::Order);
    assert_eq!(
        received.metadata.correlation_id.as_deref(),
        Some("trace-fidelity-test")
    );
    assert_eq!(received.metadata.causation_id, Some(causation_id));

    // Payload data survived
    assert_eq!(received.payload["total"], "1299.99");
    assert_eq!(received.payload["items"].as_array().unwrap().len(), 2);

    // Kafka headers match
    assert_eq!(msg.headers["event_type"], "OrderCreated");
    assert_eq!(msg.headers["aggregate_type"], "Order");
    assert_eq!(msg.headers["source_service"], "order");
    assert_eq!(msg.headers["aggregate_id"], agg_id.to_string());
    assert_eq!(msg.headers["correlation_id"], "trace-fidelity-test");
    assert_eq!(msg.headers["causation_id"], causation_id.to_string());
}

// ── Consumer-side idempotency ────────────────────────────────────────

#[tokio::test]
async fn consumer_idempotency_via_processed_events() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Publish an event through the outbox
    let agg_id = Uuid::now_v7();
    let envelope = order_envelope(agg_id);
    let event_id = envelope.metadata.event_id;
    let insert = OutboxInsert::from_envelope(&topic, &envelope);
    insert_outbox_event(&db.pool, &insert).await.unwrap();

    let claimed = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    let publisher = KafkaEventPublisher::new(&config).unwrap();
    let stored: EventEnvelope = serde_json::from_value(claimed[0].payload.clone()).unwrap();
    publisher.publish(&topic, &stored).await.unwrap();
    mark_published(&db.pool, claimed[0].id).await.unwrap();

    // Consumer receives the event
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    let received = msg.envelope();

    // First time: not yet processed
    assert!(
        !is_event_processed(&db.pool, event_id, "test-consumer")
            .await
            .unwrap()
    );

    // Process and mark
    mark_event_processed(
        &db.pool,
        received.metadata.event_id,
        "OrderCreated",
        "order",
        "test-consumer",
    )
    .await
    .unwrap();

    // Second time: already processed — consumer would skip
    assert!(
        is_event_processed(&db.pool, event_id, "test-consumer")
            .await
            .unwrap()
    );

    // Marking again is idempotent (no error)
    mark_event_processed(&db.pool, event_id, "OrderCreated", "order", "test-consumer")
        .await
        .unwrap();
}

// ── Delete-on-publish mode ───────────────────────────────────────────

#[tokio::test]
async fn relay_delete_on_publish_removes_outbox_row() {
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
    let insert = OutboxInsert::from_envelope(&topic, &envelope);
    let outbox_row = insert_outbox_event(&db.pool, &insert).await.unwrap();

    let claimed = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    let publisher = KafkaEventPublisher::new(&config).unwrap();
    let stored: EventEnvelope = serde_json::from_value(claimed[0].payload.clone()).unwrap();
    publisher.publish(&topic, &stored).await.unwrap();

    // Delete-on-publish mode: delete instead of mark_published
    delete_published(&db.pool, outbox_row.id).await.unwrap();

    // Row is gone from DB
    let row_exists: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM outbox_events WHERE id = $1)")
            .bind(outbox_row.id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        !row_exists.0,
        "outbox row should be deleted after delete_on_publish"
    );

    // But the message is still on Kafka
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    assert_eq!(msg.envelope().metadata.aggregate_id, agg_id);
}

// ── Retry then publish ───────────────────────────────────────────────

#[tokio::test]
async fn relay_retry_then_successful_publish() {
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
    let insert = OutboxInsert::from_envelope(&topic, &envelope);
    let outbox_row = insert_outbox_event(&db.pool, &insert).await.unwrap();

    // First attempt: claim succeeds, but "publish fails" → mark_retry_or_failed
    let claimed = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    assert_eq!(claimed.len(), 1);
    mark_retry_or_failed(&db.pool, outbox_row.id, "Kafka temporarily unavailable")
        .await
        .unwrap();

    // Event goes back to pending with retry_count=1 and future next_retry_at.
    // Force next_retry_at to now so we can reclaim immediately in the test.
    sqlx::query("UPDATE outbox_events SET next_retry_at = NOW() WHERE id = $1")
        .bind(outbox_row.id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Second attempt: claim again, this time publish succeeds
    let reclaimed = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].retry_count, 1);

    let publisher = KafkaEventPublisher::new(&config).unwrap();
    let stored: EventEnvelope = serde_json::from_value(reclaimed[0].payload.clone()).unwrap();
    publisher.publish(&topic, &stored).await.unwrap();
    mark_published(&db.pool, outbox_row.id).await.unwrap();

    // Event arrives on Kafka
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    assert_eq!(msg.envelope().metadata.aggregate_id, agg_id);
    assert_eq!(msg.envelope().metadata.event_type, EventType::OrderCreated);
}

// ── Mixed event types through the pipeline ───────────────────────────

#[tokio::test]
async fn relay_mixed_event_types_across_topics() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();

    let order_topic = unique_topic();
    let payment_topic = unique_topic();
    admin
        .ensure_topics(&[
            TopicSpec::new(&order_topic, 1, 1),
            TopicSpec::new(&payment_topic, 1, 1),
        ])
        .await
        .unwrap();

    // Insert events for different aggregates/topics
    let order_id = Uuid::now_v7();
    let payment_id = Uuid::now_v7();

    let order_env = order_envelope(order_id);
    let payment_env = payment_envelope(payment_id);

    insert_outbox_event(
        &db.pool,
        &OutboxInsert::from_envelope(&order_topic, &order_env),
    )
    .await
    .unwrap();
    insert_outbox_event(
        &db.pool,
        &OutboxInsert::from_envelope(&payment_topic, &payment_env),
    )
    .await
    .unwrap();

    // Claim all and publish to their respective topics
    let claimed = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(claimed.len(), 2);

    let publisher = KafkaEventPublisher::new(&config).unwrap();
    for event in &claimed {
        let env: EventEnvelope = serde_json::from_value(event.payload.clone()).unwrap();
        publisher.publish(&event.topic, &env).await.unwrap();
        mark_published(&db.pool, event.id).await.unwrap();
    }

    // Consume from order topic
    let order_consumer = TestConsumer::new(&kafka.bootstrap_servers, &order_topic);
    let order_msg = order_consumer.recv().await;
    assert_eq!(
        order_msg.envelope().metadata.event_type,
        EventType::OrderCreated
    );
    assert_eq!(order_msg.envelope().metadata.aggregate_id, order_id);

    // Consume from payment topic
    let payment_consumer = TestConsumer::new(&kafka.bootstrap_servers, &payment_topic);
    let payment_msg = payment_consumer.recv().await;
    assert_eq!(
        payment_msg.envelope().metadata.event_type,
        EventType::PaymentAuthorized
    );
    assert_eq!(payment_msg.envelope().metadata.aggregate_id, payment_id);
}

// ── Per-aggregate ordering preserved ─────────────────────────────────

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

    // Two events for the SAME aggregate — order matters
    let agg_id = Uuid::now_v7();

    let created_metadata = EventMetadata::new(
        EventType::OrderCreated,
        AggregateType::Order,
        agg_id,
        SourceService::Order,
    );
    let created_env = EventEnvelope::new(created_metadata, json!({"step": 1}));

    let confirmed_metadata = EventMetadata::new(
        EventType::OrderConfirmed,
        AggregateType::Order,
        agg_id,
        SourceService::Order,
    );
    let confirmed_env = EventEnvelope::new(confirmed_metadata, json!({"step": 2}));

    insert_outbox_event(&db.pool, &OutboxInsert::from_envelope(&topic, &created_env))
        .await
        .unwrap();
    insert_outbox_event(
        &db.pool,
        &OutboxInsert::from_envelope(&topic, &confirmed_env),
    )
    .await
    .unwrap();

    let publisher = KafkaEventPublisher::new(&config).unwrap();

    // First claim: DISTINCT ON (aggregate_id) returns oldest → OrderCreated
    let batch1 = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch1.len(), 1, "only one event per aggregate per batch");
    assert_eq!(batch1[0].event_type, "OrderCreated");

    let env1: EventEnvelope = serde_json::from_value(batch1[0].payload.clone()).unwrap();
    publisher.publish(&topic, &env1).await.unwrap();
    mark_published(&db.pool, batch1[0].id).await.unwrap();

    // Second claim: now OrderConfirmed is eligible
    let batch2 = claim_batch(&db.pool, 10, "relay-1").await.unwrap();
    assert_eq!(batch2.len(), 1);
    assert_eq!(batch2[0].event_type, "OrderConfirmed");

    let env2: EventEnvelope = serde_json::from_value(batch2[0].payload.clone()).unwrap();
    publisher.publish(&topic, &env2).await.unwrap();
    mark_published(&db.pool, batch2[0].id).await.unwrap();

    // Consumer sees them in order: Created before Confirmed
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);

    let msg1 = consumer.recv().await;
    assert_eq!(msg1.envelope().metadata.event_type, EventType::OrderCreated);
    assert_eq!(msg1.envelope().payload["step"], 1);

    let msg2 = consumer.recv().await;
    assert_eq!(
        msg2.envelope().metadata.event_type,
        EventType::OrderConfirmed
    );
    assert_eq!(msg2.envelope().payload["step"], 2);
}

// ── Large payload through the full pipeline ──────────────────────────

#[tokio::test]
async fn large_payload_survives_full_pipeline() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Build a ~100KB payload (1000 items)
    let items: Vec<serde_json::Value> = (0..1000)
        .map(|i| {
            json!({
                "sku_id": Uuid::now_v7().to_string(),
                "name": format!("Product {i} with a reasonably long description for testing"),
                "quantity": i,
                "unit_price": format!("{}.99", i % 1000),
            })
        })
        .collect();

    let agg_id = Uuid::now_v7();
    let metadata = EventMetadata::new(
        EventType::OrderCreated,
        AggregateType::Order,
        agg_id,
        SourceService::Order,
    );
    let envelope = EventEnvelope::new(metadata, json!({"items": items}));

    // Full pipeline
    let insert = OutboxInsert::from_envelope(&topic, &envelope);
    insert_outbox_event(&db.pool, &insert).await.unwrap();

    let claimed = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    let stored: EventEnvelope = serde_json::from_value(claimed[0].payload.clone()).unwrap();

    let publisher = KafkaEventPublisher::new(&config).unwrap();
    publisher.publish(&topic, &stored).await.unwrap();
    mark_published(&db.pool, claimed[0].id).await.unwrap();

    // Verify it arrives intact
    let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
    let msg = consumer.recv().await;
    let received = msg.envelope();

    let received_items = received.payload["items"].as_array().unwrap();
    assert_eq!(received_items.len(), 1000);
    assert_eq!(
        received_items[0]["name"],
        "Product 0 with a reasonably long description for testing"
    );
    assert_eq!(received_items[999]["quantity"], 999);
}

// ── Metrics reflect published state ──────────────────────────────────

#[tokio::test]
async fn metrics_update_after_relay_publishes() {
    let db = TestDb::start(MIGRATIONS).await;
    let kafka = TestKafka::start().await;
    let config = kafka.kafka_config();
    let admin = KafkaAdmin::new(&config).unwrap();
    let topic = unique_topic();
    admin
        .ensure_topics(&[TopicSpec::new(&topic, 1, 1)])
        .await
        .unwrap();

    // Insert 2 events
    for _ in 0..2 {
        let env = order_envelope(Uuid::now_v7());
        insert_outbox_event(&db.pool, &OutboxInsert::from_envelope(&topic, &env))
            .await
            .unwrap();
    }

    // Before publish: 2 pending, 0 published
    let m1 = collect_outbox_metrics(&db.pool).await.unwrap();
    assert_eq!(m1.pending_count, 2);
    assert_eq!(m1.published_count, 0);

    // Publish one
    let claimed = claim_batch(&db.pool, 1, "relay-1").await.unwrap();
    let publisher = KafkaEventPublisher::new(&config).unwrap();
    let stored: EventEnvelope = serde_json::from_value(claimed[0].payload.clone()).unwrap();
    publisher.publish(&topic, &stored).await.unwrap();
    mark_published(&db.pool, claimed[0].id).await.unwrap();

    // After publish: 1 pending, 1 published
    let m2 = collect_outbox_metrics(&db.pool).await.unwrap();
    assert_eq!(m2.pending_count, 1);
    assert_eq!(m2.published_count, 1);
}
