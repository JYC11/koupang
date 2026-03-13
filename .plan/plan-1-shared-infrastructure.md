# Plan 1: Shared Infrastructure (Revised)

## Context

Build the event-driven backbone before implementing business services (cart, order, payment). This includes Kafka setup, shared event abstractions, transactional outbox (via `outbox-core` crate), distributed tracing, and a composable service builder.

---

## 1. Docker Compose Additions

File: `docker-compose.infra.yml`

### Kafka (KRaft mode, no Zookeeper)

```yaml
kafka:
  image: apache/kafka-native:3.9.2
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
    test: ["CMD-SHELL", "bash -c 'cat < /dev/tcp/localhost/9092 &>/dev/null'"]
    interval: 5s
    timeout: 5s
    retries: 10
    start_period: 5s
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

**No `kafka-init` container** — topic creation is handled programmatically at service startup (see §3.6).

---

## 2. Topic Naming Convention

Format: `{service}.events`, keyed by aggregate_id for partition ordering.
Each service topic has a corresponding DLQ: `{service}.events.dlq`.

| Topic | DLQ | Key | Events |
|-------|-----|-----|--------|
| `orders.events` | `orders.events.dlq` | order_id | OrderCreated, OrderConfirmed, OrderCancelled |
| `inventory.events` | `inventory.events.dlq` | order_id | InventoryReserved, InventoryReservationFailed |
| `payments.events` | `payments.events.dlq` | order_id | PaymentAuthorized, PaymentFailed, PaymentCaptured, PaymentVoided |

**DLQ strategy**: After max retries (5), the consumer forwards the poison message to the DLQ topic (with original metadata preserved) and commits the offset. DLQ messages are inspectable via Kafka UI and can be replayed manually.

---

## 3. Shared Event System

### New module: `shared/src/events/`

```
shared/src/events/
├── mod.rs              # re-exports, feature gates
├── types.rs            # EventEnvelope, EventMetadata, typed enums (always compiled)
├── producer.rs         # EventPublisher trait + KafkaEventPublisher (feature: kafka)
├── consumer.rs         # KafkaEventConsumer with DLQ support (feature: kafka)
├── admin.rs            # Programmatic topic creation via AdminClient (feature: kafka)
├── mock.rs             # MockEventPublisher (feature: test-utils)
└── health.rs           # Kafka connectivity health check (feature: kafka)
```

### 3.1 Core Types (`types.rs`) — Typed Enums

```rust
/// Each service defines its own variants; shared provides the trait + envelope.
/// Serialized as PascalCase strings on the wire (e.g. "OrderCreated").
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum AggregateType {
    Order,
    Payment,
    Inventory,
    // Extensible per service via feature gates or a generic string fallback
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum EventType {
    // Order events
    OrderCreated, OrderConfirmed, OrderCancelled,
    // Inventory events
    InventoryReserved, InventoryReservationFailed, InventoryReleased,
    // Payment events
    PaymentAuthorized, PaymentFailed, PaymentCaptured, PaymentVoided,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub event_id: Uuid,               // UUID v7
    pub event_type: EventType,        // typed enum (was String)
    pub aggregate_type: AggregateType, // typed enum (was String)
    pub aggregate_id: Uuid,
    pub timestamp: DateTime<Utc>,
    pub source_service: String,       // "order", "payment", etc.
    pub correlation_id: Option<String>,
    pub causation_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub metadata: EventMetadata,
    pub payload: serde_json::Value,
}
```

### 3.2 EventPublisher Trait (`producer.rs`)

```rust
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, topic: &str, envelope: &EventEnvelope) -> Result<(), AppError>;
}

pub struct KafkaEventPublisher { producer: FutureProducer }
// Uses rdkafka with acks=all, idempotence enabled, 3 retries
```

### 3.3 KafkaEventConsumer (`consumer.rs`)

```rust
pub struct KafkaEventConsumer { consumer: StreamConsumer, dlq_producer: FutureProducer }
// Manual commit (at-least-once), auto.offset.reset=earliest
// run() method accepts async handler closure
// On handler failure after max retries: forward to DLQ topic, commit original offset
```

### 3.4 MockEventPublisher (`mock.rs`)

```rust
pub struct MockEventPublisher {
    pub events: Arc<Mutex<Vec<(String, EventEnvelope)>>>,
}
// Collects published events for test assertions
// Follows MockEmailService pattern
```

### 3.5 Kafka Health Check (`health.rs`)

```rust
pub async fn kafka_health_check(producer: &FutureProducer) -> HealthStatus {
    // Attempts metadata fetch with short timeout
    // Returns Healthy / Degraded / Unhealthy
    // Used by service health endpoint — reports degraded (not hard-fail)
    // Service can still accept writes (outbox buffers events)
}
```

### 3.6 Programmatic Topic Creation (`admin.rs`)

```rust
pub async fn ensure_topics(
    bootstrap_servers: &str,
    topics: &[TopicSpec],  // name, partitions, replication_factor
) -> Result<(), AppError>
// Uses rdkafka AdminClient
// Idempotent: creates only if topic doesn't exist
// Called at service startup before spawning consumers
// Eliminates kafka-init Docker container
```

---

## 4. Transactional Outbox (via `outbox-core` crate)

Use the [`outbox-core`](https://crates.io/crates/outbox-core) crate instead of hand-rolling the outbox pattern. The outbox pattern gets complex quickly (polling, retry, batching, cleanup) and this crate handles it.

### Schema (per-service migration template)

The outbox table schema will follow `outbox-core`'s expected format. We still own the migration since each service has its own DB.

```sql
-- Schema will be adapted to outbox-core's requirements during implementation.
-- Core concept remains: outbox row written in same transaction as domain data.
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

### Integration with `with_transaction()`

```rust
// In service layer — same pattern, outbox-core handles the relay/polling side:
with_transaction(&self.pool, |tx| Box::pin(async move {
    let order = repository::create_order(tx.as_executor(), ...).await?;
    outbox::insert_outbox_event(tx.as_executor(), "orders.events", &envelope).await?;
    Ok(order)
})).await
```

No changes to `transaction_support.rs` needed.

### Kafka Down Resilience

- **Producer side**: If Kafka is unreachable, the outbox relay's publish calls fail. Outbox entries stay unpublished and retry on next poll interval (500ms). No data loss — writes are already committed to Postgres.
- **Consumer side**: `rdkafka` has built-in reconnection with configurable backoff. Consumers automatically resume from last committed offset when Kafka comes back.
- **Health reporting**: `/health` endpoint reports Kafka as "degraded" (not a hard failure). Service continues accepting HTTP requests.

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

## 7. Service Builder (Composable Bootstrap)

Replace proliferating `run_*_service_with_infra()` functions with a composable builder:

```rust
ServiceBuilder::new("cart")
    .with_redis()                              // optional — connects to Redis
    .with_postgres()                           // optional — connects to Postgres, runs migrations
    .with_kafka_producer()                     // optional — initializes FutureProducer
    .with_kafka_consumers(consumer_configs)     // optional — spawns consumer tasks
    .with_outbox_relay(outbox_config)          // optional — spawns outbox polling task
    .build(|infra| {
        // infra provides .pg_pool(), .redis_conn(), .kafka_producer() etc.
        // Returns Router
        app(infra)
    })
    .run()
    .await
```

**Benefits**:
- Each service composes only the infra it needs
- No more `run_redis_service_with_infra`, `run_event_driven_service`, etc.
- Existing `run_service_with_infra()` becomes a convenience wrapper (or services migrate to builder)
- Easy to add new infra (e.g., gRPC, S3) without new top-level functions

**Example service configurations**:
- Cart: `.with_redis()` only
- Identity: `.with_postgres()` only (existing behavior)
- Order: `.with_postgres().with_kafka_producer().with_kafka_consumers(...).with_outbox_relay(...)`

---

## 8. Cargo.toml Changes

```toml
[dependencies]
rdkafka = { version = "0.37", features = ["cmake-build"], optional = true }
outbox-core = { version = "...", optional = true }  # version TBD after research
opentelemetry = { version = "0.28", optional = true }
opentelemetry_sdk = { version = "0.28", features = ["rt-tokio"], optional = true }
opentelemetry-otlp = { version = "0.28", features = ["tonic"], optional = true }
tracing-opentelemetry = { version = "0.29", optional = true }

[features]
kafka = ["dep:rdkafka", "dep:outbox-core"]
telemetry = ["dep:opentelemetry", "dep:opentelemetry_sdk", "dep:opentelemetry-otlp", "dep:tracing-opentelemetry"]
```

---

## 9. Implementation Order

1. `events/types.rs` — typed enums (`EventType`, `AggregateType`), `EventEnvelope`, `EventMetadata`
2. `events/mock.rs` + `events/mod.rs` — zero external deps, test infrastructure
3. Research `outbox-core` API — verify compatibility with sqlx + `with_transaction()` pattern
4. Docker compose additions (Kafka, Jaeger — no kafka-init container)
5. `events/admin.rs` — programmatic topic creation via `AdminClient`
6. `events/producer.rs` — `KafkaEventPublisher` with rdkafka
7. `events/consumer.rs` — `KafkaEventConsumer` with DLQ support
8. `events/health.rs` — Kafka connectivity health check
9. Outbox integration via `outbox-core` crate + migration templates
10. `server.rs` — `ServiceBuilder` composable bootstrap
11. `observability.rs` — OTLP exporter (feature-gated)
12. `config/kafka_config.rs` — expand with group_id, try_new()
13. ADR-010: Event-driven architecture

## 10. Testing

- Unit tests: EventEnvelope serialization round-trips, typed enum serde
- Unit tests: EventType/AggregateType enum coverage
- Integration: outbox insert/fetch/mark with TestDb (via outbox-core)
- Integration: DLQ forwarding (consumer sends poison pill to DLQ after max retries)
- Integration: ServiceBuilder constructs correct infra combinations
- Optional (ignored by default): full Kafka round-trip with testcontainers

## 11. Files Summary

**New files** (~12):
- `shared/src/events/{mod,types,producer,consumer,admin,mock,health}.rs`

**Modified files** (~5):
- `docker-compose.infra.yml`
- `shared/Cargo.toml`
- `shared/src/lib.rs`
- `shared/src/server.rs` (ServiceBuilder)
- `shared/src/observability.rs`

**Unchanged**: identity/, catalog/ (fully backward compatible)
