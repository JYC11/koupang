# Plan 1: Shared Infrastructure

## Context

Build the event-driven backbone before implementing business services (cart, order, payment). This includes Kafka setup, shared event abstractions, transactional outbox pattern, distributed tracing, and bootstrap extensions.

---
#### TODO need to read up more on Kafka and distributed tracing

## 1. Docker Compose Additions

File: `docker-compose.infra.yml`

### Kafka (KRaft mode, no Zookeeper)

```yaml
kafka:
  image: apache/kafka:3.9.0
  container_name: koupang-kafka
  environment:
    KAFKA_NODE_ID: 1
    KAFKA_PROCESS_ROLES: broker,controller
    KAFKA_LISTENERS: PLAINTEXT://0.0.0.0:9092,CONTROLLER://0.0.0.0:9093,EXTERNAL://0.0.0.0:29092
    KAFKA_ADVERTISED_LISTENERS: PLAINTEXT://kafka:9092,EXTERNAL://localhost:29092
    KAFKA_LISTENER_SECURITY_PROTOCOL_MAP: CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT,EXTERNAL:PLAINTEXT
    KAFKA_CONTROLLER_QUORUM_VOTERS: 1@kafka:9093
    KAFKA_CONTROLLER_LISTENER_NAMES: CONTROLLER
    KAFKA_INTER_BROKER_LISTENER_NAME: PLAINTEXT
    KAFKA_OFFSETS_TOPIC_REPLICATION_FACTOR: 1
    KAFKA_TRANSACTION_STATE_LOG_REPLICATION_FACTOR: 1
    KAFKA_TRANSACTION_STATE_LOG_MIN_ISR: 1
    KAFKA_AUTO_CREATE_TOPICS_ENABLE: "false"
  ports:
    - "29092:29092"
  healthcheck:
    test: ["/opt/kafka/bin/kafka-broker-api-versions.sh", "--bootstrap-server", "localhost:9092"]
    interval: 10s
    timeout: 10s
    retries: 5
```

### Kafka UI

```yaml
kafka-ui:
  image: provectuslabs/kafka-ui:latest
  container_name: koupang-kafka-ui
  depends_on:
    kafka:
      condition: service_healthy
  environment:
    KAFKA_CLUSTERS_0_NAME: koupang-local
    KAFKA_CLUSTERS_0_BOOTSTRAPSERVERS: kafka:9092
  ports:
    - "8080:8080"
```

### Topic Init

```yaml
kafka-init:
  image: apache/kafka:3.9.0
  depends_on:
    kafka:
      condition: service_healthy
  entrypoint: ["/bin/sh", "-c"]
  command: |
    "
    /opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --create --if-not-exists --topic orders.events --partitions 3 --replication-factor 1
    /opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --create --if-not-exists --topic inventory.events --partitions 3 --replication-factor 1
    /opt/kafka/bin/kafka-topics.sh --bootstrap-server kafka:9092 --create --if-not-exists --topic payments.events --partitions 3 --replication-factor 1
    "
  restart: "no"
```

### Jaeger (Distributed Tracing)

```yaml
jaeger:
  image: jaegertracing/all-in-one:1.67
  container_name: koupang-jaeger
  environment:
    COLLECTOR_OTLP_ENABLED: "true"
  ports:
    - "16686:16686"  # Jaeger UI
    - "4317:4317"    # OTLP gRPC
```

---

### Comments on Kafka:
- Any considerations for Dead Letter Queues?
- Kafka topic init/configuration stuff: is there a way to do this with code rather than scripting?
- We have to consider cases where Kafka may be down, so we need to have a retry mechanism.

## 2. Topic Naming Convention

Format: `{service}.events`, keyed by aggregate_id for partition ordering.

| Topic | Key | Events |
|-------|-----|--------|
| `orders.events` | order_id | OrderCreated, OrderConfirmed, OrderCancelled |
| `inventory.events` | order_id | InventoryReserved, InventoryReservationFailed |
| `payments.events` | order_id | PaymentAuthorized, PaymentFailed, PaymentCaptured, PaymentVoided |

---

## 3. Shared Event System

### New module: `shared/src/events/`

```
shared/src/events/
├── mod.rs              # re-exports, feature gates
├── types.rs            # EventEnvelope, EventMetadata, DomainEvent trait (always compiled)
├── producer.rs         # EventPublisher trait + KafkaEventPublisher (feature: kafka)
├── consumer.rs         # KafkaEventConsumer (feature: kafka)
├── mock.rs             # MockEventPublisher (feature: test-utils)
├── outbox.rs           # Outbox table CRUD (always compiled)
└── outbox_relay.rs     # Background poller (feature: kafka)
```

### Core Types (`types.rs`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub event_id: Uuid,           // UUID v7
    pub event_type: String,       // "OrderCreated"
    pub aggregate_type: String,   // "Order"
    pub aggregate_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub source_service: String,   // "order"
    pub correlation_id: Option<String>,
    pub causation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub metadata: EventMetadata,
    pub payload: serde_json::Value,
}
```

## Comment On EventMetadata
- perhaps we can use enums and value objects for the types here as well?

### EventPublisher Trait (`producer.rs`)

```rust
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, topic: &str, envelope: &EventEnvelope) -> Result<(), AppError>;
}

pub struct KafkaEventPublisher { producer: FutureProducer }
// Uses rdkafka with acks=all, idempotence enabled, 3 retries
```

### KafkaEventConsumer (`consumer.rs`)

```rust
pub struct KafkaEventConsumer { consumer: StreamConsumer }
// Manual commit (at-least-once), auto.offset.reset=earliest
// run() method accepts async handler closure
// Poison pill messages are committed to avoid infinite loop
```

### MockEventPublisher (`mock.rs`)

```rust
pub struct MockEventPublisher {
    pub events: Arc<Mutex<Vec<(String, EventEnvelope)>>>,
}
// Collects published events for test assertions
// Follows MockEmailService pattern
```

---

## 4. Transactional Outbox

### Schema (per-service migration template)

```sql
CREATE TABLE outbox (
    id             UUID PRIMARY KEY DEFAULT uuidv7(),
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    aggregate_type VARCHAR(100) NOT NULL,
    aggregate_id   UUID NOT NULL,
    event_type     VARCHAR(100) NOT NULL,
    topic          VARCHAR(255) NOT NULL,
    partition_key  VARCHAR(255) NOT NULL,
    payload        JSONB NOT NULL,
    published_at   TIMESTAMPTZ,
    retries        INTEGER NOT NULL DEFAULT 0,
    last_error     TEXT
);
CREATE INDEX idx_outbox_unpublished ON outbox (created_at) WHERE published_at IS NULL;
```

### Outbox Repository (`outbox.rs`)

- `insert_outbox_event(conn, topic, envelope)` — called within existing transactions
- `fetch_unpublished(conn, batch_size, max_retries)` — `SELECT ... FOR UPDATE SKIP LOCKED`
- `mark_published(conn, event_id)`
- `mark_failed(conn, event_id, error)`
- `cleanup_published(conn, before_date)`

### Outbox Relay (`outbox_relay.rs`)

```rust
pub fn spawn_outbox_relay(
    pool: PgPool,
    publisher: Arc<dyn EventPublisher>,
    config: OutboxRelayConfig,  // poll_interval: 500ms, batch: 50, max_retries: 5
) -> JoinHandle<()>
```

Background tokio task. Polls outbox, publishes to Kafka, marks published. Separate cleanup interval (hourly, 7-day retention).

### Integration with `with_transaction()`

```rust
// In service layer:
with_transaction(&self.pool, |tx| Box::pin(async move {
    // 1. Write domain data
    let order = repository::create_order(tx.as_executor(), ...).await?;
    // 2. Write outbox entry (same transaction)
    outbox::insert_outbox_event(tx.as_executor(), "orders.events", &envelope).await?;
    Ok(order)
})).await
```

No changes to `transaction_support.rs` needed.


## Comment on outbox
- use the outbox crate: https://crates.io/crates/outbox-core
- no need to reinvent the wheel because the outbox pattern gets complex quickly

---

## 5. Processed Events (Idempotent Consumers)

### Schema (per-service migration template)

```sql
CREATE TABLE processed_events (
    event_id     UUID PRIMARY KEY,
    event_type   VARCHAR(100) NOT NULL,
    processed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

### Usage Pattern

```rust
// In event handler:
if repository::is_event_processed(&pool, event.event_id).await? {
    return Ok(()); // Already handled
}
// ... handle event ...
// In same transaction:
repository::mark_event_processed(tx, event.event_id, "EventType").await?;
```

---

## 6. Distributed Tracing

### Enhanced `observability.rs`

Feature-gated behind `telemetry`. When `OTLP_ENDPOINT` env var is set, adds OTLP exporter layer alongside console fmt layer. When not set, falls back to current console-only behavior (no breaking change).

### Deps (all optional)

```toml
opentelemetry = { version = "0.28", optional = true }
opentelemetry_sdk = { version = "0.28", features = ["rt-tokio"], optional = true }
opentelemetry-otlp = { version = "0.28", features = ["tonic"], optional = true }
tracing-opentelemetry = { version = "0.29", optional = true }
```

### Trace Context Propagation

`correlation_id` field in `EventMetadata` carries W3C `traceparent` value through Kafka messages.

---

## 7. Server Bootstrap Extensions

### Redis-only service (`run_redis_service_with_infra`)

```rust
pub struct RedisServiceConfig {
    pub name: &'static str,
    pub port_env_key: &'static str,
}

pub async fn run_redis_service_with_infra<F>(
    config: RedisServiceConfig,
    build_app: F,
) -> Result<(), Box<dyn Error>>
where F: FnOnce(redis::aio::ConnectionManager) -> Router
```

No Postgres connection. Redis is required (panics if REDIS_URL not set).

#### Comments on Redis-only service
- at this point, it would be better to have a full service-builder abstraction
- I don't want to introduce many run_x_service_with_infra functions

### Event-driven service (`run_event_driven_service`)

Extension of `run_service_with_infra` that optionally initializes Kafka producer, spawns outbox relay, and spawns consumer tasks.

---

## 8. Cargo.toml Changes

```toml
[dependencies]
rdkafka = { version = "0.37", features = ["cmake-build"], optional = true }
opentelemetry = { version = "0.28", optional = true }
opentelemetry_sdk = { version = "0.28", features = ["rt-tokio"], optional = true }
opentelemetry-otlp = { version = "0.28", features = ["tonic"], optional = true }
tracing-opentelemetry = { version = "0.29", optional = true }

[features]
kafka = ["dep:rdkafka"]
telemetry = ["dep:opentelemetry", "dep:opentelemetry_sdk", "dep:opentelemetry-otlp", "dep:tracing-opentelemetry"]
```

---

## 9. Implementation Order

1. `events/types.rs` + `events/mock.rs` + `events/mod.rs` — zero external deps
2. `events/outbox.rs` + integration tests with TestDb
3. Docker compose additions (Kafka, Jaeger, topic init)
4. `events/producer.rs` — KafkaEventPublisher with rdkafka
5. `events/consumer.rs` — KafkaEventConsumer
6. `events/outbox_relay.rs` — background poller
7. `server.rs` — `RedisServiceConfig` + `run_redis_service_with_infra`
8. `server.rs` — `run_event_driven_service` extension
9. `observability.rs` — OTLP exporter (feature-gated)
10. `config/kafka_config.rs` — expand with group_id, try_new()
11. ADR-010: Event-driven architecture

## 10. Testing

- Unit tests: EventEnvelope serialization round-trips
- Integration: outbox insert/fetch/mark with TestDb
- Integration: outbox relay with MockEventPublisher + TestDb
- Optional (ignored by default): full Kafka round-trip with testcontainers

## 11. Files Summary

**New files** (~12):
- `shared/src/events/{mod,types,producer,consumer,mock,outbox,outbox_relay}.rs`

**Modified files** (~6):
- `docker-compose.infra.yml`
- `shared/Cargo.toml`
- `shared/src/lib.rs`
- `shared/src/server.rs`
- `shared/src/observability.rs`
- `shared/src/config/kafka_config.rs`

**Unchanged**: identity/, catalog/ (fully backward compatible)
