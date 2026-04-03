# Kafka Infrastructure Guide

## Topic Management

```rust
use shared::events::{KafkaAdmin, TopicSpec};
use shared::config::kafka_config::KafkaConfig;

let config = KafkaConfig::new(); // reads KAFKA_BROKERS env, default "localhost:29092"
let admin = KafkaAdmin::new(&config)?;
admin.ensure_topics(&[
    TopicSpec::new("order.events", 3, 1),
    TopicSpec::new("payment.events", 3, 1)
        .with_config("retention.ms", "604800000"),
]).await?; // idempotent — existing topics silently skipped
```

## Publishing Events Directly (used by the relay)

```rust
use shared::events::{KafkaEventPublisher, EventPublisher};

let publisher = KafkaEventPublisher::new(&config)?;
publisher.publish("order.events", &envelope).await?;
// Payload: JSON-serialized EventEnvelope
// Key: aggregate_id (partition affinity)
// Headers: event_id, event_type, aggregate_type, aggregate_id, source_service,
//          correlation_id (if set), causation_id (if set)
```

## Testing with Kafka

```rust
use shared::test_utils::kafka::{TestKafka, TestConsumer};

let kafka = TestKafka::start().await;       // shared KRaft container via OnceCell
let config = kafka.kafka_config();          // KafkaConfig pointing at testcontainer
let topic = format!("test-{}", Uuid::now_v7()); // unique topic per test

// ... publish ...

let consumer = TestConsumer::new(&kafka.bootstrap_servers, &topic);
let msg = consumer.recv().await;            // retries on transient BrokerTransportFailure
let envelope = msg.envelope();              // deserialize payload as EventEnvelope
assert_eq!(msg.headers["event_type"], "OrderCreated");
```

## Kafka Event Consumer

Consumer with transactional idempotency and dead-letter queue (DLQ) support.

### Implementing a consumer

```rust
use shared::events::{KafkaEventConsumer, EventHandler, HandlerError, ConsumerConfig, EventEnvelope};
use shared::config::kafka_config::KafkaConfig;
use tokio_util::sync::CancellationToken;

// 1. Implement EventHandler
struct MyHandler;

#[async_trait::async_trait]
impl EventHandler for MyHandler {
    async fn handle(&self, envelope: &EventEnvelope, tx: &mut sqlx::PgConnection) -> Result<(), HandlerError> {
        match envelope.metadata.event_type {
            EventType::OrderCreated => { /* handle in tx */ Ok(()) }
            _ => Err(HandlerError::permanent("unknown event type"))
        }
    }
}

// 2. Configure, grab metrics handle, and run
let config = ConsumerConfig::new("order-consumer", vec!["order.events".into()]);
let consumer = KafkaEventConsumer::new(&kafka_config, config, Arc::new(MyHandler), pool)?;
let metrics = consumer.metrics(); // Arc<ConsumerMetricsCollector> — pass to health endpoint
consumer.run(shutdown_token).await;
// metrics.snapshot() → ConsumerMetrics { events_processed, events_retried, ... }
```

### Processing guarantees

- **At-least-once delivery** — offset committed after handler success or DLQ publish
- **Transactional idempotency** — handler runs inside a DB transaction with `processed_events` marker; committed atomically
- **Inline retry** — transient errors get up to `max_retries` attempts with exponential backoff (1s, 2s, 4s default)
- **Per-source-topic DLQ** — failed events go to `{topic}.dlq` with headers: `dlq_reason`, `dlq_retry_count`, `dlq_original_topic`, `dlq_timestamp`, `dlq_consumer_group`
- **Graceful shutdown** — finishes in-flight message, skips remaining retries
