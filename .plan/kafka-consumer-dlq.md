# Plan: KafkaEventConsumer with DLQ Support (bd-3sv)

## Context

The project has a complete transactional outbox (producer → relay → Kafka) but no consumer infrastructure. Services that need to react to domain events (order listening for inventory_events, catalog listening for order_events, etc.) have no generic consumer to use. This blocks Plan 3 (order/payment saga) which requires Kafka consumers in every service.

## Design Decisions

1. **Single EventHandler trait per consumer** — each service creates a handler struct with internal `match` on event_type. No registration router (YAGNI at this scale).
2. **Inline retry with exponential backoff** — no intermediate retry topics. Transient errors get 3 retries (1s, 2s, 4s), permanent errors skip straight to DLQ.
   - **Trade-off:** inline retry blocks the consumer for up to 7s per transient failure across all partitions. Acceptable at current scale; if it becomes a problem, the fix is per-partition processing or a retry buffer, not retry topics.
   - Retry loop checks shutdown token between attempts — don't burn 7s during graceful shutdown.
3. **Per-source-topic DLQ** — `{topic}.dlq` convention (e.g. `order.events.dlq`). Optional single-topic override.
   - DLQ topics auto-created with **1 partition, replication factor 1** (DLQ consumption is manual/low-volume).
4. **Manual offset commit** — at-least-once delivery. Commit after successful handler OR successful DLQ publish.
5. **ConsumerConfig requires group_id + topics** — no Default (define errors out of existence).
6. **Transactional idempotency** — `EventHandler::handle` receives a `&mut PgConnection` (transaction). The consumer wraps idempotency check + handler + `mark_event_processed` in a single DB transaction. The DB commit is the real acknowledgment; Kafka offset commit is best-effort after.
7. **DLQ publish failure = no commit** — if DLQ publish fails, the offset is NOT committed. The message will be redelivered on next poll. Logged at `error!` level.

## Files to Create/Modify

### New: `shared/src/events/consumer.rs`

Contains all consumer types:

**HandlerError** enum:
```rust
pub enum HandlerError {
    Transient(Box<dyn std::error::Error + Send + Sync>),  // retryable
    Permanent(Box<dyn std::error::Error + Send + Sync>),  // straight to DLQ
}
```

Convenience constructors: `HandlerError::transient(msg: impl Into<String>)`, `HandlerError::permanent(msg: impl Into<String>)` for simple string errors. Also `impl From<AppError> for HandlerError` mapping infra errors to `Transient`.

**EventHandler** trait:
```rust
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    /// Handle an event inside a transaction.
    /// The consumer calls mark_event_processed in the same tx after success.
    async fn handle(&self, envelope: &EventEnvelope, tx: &mut PgConnection) -> Result<(), HandlerError>;
}
```

**ConsumerConfig** struct (follows RelayConfig pattern):
- `group_id: String` — required (e.g. `"order-consumer"`)
- `topics: Vec<String>` — required
- `max_retries: u32` — default 3
- `retry_base_delay: Duration` — default 1s
- `retry_max_delay: Duration` — default 30s
- `dlq_topic_override: Option<String>` — default None (`{topic}.dlq`)
- `session_timeout: Duration` — default 30s
- `auto_create_dlq_topics: bool` — default true
- `processed_events_cleanup_interval: Duration` — default 1 hour
- `processed_events_max_age: Duration` — default 7 days
- Constructors: `new(group_id, topics)` with defaults, `from_env(group_id, topics)` reads `EVENT_CONSUMER_*` env vars

**KafkaEventConsumer** struct:
```rust
pub struct KafkaEventConsumer {
    consumer: StreamConsumer,
    handler: Arc<dyn EventHandler>,
    pool: PgPool,
    producer: FutureProducer,  // for DLQ
    config: ConsumerConfig,
    kafka_config: KafkaConfig, // for DLQ topic auto-creation
}
```

Methods:
- `new(kafka_config, consumer_config, handler, pool) -> Result<Self, AppError>`
- `run(self, shutdown: CancellationToken)` — spawns message loop + processed_events cleanup loop (mirrors OutboxRelay::run pattern)
- `process_message(&self, msg) -> Result<(), AppError>` — deserialize → begin tx → idempotency check → handle → mark_processed → commit tx → kafka commit
- `handle_with_retry(&self, envelope, tx, shutdown) -> Result<(), HandlerError>` — retry loop, checks shutdown between retries
- `publish_to_dlq(&self, original_topic, envelope_or_raw, error, retry_count) -> Result<(), AppError>`
- `dlq_topic_for(&self, source_topic) -> String`

**Processing flow per message:**
```
deserialize payload → EventEnvelope
  ├─ deser failure → publish raw bytes to DLQ → commit offset (or skip on DLQ failure)
  └─ success → BEGIN TX
      ├─ is_event_processed(tx, event_id)?
      │   ├─ already processed → COMMIT → commit offset (skip)
      │   └─ not processed → handle_with_retry(envelope, tx, shutdown)
      │       ├─ Ok → mark_event_processed(tx) → COMMIT → commit offset
      │       └─ Err(exhausted/permanent) → ROLLBACK → publish_to_dlq
      │           ├─ DLQ Ok → commit offset
      │           └─ DLQ Err → do NOT commit (redeliver on next poll, log error!)
      └─ TX/DB error → do NOT commit offset (redeliver)
```

**Graceful shutdown behavior:**
1. Stop polling for new messages (select on shutdown token)
2. Finish processing the current in-flight message (do NOT abandon mid-handler)
3. Skip remaining retries if shutdown is signaled between retry attempts
4. Commit the last successful offset
5. Exit

**DLQ message format:**
- Key: original aggregate_id
- Payload: original EventEnvelope JSON (or raw bytes on deser failure)
- Headers: `dlq_reason`, `dlq_retry_count`, `dlq_original_topic`, `dlq_timestamp`, `dlq_consumer_group`

### Modify: `shared/src/events/mod.rs`

Add `mod consumer;` (behind `kafka` feature if gated, otherwise always) and re-exports:
```rust
pub use consumer::{ConsumerConfig, EventHandler, HandlerError, KafkaEventConsumer};
```

### New: `shared/src/events/mock_handler.rs` (behind `test-utils` feature)

```rust
pub struct MockEventHandler {
    results: Arc<Mutex<VecDeque<Result<(), HandlerError>>>>,
    received: Arc<Mutex<Vec<EventEnvelope>>>,
}
```
Methods: `new()`, `push_result(r)`, `always_ok()`, `received()`, `received_count()`

Note: mock handler receives `&mut PgConnection` but doesn't use it (just records the envelope).

Re-export from `mock.rs` or `mod.rs` under test-utils.

## Tasks (implementation order)

1. **HandlerError + EventHandler trait + ConsumerConfig** — types only, unit tests for config defaults and `from_env`
2. **KafkaEventConsumer::new + run loop** — StreamConsumer setup, message loop with shutdown, processed_events cleanup loop
3. **process_message + transactional idempotency** — deserialize, begin tx, dedup check, handler dispatch, mark processed, commit tx, kafka offset commit
4. **handle_with_retry** — exponential backoff, transient/permanent distinction, shutdown token check between retries
5. **publish_to_dlq** — DLQ publishing with failure headers, auto-create DLQ topics (1 partition), handle DLQ publish failure (no commit)
6. **MockEventHandler** — test utility for integration tests
7. **Update mod.rs** — module declaration + re-exports
8. **Integration tests** — in `shared/tests/consumer_tests.rs`

## Integration Tests (`shared/tests/consumer_tests.rs`)

Using TestKafka + TestDb + unique topics:

1. `consumer_processes_event_and_marks_processed` — happy path: event processed, DB has processed_events row, offset committed
2. `consumer_skips_duplicate_events` — pre-insert processed_events, verify handler not called
3. `consumer_retries_transient_errors` — handler fails 2x then succeeds, verify 3 calls total
4. `consumer_sends_to_dlq_after_exhausting_retries` — always-fail transient, check DLQ topic has message with headers
5. `consumer_sends_permanent_error_to_dlq_immediately` — permanent error, no retries, check DLQ
6. `consumer_handles_deserialization_failure` — publish garbage, check DLQ has raw bytes
7. `consumer_graceful_shutdown` — cancel token mid-processing, verify clean exit and no lost messages
8. `consumer_handler_writes_in_same_transaction` — handler inserts a row, verify it's committed atomically with processed_events marker

## Follow-up Tasks (not in this PR)

- **Consumer metrics**: `events_processed_total` (by event_type, outcome), `events_retried_total`, `event_processing_duration_seconds`, consumer lag gauge (rdkafka stats callback). Mirrors `outbox::metrics` pattern.
- **Rebalance handling**: investigate `ConsumerContext` for pre-rebalance offset commit to reduce duplicate processing during rebalancing.

## Verification

```bash
make test SERVICE=shared   # all existing + new tests pass
make check SERVICE=shared CLIPPY=1  # no new warnings
```

## Key files to reference during implementation

- `shared/src/events/producer.rs` — rdkafka ClientConfig/FutureProducer setup, header construction
- `shared/src/outbox/relay.rs` — CancellationToken run loop pattern, tokio::select!, cleanup loop
- `shared/src/outbox/processed.rs` — is_event_processed, mark_event_processed (reuse directly, takes `impl PgExecutor`)
- `shared/src/config/relay_config.rs` — env_or/env_parse pattern for config
- `shared/src/test_utils/kafka.rs` — TestKafka, TestConsumer for integration tests
- `shared/src/db/transaction_support.rs` — with_transaction pattern (reference, but consumer manages its own tx)
