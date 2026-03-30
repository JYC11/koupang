# Research: Shared Crate Background Processing Architecture

## Findings

### Component Structure

**Q1: What is the full structure of the `shared/src/outbox/` module? List every file, its public types, and its public functions. How are these re-exported through `mod.rs`?**

The `shared/src/outbox/` module contains 7 files:

| File | Public types / functions | Visibility |
|------|--------------------------|------------|
| `mod.rs` | Re-export hub | pub |
| `types.rs` | `OutboxStatus`, `OutboxEvent`, `OutboxInsert`, `FailureEscalation` (trait), `LogFailureEscalation`, `capture_trace_context()`, `RelayHeartbeat`, `OutboxMetrics` | `pub(crate)` module, individual types re-exported |
| `repository.rs` | `insert_outbox_event()`, `claim_batch()`, `mark_published()`, `delete_published()`, `mark_retry_or_failed()`, `release_stale_locks()`, `cleanup_published()`, `outbox_lag()`, `oldest_unpublished_age_secs()` | private module, all functions `pub`, re-exported via `pub use repository::*` |
| `relay.rs` | `OutboxRelay` (struct) | private module, `OutboxRelay` re-exported explicitly |
| `dedup.rs` | `is_published()`, `mark_published()` | `pub` module |
| `processed.rs` | `is_event_processed()`, `mark_event_processed()`, `cleanup_processed_events()` | private module, re-exported via `pub use processed::*` |
| `metrics.rs` | `collect_outbox_metrics()` | private module, re-exported via `pub use metrics::*` |

Re-export structure in `mod.rs` (lines 1-25):

```rust
pub mod dedup;
mod metrics;
mod processed;
mod relay;
mod repository;
pub(crate) mod types;

pub use crate::config::relay_config::RelayConfig;
pub use metrics::*;
pub use processed::*;
pub use relay::OutboxRelay;
pub use repository::*;
pub use types::{
    FailureEscalation, LogFailureEscalation, OutboxEvent, OutboxInsert,
    OutboxMetrics, OutboxStatus, RelayHeartbeat, capture_trace_context,
};
```

Notable: `dedup` is the only sub-module exposed as `pub mod` (callers can access `outbox::dedup::is_published()`). The `types` module is `pub(crate)` -- its members are individually re-exported. `RelayConfig` is re-exported from `config::relay_config`, not from the outbox module itself.

---

**Q2: What fields does `ServiceBuilder` hold, and what builder methods does it expose? Specifically, how does `with_consumers()` work -- what closure signature does it accept, and how does it interact with the `InfraDep` system?**

`ServiceBuilder` is defined at `/Users/admin/Desktop/code/koupang/shared/src/server.rs`, lines 135-140:

```rust
pub struct ServiceBuilder {
    name: &'static str,
    http_port_env: &'static str,
    deps: Vec<InfraDep>,
    consumer_factory: Option<ConsumerFactory>,
}
```

Where `ConsumerFactory` is (line 133):
```rust
type ConsumerFactory = Box<dyn FnOnce(&Infra) -> Vec<ConsumerRegistration>>;
```

Builder methods:

| Method | Signature | Effect |
|--------|-----------|--------|
| `new(name)` | `fn new(name: &'static str) -> Self` | Sets name, http_port_env="PORT", empty deps, no consumer_factory |
| `http_port_env(key)` | `fn http_port_env(mut self, key: &'static str) -> Self` | Overrides HTTP port env var key |
| `with_db(url_env)` | `fn with_db(mut self, url_env: &'static str) -> Self` | Pushes `InfraDep::Postgres { url_env, migrations_dir: "./migrations" }` |
| `with_db_migrations(url_env, dir)` | `fn with_db_migrations(mut self, url_env: &'static str, dir: &'static str) -> Self` | Same but custom migrations dir |
| `with_redis()` | `fn with_redis(mut self) -> Self` | Pushes `InfraDep::Redis` |
| `with_consumers(factory)` | `fn with_consumers<F>(mut self, factory: F) -> Self where F: FnOnce(&Infra) -> Vec<ConsumerRegistration> + 'static` | Stores factory, auto-adds `InfraDep::Kafka` if not present |
| `run(build_app)` | `async fn run<F>(self, build_app: F) -> Result<(), Box<dyn Error>> where F: FnOnce(&Infra) -> Router` | HTTP-only run |
| `run_with_grpc(grpc_config, build_app, build_grpc)` | HTTP + gRPC concurrent run |

`with_consumers()` (lines 184-193):
1. Checks if `InfraDep::Kafka` is already in `deps`; if not, pushes it
2. Wraps the closure in `Box::new()` and stores as `consumer_factory`
3. The closure receives `&Infra` (post-initialization), giving it access to `db`, `redis`, `kafka`

This is how the payment service passes `infra.require_db().clone()` and `infra.redis.clone()` into its `PaymentEventHandler` from within the consumer factory closure.

---

**Q3: How is the `Infra` struct defined, and what accessor methods does it provide? What resources are currently available through `Infra`?**

Defined at `shared/src/server.rs`, lines 63-67:

```rust
#[derive(Clone)]
pub struct Infra {
    pub db: Option<PgPool>,
    pub redis: Option<redis::aio::ConnectionManager>,
    pub kafka: Option<KafkaConfig>,
}
```

All three fields are `pub` and `Option`-wrapped -- only populated for declared `InfraDep` variants.

Accessor methods (lines 70-83):

| Method | Signature | Behavior |
|--------|-----------|----------|
| `require_db()` | `pub fn require_db(&self) -> &PgPool` | Unwraps `db` or panics with "BUG: service requires Postgres but ServiceBuilder was not configured with .with_db()" |
| `require_redis()` | `pub fn require_redis(&self) -> &redis::aio::ConnectionManager` | Unwraps `redis` or panics with "BUG: service requires Redis but ServiceBuilder was not configured with .with_redis()" |

There is no `require_kafka()` method -- `kafka` is accessed directly via `infra.kafka.as_ref()` in `spawn_consumers()` (line 271).

---

**Q4: How is the `OutboxRelay` struct defined? What fields does it hold, how is it constructed, and what is the signature of its `run()` method?**

Defined at `shared/src/outbox/relay.rs`, lines 24-32:

```rust
pub struct OutboxRelay {
    pool: PgPool,
    publisher: Arc<dyn EventPublisher>,
    config: RelayConfig,
    heartbeat: Arc<RelayHeartbeat>,
    redis: Option<redis::aio::ConnectionManager>,
}
```

Constructor (lines 35-43):
```rust
pub fn new(pool: PgPool, publisher: Arc<dyn EventPublisher>, config: RelayConfig) -> Self {
    Self {
        pool,
        publisher,
        config,
        heartbeat: Arc::new(RelayHeartbeat::new()),
        redis: None,
    }
}
```

Builder method for Redis dedup (lines 45-48):
```rust
pub fn with_redis(mut self, redis: Option<redis::aio::ConnectionManager>) -> Self {
    self.redis = redis;
    self
}
```

Heartbeat accessor (lines 54-56):
```rust
pub fn heartbeat(&self) -> Arc<RelayHeartbeat> {
    Arc::clone(&self.heartbeat)
}
```

Run method signature (line 59):
```rust
pub async fn run(self, shutdown: CancellationToken)
```

`run()` consumes `self`, takes a `CancellationToken`, and returns nothing (no `Result`). It runs indefinitely until the token is cancelled.

---

**Q5: What configuration types exist under `shared/src/config/`? For each config struct, list its fields, defaults, and how values are loaded.**

Six config structs across six files plus a `mod.rs` with helper functions.

**`mod.rs`** (lines 9-19) -- helper functions:
```rust
pub(crate) fn read_env_or(key: &str, default: String) -> String {
    std::env::var(key).unwrap_or(default)
}

pub(crate) fn parse_env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}
```

**`DbConfig`** (`db_config.rs`):
| Field | Type | Loading |
|-------|------|---------|
| `url` | `String` | `env!(db_url_key)` -- panics if unset |
| `max_connections` | `u32` | `env!("DB_MAX_CONNECTIONS")` -- panics if unset |

No defaults -- all required.

**`AuthConfig`** (`auth_config.rs`):
| Field | Type | Loading |
|-------|------|---------|
| `access_token_secret` | `Vec<u8>` | `env!("ACCESS_TOKEN_SECRET")` -- panics if unset |
| `refresh_token_secret` | `Vec<u8>` | `env!("REFRESH_TOKEN_SECRET")` -- panics if unset |
| `access_token_expiry_secs` | `u64` | `env!("ACCESS_TOKEN_EXPIRY")` -- panics if unset |
| `refresh_token_expiry_secs` | `u64` | `env!("REFRESH_TOKEN_EXPIRY")` -- panics if unset |

Has a `#[cfg(test)] for_tests()` returning hardcoded values (1s expiry).

**`RedisConfig`** (`redis_config.rs`):
| Field | Type | Loading |
|-------|------|---------|
| `url` | `String` | `new()`: panics if `REDIS_URL` unset. `try_new()`: returns `Option<Self>`, `None` if unset |

**`KafkaConfig`** (`kafka_config.rs`):
| Field | Type | Loading | Default |
|-------|------|---------|---------|
| `brokers` | `String` | `read_env_or("KAFKA_BROKERS", "localhost:29092")` | `"localhost:29092"` |

Also has `from_brokers(brokers)` for explicit construction (tests). Implements `Default` via `new()`.

**`RelayConfig`** (`relay_config.rs`):
| Field | Type | Default | Env Var |
|-------|------|---------|---------|
| `instance_id` | `String` | UUID v7 | `OUTBOX_RELAY_INSTANCE_ID` |
| `batch_size` | `i64` | 50 | `OUTBOX_RELAY_BATCH_SIZE` |
| `poll_interval` | `Duration` | 500ms | `OUTBOX_RELAY_POLL_INTERVAL_MS` |
| `stale_lock_check_interval` | `Duration` | 30s | `OUTBOX_RELAY_STALE_LOCK_CHECK_INTERVAL_SECS` |
| `stale_lock_timeout` | `Duration` | 60s | `OUTBOX_RELAY_STALE_LOCK_TIMEOUT_SECS` |
| `cleanup_interval` | `Duration` | 3600s | `OUTBOX_RELAY_CLEANUP_INTERVAL_SECS` |
| `cleanup_max_age` | `Duration` | 7 days | `OUTBOX_RELAY_CLEANUP_MAX_AGE_SECS` |
| `delete_on_publish` | `bool` | false | `OUTBOX_RELAY_DELETE_ON_PUBLISH` |
| `failure_escalation` | `Option<Arc<dyn FailureEscalation>>` | `None` | N/A (programmatic only) |

Has both `Default` impl and `from_env()`. Asserts: batch_size > 0, poll_interval > 0, stale_lock_timeout > stale_lock_check_interval.

**`ConsumerConfig`** (`consumer_config.rs`):
| Field | Type | Default | Env Var |
|-------|------|---------|---------|
| `group_id` | `String` | (required) | N/A |
| `topics` | `Vec<String>` | (required) | N/A |
| `max_retries` | `u32` | 3 | `EVENT_CONSUMER_MAX_RETRIES` |
| `retry_base_delay` | `Duration` | 1s | `EVENT_CONSUMER_RETRY_BASE_DELAY_MS` |
| `retry_max_delay` | `Duration` | 30s | `EVENT_CONSUMER_RETRY_MAX_DELAY_SECS` |
| `dlq_topic_override` | `Option<String>` | None | `EVENT_CONSUMER_DLQ_TOPIC` |
| `session_timeout` | `Duration` | 30s | `EVENT_CONSUMER_SESSION_TIMEOUT_SECS` |
| `auto_create_dlq_topics` | `bool` | true | `EVENT_CONSUMER_AUTO_CREATE_DLQ` |
| `processed_events_cleanup_interval` | `Duration` | 3600s | `EVENT_CONSUMER_CLEANUP_INTERVAL_SECS` |
| `processed_events_max_age` | `Duration` | 7 days | `EVENT_CONSUMER_CLEANUP_MAX_AGE_SECS` |

Has `new(group_id, topics)` (defaults for optional fields) and `from_env(group_id, topics)` (reads env vars). No `Default` impl -- `group_id` and `topics` are required.

---

### Data Flow

**Q6: Trace the full lifecycle of `ServiceBuilder::run()`: from infrastructure initialization through HTTP server startup, consumer spawning, and shutdown signal propagation.**

`ServiceBuilder::run()` at `shared/src/server.rs`, lines 197-216:

```
1. parse_http_port()                          → reads env var (default "3000")
2. Destructure self into name, deps, consumer_factory
3. do_init_infra(name, &deps)                 → static method:
   a. init_tracing(name)                      → sets up tracing subscriber
   b. Log deps: "order infra deps: [Postgres(ORDER_DB_URL), Redis, Kafka]"
   c. For each InfraDep:
      - Postgres → DbConfig::new(url_env) → init_db(config, migrations_dir)
                    (connects + runs migrations from CARGO_MANIFEST_DIR/migrations_dir)
      - Redis   → init_optional_redis()      (returns None if REDIS_URL unset)
      - Kafka   → KafkaConfig::new()         (reads KAFKA_BROKERS env)
   d. Return Infra { db, redis, kafka }
4. build_app(&infra)                          → user closure builds Router
5. .merge(health_routes(name))                → adds GET /health
6. CancellationToken::new()                   → creates shared shutdown token
7. spawn_consumers(name, consumer_factory, &infra, &shutdown)
   a. If no consumer_factory → return immediately
   b. Call factory(infra) → Vec<ConsumerRegistration>
   c. Get kafka_config from infra (panics if None)
   d. Get pool from infra.require_db()
   e. For each registration:
      - ConsumerConfig::new(group_id, topics)
      - KafkaEventConsumer::new(kafka_config, config, handler, pool.clone())
      - tokio::spawn(consumer.run(shutdown.clone()))
      - Log: "{name} consumer '{group_id}' started"
8. TcpListener::bind("0.0.0.0:{port}")
9. axum::serve(listener, app).await           → blocks until server stops
10. shutdown.cancel()                          → signals all background tasks to stop
```

**CancellationToken flow**: A single token is created in `run()`. Clones are passed to each spawned consumer via `shutdown.clone()`. The `shutdown.cancel()` call at line 214 only fires AFTER `axum::serve` returns (i.e., after the HTTP server stops). This means consumers outlive the HTTP server shutdown -- they keep running until `shutdown.cancel()` is called.

For `run_with_grpc()`, the pattern is identical except `tokio::select!` runs HTTP and gRPC concurrently; `shutdown.cancel()` fires when either exits.

---

**Q7: Trace the `OutboxRelay::run()` method end-to-end.**

`OutboxRelay::run()` at `shared/src/outbox/relay.rs`, lines 59-84:

```rust
pub async fn run(self, shutdown: CancellationToken) {
    let relay = Arc::new(self);   // wrap self in Arc for sharing across tasks

    // Spawn 3 concurrent loops, each gets Arc<Self> + shutdown clone
    let relay_handle = tokio::spawn(Self::relay_loop(Arc::clone(&relay), shutdown.clone()));
    let stale_handle = tokio::spawn(Self::stale_lock_loop(Arc::clone(&relay), shutdown.clone()));
    let cleanup_handle = tokio::spawn(Self::cleanup_loop(Arc::clone(&relay), shutdown.clone()));

    let _ = tokio::join!(relay_handle, stale_handle, cleanup_handle);
    tracing::info!("Outbox relay shut down gracefully");
}
```

**Relay loop** (lines 88-132):
```
1. connect_listener(pool) → Option<PgListener>  (listens on "outbox_events" channel)
2. loop {
     tokio::select! {
       biased;
       _ = shutdown.cancelled() => return;          // highest priority: exit
       notification = listener.recv() => {          // PG NOTIFY wakeup
         Ok(Some(_)) → "woken by PG notification"
         Err(e) → reconnect PgListener
       }
       _ = sleep(config.poll_interval) => {         // fallback poll (default 500ms)
         if listener.is_none() → try reconnect
       }
     }
     heartbeat.beat();
     process_pending(&shutdown).await;              // drain loop: up to 100 iterations
   }
```

`process_pending()` (lines 138-159) loops up to 100 times calling `process_batch()`. On `Ok(0)` it returns. On error, backs off 1s with shutdown-aware select.

**Stale lock loop** (lines 294-313):
```
loop {
  tokio::select! {
    biased;
    _ = shutdown.cancelled() => return;
    _ = sleep(config.stale_lock_check_interval) => {}
  }
  release_stale_locks(pool, timeout_secs).await;
}
```

**Cleanup loop** (lines 317-349):
```
loop {
  tokio::select! {
    biased;
    _ = shutdown.cancelled() => return;
    _ = sleep(config.cleanup_interval) => {}
  }
  // Drain in batches of 1000 until 0 returned or shutdown
  loop {
    if shutdown.is_cancelled() → break;
    cleanup_published(pool, max_age_secs)
    Ok(0) → break; Ok(n) → total += n; Err → break;
  }
}
```

**PgListener integration**: `connect_listener()` (lines 353-370) calls `PgListener::connect_with(pool)` then `listener.listen("outbox_events")`. The `outbox_events_after_insert` trigger (from migration `000001`) fires `pg_notify('outbox_events', NEW.id::text)` on every INSERT, waking the relay. If the listener connection fails, it falls back to poll-only mode and retries reconnection on each poll interval.

---

**Q8: Trace the Kafka consumer's message processing flow.**

`KafkaEventConsumer` at `shared/src/events/consumer.rs`:

**Top-level: `run()`** (lines 145-182):
```
1. Auto-create DLQ topics (if auto_create_dlq_topics=true)
2. Subscribe to topics
3. Spawn two concurrent loops:
   - message_loop (receives + processes messages)
   - cleanup_loop (periodically deletes old processed_events rows)
4. tokio::join! both handles
```

**Message loop** (lines 186-213): `tokio::select! { biased; shutdown, consumer.recv() }` -- processes one message at a time via `process_message()`.

**Per-message: `process_message()`** (lines 217-263):
```
1. Deserialize raw bytes → EventEnvelope
   Failure → publish_raw_to_dlq() → commit offset (or skip commit if DLQ fails)
2. process_with_retry(envelope, topic, shutdown)
   Success | Skipped | SentToDlq → commit offset
   DlqFailed | DbError → do NOT commit (message will be redelivered)
```

**Retry loop: `process_with_retry()`** (lines 266-343):
```
for attempt in 0..=max_retries:
  try_process_once(envelope, event_type, source, group)
    → Committed → return Success
    → AlreadyProcessed → return Skipped
    → DbError → return DbError
    → PermanentFailure → send to DLQ immediately, return
    → TransientFailure →
        if attempt >= max_retries → break to exhausted path
        record_retry metric
        backoff_or_shutdown(attempt, shutdown)
          if shutdown → send to DLQ with "shutdown during backoff" reason, return
          else → sleep(backoff), continue loop

After loop: send to DLQ (exhausted all retries)
```

**Single attempt: `try_process_once()`** (lines 346-413):
```
1. pool.begin() → tx (raw sqlx::Transaction, NOT TxContext)
2. is_event_processed(&mut *tx, event_id, consumer_group)
   → true: commit tx, return AlreadyProcessed
   → false: continue
   → Err: return DbError
3. handler.handle(envelope, &mut *tx)   ← handler gets &mut PgConnection INSIDE consumer's tx
   → Ok: mark_event_processed(&mut *tx, ...) → tx.commit() → Committed
   → Permanent(err): drop tx, return PermanentFailure(err)
   → Transient(err): drop tx, return TransientFailure(err)
```

**Backoff**: `calculate_backoff()` at line 581:
```rust
fn calculate_backoff(&self, attempt: u32) -> Duration {
    let base_ms = self.config.retry_base_delay.as_millis() as u64;
    let delay_ms = base_ms.saturating_mul(1u64 << attempt.min(10));
    let max_ms = self.config.retry_max_delay.as_millis() as u64;
    Duration::from_millis(delay_ms.min(max_ms))
}
```
With default base=1s, max=30s: 1s, 2s, 4s, 8s, 16s, 30s (capped). Bit shift capped at 10 to prevent overflow.

---

**Q9: How does `with_transaction()` work? What is the `TxContext` type?**

File: `shared/src/db/transaction_support.rs`

**`TxContext<'tx>`** (lines 27-29):
```rust
pub struct TxContext<'tx> {
    tx: Option<Transaction<'tx, Postgres>>,
}
```

The `'tx` lifetime is the borrow from the pool. The `Option` allows taking ownership for commit/rollback.

**`as_executor()`** (lines 52-57):
```rust
pub fn as_executor(&mut self) -> &mut PgConnection {
    self.tx
        .as_mut()
        .expect("Transaction has already been consumed")
        .deref_mut()
}
```

Calls `DerefMut` on `Transaction<'tx, Postgres>` to get `&mut PgConnection`. Panics if the transaction was already consumed (committed/rolled back).

**`with_transaction()`** (lines 71-92):
```rust
pub async fn with_transaction<F, T>(pool: &Pool<Postgres>, f: F) -> TxResult<T>
where
    F: for<'a> FnOnce(
        &'a mut TxContext<'_>,
    ) -> Pin<Box<dyn Future<Output = TxResult<T>> + Send + 'a>>,
    T: Send,
{
    let mut tx_ctx = TxContext::begin(pool).await?;
    match f(&mut tx_ctx).await {
        Ok(result) => { tx_ctx.commit().await?; Ok(result) }
        Err(e) => {
            if let Err(rb_err) = tx_ctx.rollback().await {
                tracing::warn!(error = %rb_err, "Transaction rollback failed");
            }
            Err(e)
        }
    }
}
```

The closure signature uses HRTB (`for<'a>`) and returns a `Pin<Box<dyn Future>>` -- this is needed to express the lifetime relationship between the `TxContext` borrow and the async closure. The `From<AppError> for TxError` impl (line 19) enables `?` propagation of `AppError` inside the closure.

**`with_nested_transaction()`** (lines 97-130):
```rust
pub async fn with_nested_transaction<F, T>(tx_ctx: &mut TxContext<'_>, f: F) -> TxResult<T>
```

Uses `SAVEPOINT` / `RELEASE SAVEPOINT` / `ROLLBACK TO SAVEPOINT` instead of begin/commit/rollback. A global `AtomicU64` counter (`SAVEPOINT_COUNTER`) generates unique savepoint names (`sp_0`, `sp_1`, ...) to prevent conflicts when nesting.

Key difference: `with_transaction` takes a `&Pool<Postgres>` and creates a new transaction; `with_nested_transaction` takes a `&mut TxContext<'_>` and creates a savepoint within the existing transaction.

---

### Patterns & Conventions

**Q10: What is the `EventHandler` trait's exact signature? How does the Kafka consumer call it?**

Defined at `shared/src/events/consumer.rs`, lines 65-72:

```rust
#[async_trait::async_trait]
pub trait EventHandler: Send + Sync {
    async fn handle(
        &self,
        envelope: &EventEnvelope,
        tx: &mut sqlx::PgConnection,
    ) -> Result<(), HandlerError>;
}
```

The consumer calls it at line 379 inside `try_process_once()`:
```rust
match self.handler.handle(envelope, &mut tx).await {
```

Here `tx` is a `sqlx::Transaction<'_, Postgres>` obtained from `self.pool.begin().await`. The `&mut tx` auto-derefs to `&mut PgConnection` because `Transaction` implements `DerefMut<Target = PgConnection>`. This means the handler's writes happen INSIDE the consumer's transaction. After the handler returns `Ok(())`, the consumer calls `mark_event_processed(&mut *tx, ...)` on the SAME transaction, then commits. This makes the handler's business writes and the idempotency marker atomic.

Note: The consumer uses raw `sqlx::Transaction` (pool.begin()), NOT `TxContext` from `transaction_support.rs`. This is a different transaction management pattern from what services use internally.

---

**Q11: How do services register Kafka consumers with `ServiceBuilder`? Show the `ConsumerRegistration` struct and a concrete example.**

`ConsumerRegistration` at `shared/src/server.rs`, lines 126-130:
```rust
pub struct ConsumerRegistration {
    pub group_id: String,
    pub topics: Vec<String>,
    pub handler: Arc<dyn EventHandler>,
}
```

**Order service** (`order/src/main.rs`, lines 1-24):
```rust
ServiceBuilder::new("order")
    .http_port_env("ORDER_PORT")
    .with_db("ORDER_DB_URL")
    .with_consumers(|_infra| {
        vec![ConsumerRegistration {
            group_id: "order-service".to_string(),
            topics: vec!["catalog.events".to_string(), "payments.events".to_string()],
            handler: Arc::new(OrderEventHandler::new()),
        }]
    })
    .run(|infra| {
        let app_state = AppState::new(infra.require_db().clone());
        app(app_state)
    })
    .await
```

The order service's factory closure ignores `_infra` -- `OrderEventHandler::new()` takes no arguments.

**Payment service** (`payment/src/main.rs`, lines 1-34):
```rust
ServiceBuilder::new("payment")
    .http_port_env("PAYMENT_PORT")
    .with_db("PAYMENT_DB_URL")
    .with_redis()
    .with_consumers(|infra| {
        let handler = Arc::new(PaymentEventHandler::new(
            infra.require_db().clone(),
            Arc::new(payment::gateway::mock::MockPaymentGateway::always_succeeds()),
            infra.redis.clone(),
        ));
        vec![ConsumerRegistration {
            group_id: "payment-service".to_string(),
            topics: vec![
                "catalog.events".to_string(),
                "orders.events".to_string(),
                "payments.events".to_string(), // self-consumption for capture retry
            ],
            handler,
        }]
    })
    .run(|infra| { ... })
    .await
```

The payment service's factory uses `infra.require_db()` and `infra.redis.clone()` to construct `PaymentEventHandler` with dependencies. This demonstrates why the consumer factory receives `&Infra` -- handlers need access to initialized infrastructure.

---

**Q12: How does `claim_batch()` use `FOR UPDATE SKIP LOCKED`? What is the two-step CTE strategy?**

`claim_batch()` at `shared/src/outbox/repository.rs`, lines 56-89:

```sql
WITH oldest_per_aggregate AS (
    SELECT DISTINCT ON (aggregate_id) id
    FROM outbox_events
    WHERE status = 'pending'
      AND next_retry_at <= NOW()
    ORDER BY aggregate_id, created_at ASC
),
locked AS (
    SELECT oe.id FROM outbox_events oe
    JOIN oldest_per_aggregate opa ON oe.id = opa.id
    WHERE oe.locked_by IS NULL
    FOR UPDATE OF oe SKIP LOCKED
    LIMIT $1
)
UPDATE outbox_events
SET locked_by = $2, locked_at = NOW()
FROM locked
WHERE outbox_events.id = locked.id
RETURNING outbox_events.*
```

**Step 1 (CTE `oldest_per_aggregate`)**: `DISTINCT ON (aggregate_id)` with `ORDER BY aggregate_id, created_at ASC` selects the OLDEST pending event per aggregate. This enforces per-aggregate ordering -- newer events for the same aggregate are NOT claimed until the oldest one is published.

**Step 2 (CTE `locked`)**: Joins back to `outbox_events`, filters `locked_by IS NULL`, and applies `FOR UPDATE OF oe SKIP LOCKED LIMIT $1`. `SKIP LOCKED` means rows already locked by another relay instance are silently skipped (no blocking). The `LIMIT` caps the batch size.

**Step 3 (UPDATE)**: Sets `locked_by` and `locked_at` on the claimed rows and returns them.

The `next_retry_at <= NOW()` filter in step 1 prevents claiming events in exponential backoff (their `next_retry_at` is in the future).

---

**Q13: How does the outbox system handle retry and failure escalation?**

**`mark_retry_or_failed()`** at `shared/src/outbox/repository.rs`, lines 136-166:

```sql
UPDATE outbox_events
SET
    status = CASE
        WHEN retry_count + 1 >= max_retries THEN 'failed'
        ELSE 'pending'
    END,
    retry_count = retry_count + 1,
    next_retry_at = CASE
        WHEN retry_count + 1 >= max_retries THEN next_retry_at
        ELSE NOW() + make_interval(secs => POW(2, LEAST(retry_count + 1, 10))::float8)
    END,
    last_error = $2,
    locked_by = NULL,
    locked_at = NULL
WHERE id = $1
```

**Exponential backoff**: `POW(2, LEAST(retry_count + 1, 10))` produces delays of 2s, 4s, 8s, 16s, 32s, ..., capped at 2^10 = 1024s (~17 min). The `LEAST(..., 10)` prevents numeric overflow.

**Status transition**: If `retry_count + 1 >= max_retries`, status becomes `'failed'` (terminal). Otherwise stays `'pending'` with updated `next_retry_at`.

**Lock release**: Always clears `locked_by` and `locked_at`, regardless of whether retrying or failing.

**Escalation path** (relay.rs, lines 228-256):

```rust
Err(e) => {
    let error_msg = e.to_string();
    match mark_retry_or_failed(&self.pool, event.id, &error_msg).await {
        Ok(()) => {
            // Escalate only AFTER the status transition succeeds
            if event.retry_count + 1 >= event.max_retries {
                self.escalate_failure(&event).await;
            }
        }
        Err(db_err) => {
            // DB update failed → event stays locked → stale lock recovery will free it
        }
    }
}
```

**`escalate_failure()`** (lines 276-290): Invokes `config.failure_escalation` (if set) or falls back to `LogFailureEscalation` (which logs the failure at error level). Escalation errors are caught and logged -- they never abort the batch.

Key design: Escalation runs ONLY after the DB status transition succeeds. This prevents spurious escalations when the DB update fails (the event stays pending and would be re-escalated on the next actual transition to `failed`).

---

**Q14: What SQL migration conventions does the project follow?**

**Shared test migrations** (`shared/tests/migrations/`):

Three files with numeric prefixes:
1. `000001_outbox_events.sql` -- outbox_events table + indexes + LISTEN/NOTIFY trigger
2. `000002_processed_events.sql` -- processed_events table + index
3. `000003_outbox_status_transition_trigger.sql` -- state machine enforcement trigger

Convention: `{6-digit-number}_{name}.sql`, no `up`/`down` split.

**Service migrations** (e.g., `order/migrations/`):

Single file: `202603151900_init.sql` -- timestamp-based naming: `{YYYYMMDDHHMI}_{name}.sql`.

The order migration includes ALL tables in one file:
- `orders` + `order_items` (business tables)
- `outbox_events` (identical schema to shared test migration, including NOTIFY trigger and status transition trigger)
- `processed_events` (identical schema to shared test migration)

**Migration directory references**:

`ServiceBuilder::with_db()` defaults to `"./migrations"` (relative to CARGO_MANIFEST_DIR). `ServiceBuilder::with_db_migrations()` accepts a custom path.

In `shared/src/db/mod.rs` line 22, `migrate_db()` resolves the path: `Path::new(&env!("CARGO_MANIFEST_DIR")).join(migrations_dir)`.

`TestDb::start()` takes a `migrations_dir` string -- tests pass `"tests/migrations"` (for shared crate tests) or `"../shared/tests/migrations"` (for service tests that need outbox tables). The resolution is the same: `Path::new(&CARGO_MANIFEST_DIR).join(migrations_dir)`.

Convention: Each service duplicates the outbox/processed_events DDL in its own migration file rather than depending on shared migrations at runtime. The shared `tests/migrations/` directory serves only the shared crate's own integration tests.

---

### Testing

**Q15: How does `TestDb::start()` work?**

File: `shared/src/test_utils/db.rs`

**Shared container pattern**:

```rust
static SHARED_PG: OnceCell<SharedPgContainer> = OnceCell::const_new();
```

`SharedPgContainer` holds:
- `_container: ContainerAsync<Postgres>` -- keeps the container alive
- `connection_base: String` -- e.g., `"postgres://postgres:postgres@localhost:55123"`
- `template_db: String` -- always `"test_template"`
- `db_counter: AtomicU32` -- per-test DB counter

**Lifecycle**:

1. First call to `TestDb::start(migrations_dir)` triggers `SHARED_PG.get_or_init()`:
   a. Start Postgres 18 container (`Postgres::default().with_tag("18.0-alpine3.21").start()`)
   b. Connect to `postgres` admin DB, `CREATE DATABASE test_template`
   c. Connect to `test_template`, run all migrations from `migrations_dir`
   d. Close template pool (required for Postgres to allow TEMPLATE usage)

2. Every subsequent call (including the first, after init):
   a. `db_counter.fetch_add(1)` → n
   b. Connect to `postgres` admin DB
   c. `CREATE DATABASE test_db_{n} TEMPLATE test_template` (~50-100ms file-level copy)
   d. Connect to `test_db_{n}` with max 5 connections
   e. Return `TestDb { pool }`

**Key detail** (comment at line 17): No pools are stored in `SharedPgContainer`. Each `#[tokio::test]` creates its own tokio runtime, so pools from one test's runtime cannot be reused in another. Only the connection URL and container handle are shared.

The container lives for the entire test binary execution (owned by the `OnceCell`). Individual test databases are created cheaply via TEMPLATE and are not explicitly dropped -- they're destroyed when the container exits.

---

**Q16: How are the outbox relay integration tests structured?**

File: `shared/tests/outbox_relay_tests.rs` (464 lines)

**Test helpers**:

| Helper | Purpose |
|--------|---------|
| `FailingPublisher` | Wraps a real publisher; fails the first N times (via `AtomicU32` countdown), then delegates to inner |
| `AlwaysFailPublisher` | Always returns `Err(AppError::InternalServerError(...))` |
| `TrackingEscalation` | Implements `FailureEscalation`; records failed event IDs in `Arc<Mutex<Vec<Uuid>>>` |
| `start_relay(pool, publisher, config)` | Creates `OutboxRelay`, spawns it on a tokio task, returns `CancellationToken` |
| `fast_relay_config()` | `RelayConfig` with poll_interval=50ms, stale_lock_check=1s, stale_lock_timeout=2s |
| `unique_topic()` | `format!("test-{}", Uuid::now_v7())` |
| `order_envelope(agg_id)` | Builds an `EventEnvelope` for `OrderCreated` |

**Timing control**:

Tests use `fast_relay_config()` with 50ms poll interval for fast feedback. For the retry test (`relay_retries_on_publish_failure`, line 225), after waiting 200ms for the first failure, the test manually updates the database:
```sql
UPDATE outbox_events SET next_retry_at = NOW() WHERE status = 'pending'
```
This bypasses the exponential backoff delay so the relay picks up the event immediately on the next poll.

For the PG notification test (`relay_wakes_on_pg_notification`, line 428), the poll_interval is set to 60s (won't fire during the test). The event is inserted AFTER the relay starts, proving that the PG LISTEN/NOTIFY trigger wakes the relay, not the poll fallback.

**Tests** (8 total):
1. `relay_publishes_pending_events` -- 3 events with different aggregates, verifies all arrive in Kafka
2. `relay_delete_on_publish_mode` -- verifies row is DELETE'd (not marked published)
3. `relay_retries_on_publish_failure` -- FailingPublisher(1), manual next_retry_at reset
4. `relay_escalates_permanent_failure` -- AlwaysFailPublisher + max_retries=1 + TrackingEscalation
5. `relay_preserves_per_aggregate_ordering` -- 2 events for same aggregate, verifies order
6. `relay_graceful_shutdown` -- cancel token, verify task exits within 5s
7. `relay_releases_stale_locks` -- manually set stale lock, verify relay frees and publishes
8. `relay_wakes_on_pg_notification` -- 60s poll, insert triggers NOTIFY, event arrives < 10s

---

**Q17: How are Kafka consumer integration tests structured?**

File: `shared/tests/consumer_tests.rs` (511 lines)

**`TestKafka::start()`** (`shared/src/test_utils/kafka.rs`):
```rust
static SHARED_KAFKA: OnceCell<SharedKafkaContainer> = OnceCell::const_new();
```
Uses `OnceCell` for shared container (same pattern as `TestDb`). Starts an Apache Kafka container (KRaft, no Zookeeper). Retries up to 3 times on `WaitLog(EndOfStream)` errors (intermittent Docker log-streaming issue). Returns `TestKafka { bootstrap_servers }`.

**`TestConsumer`** (`shared/src/test_utils/kafka.rs`, lines 101-162):
- Creates a `StreamConsumer` with a unique group ID (`test-group-{uuid}`)
- `recv()` method: loops with a 30s deadline, retries on transient `BrokerTransportFailure`, returns `ReceivedMessage { key, payload, headers }`

**Test infrastructure helpers**:
- `publish_to_topic(kafka, topic, envelope)` -- publishes directly to Kafka (bypasses outbox)
- `spawn_consumer(kafka, db, topic, handler)` -- creates `KafkaEventConsumer` with fast retry config (base=50ms, max=200ms), spawns it, returns `(shutdown_token, join_handle, group_id)`
- `wait_for(timeout, interval, predicate)` -- polls a predicate with interval, panics on timeout

**Tests** (8 total):
1. `consumer_processes_event_and_marks_processed` -- happy path, verifies handler called + processed_events row
2. `consumer_skips_duplicate_events` -- pre-inserts processed_events row, verifies handler NOT called
3. `consumer_retries_transient_errors` -- MockEventHandler queues 2 transient failures, verifies 3 calls total + eventual success
4. `consumer_sends_to_dlq_after_exhausting_retries` -- 4 transient failures (initial + 3 retries), verifies DLQ message headers
5. `consumer_sends_permanent_error_to_dlq_immediately` -- permanent error, verifies 1 call only + DLQ
6. `consumer_handles_deserialization_failure` -- publishes raw garbage bytes, verifies DLQ with raw bytes
7. `consumer_graceful_shutdown` -- cancel token, verify exit within 10s
8. `consumer_handler_writes_in_same_transaction` -- custom `WritingHandler` writes to DB inside tx, verifies both handler's write and idempotency marker are committed atomically

---

**Q18: What test utilities exist under `shared/src/test_utils/`?**

Seven modules (all gated behind `test-utils` feature):

| Module | File | Key exports |
|--------|------|-------------|
| `db` | `db.rs` | `TestDb` -- `start(migrations_dir)` returns `TestDb { pool }` |
| `redis` | `redis.rs` | `TestRedis` -- `start()` returns `TestRedis { conn }` with FLUSHDB for isolation |
| `kafka` | `kafka.rs` | `TestKafka` -- `start()` returns `TestKafka { bootstrap_servers }`, `.kafka_config()`; `TestConsumer` -- `new(brokers, topic)`, `.recv()` -> `ReceivedMessage`; `ReceivedMessage { key, payload, headers }`, `.envelope()` |
| `events` | `events.rs` | `make_envelope(event_type, aggregate_id, extra)` -> `EventEnvelope` |
| `auth` | `auth.rs` | `test_auth_config()`, `test_token(user)`, `seller_user()`, `buyer_user()`, `admin_user()` |
| `http` | `http.rs` | `body_bytes(response)`, `body_json(response)`, `json_request(method, uri, body)`, `authed_json_request(method, uri, token, body)`, `authed_get(uri, token)`, `authed_delete(uri, token)` |
| `grpc` | `grpc.rs` | `start_test_grpc_server(router)` -> `String` (URL) |

**`make_envelope()`** at `shared/src/test_utils/events.rs`:

```rust
pub fn make_envelope(
    event_type: EventType,
    aggregate_id: Uuid,
    extra: serde_json::Value,
) -> EventEnvelope
```

1. Calls `source_and_aggregate(&event_type)` -- a match on all `EventType` variants that returns the correct `(SourceService, AggregateType)` pair. This mapping covers all 12 event type variants.

2. Builds a payload: `{"order_id": aggregate_id.to_string()}` merged with the `extra` JSON object.

3. Constructs `EventMetadata::new(event_type, agg_type, aggregate_id, source)` which generates a fresh UUID v7 event_id and timestamp.

4. Returns `EventEnvelope::new(metadata, payload)`.

---

### Error Handling & Edge Cases

**Q19: How does the outbox relay handle individual event failures without aborting the entire batch?**

In `process_batch()` at `shared/src/outbox/relay.rs`, lines 166-260:

The relay iterates over claimed events in a `for` loop. Each event is processed independently:

**Successful publish but DB update fails** (lines 209-221):
```rust
let update_result = if self.config.delete_on_publish {
    delete_published(&self.pool, event.id).await
} else {
    mark_published(&self.pool, event.id).await
};
if let Err(db_err) = update_result {
    tracing::error!(..., "Failed to mark outbox event as published, stale lock recovery will free it");
    continue;  // does NOT abort the batch
}
```

**Failed publish** (lines 228-256): Calls `mark_retry_or_failed()`. If that DB update also fails, logs error and `continue`s. The event remains locked; the stale lock recovery loop will eventually free it.

**Redis dedup cache** (`shared/src/outbox/dedup.rs`):

The dedup cache prevents duplicate Kafka publishes in the scenario where:
1. Event is successfully published to Kafka
2. Redis dedup mark succeeds (line 205-207)
3. DB mark_published fails (e.g., transient DB error)

On the next relay cycle, the event will be claimed again (after stale lock timeout), but:
```rust
if let Some(ref redis) = self.redis {
    if dedup::is_published(redis, &event.event_id).await {
        // Skip re-publishing, just try DB mark again
        let _ = mark_published(&self.pool, event.id).await;
        continue;
    }
}
```

The Redis key has a TTL of 300 seconds (5 minutes), set via `SET EX` at `dedup.rs` line 28. The dedup is fail-open: if Redis is unavailable, `is_published()` returns `false` (line 18-19), and the event will be re-published to Kafka (at-least-once semantics).

The ordering of operations is deliberate (line 205 comment): Redis dedup mark happens BEFORE the DB update, so if the DB update fails, the dedup cache already has the entry for the next claim cycle.

---

**Q20: How does the Kafka consumer distinguish between transient and permanent handler errors?**

**`HandlerError`** at `shared/src/events/consumer.rs`, lines 22-26:
```rust
pub enum HandlerError {
    Transient(Box<dyn std::error::Error + Send + Sync>),
    Permanent(Box<dyn std::error::Error + Send + Sync>),
}
```

- `Transient` -- retryable (e.g., DB timeout, gateway 503)
- `Permanent` -- not retryable (e.g., invalid payload, unknown event type)
- `From<AppError>` maps to `Transient` (line 42-46): all `AppError` variants are treated as transient by default

**`AttemptResult`** (lines 639-650):
```rust
enum AttemptResult {
    Committed,            // handler Ok + mark_processed + commit
    AlreadyProcessed,     // idempotency skip
    DbError,              // DB failure (begin, check, mark, or commit)
    PermanentFailure(String),  // handler returned Permanent
    TransientFailure(String),  // handler returned Transient
}
```

**`ProcessResult`** (lines 625-636):
```rust
enum ProcessResult {
    Success,    // → commit offset
    Skipped,    // → commit offset
    SentToDlq,  // → commit offset
    DlqFailed,  // → do NOT commit (redelivery)
    DbError,    // → do NOT commit (redelivery)
}
```

**Offset commit decisions** (lines 256-262):
```rust
match ... {
    ProcessResult::Success | ProcessResult::Skipped | ProcessResult::SentToDlq => {
        self.commit_offset(msg);  // advance past this message
    }
    ProcessResult::DlqFailed | ProcessResult::DbError => {
        // Do NOT commit — message will be redelivered on next poll
    }
}
```

The key insight: `SentToDlq` commits the offset because the message has been safely moved to the DLQ. `DlqFailed` does NOT commit because the message would be lost (handler failed AND DLQ failed).

---

**Q21: How does the outbox status transition trigger enforce valid state machine transitions?**

File: `shared/tests/migrations/000003_outbox_status_transition_trigger.sql`

```sql
CREATE OR REPLACE FUNCTION enforce_outbox_status_transition() RETURNS trigger AS $$
BEGIN
    -- Self-transitions are always allowed
    IF OLD.status = NEW.status THEN
        RETURN NEW;
    END IF;
    -- Valid forward transitions
    IF OLD.status = 'pending' AND NEW.status IN ('published', 'failed') THEN
        RETURN NEW;
    END IF;
    -- Everything else is invalid
    RAISE EXCEPTION 'invalid outbox status transition: % → %', OLD.status, NEW.status
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER outbox_enforce_status_transition
    BEFORE UPDATE OF status ON outbox_events
    FOR EACH ROW EXECUTE FUNCTION enforce_outbox_status_transition();
```

**Allowed transitions**:
| From | To | Use case |
|------|-----|----------|
| pending | pending | retry with backoff, lock/unlock cycles |
| pending | published | successful Kafka publish |
| pending | failed | retries exhausted |
| published | published | idempotent mark_published |
| failed | failed | (self-transition, allowed but not used) |

**Rejected transitions** (raise exception with `check_violation` errcode):
| From | To | Reason |
|------|-----|--------|
| published | pending | cannot un-publish |
| published | failed | published is terminal-success |
| failed | pending | cannot resurrect without manual intervention |
| failed | published | cannot publish a failed event |

The trigger fires as `BEFORE UPDATE OF status` -- only when the `status` column specifically changes. Updates to other columns (e.g., `locked_by`, `retry_count`) without changing `status` do NOT trigger this check. This is important because `mark_retry_or_failed()` may keep `status = 'pending'` while updating `retry_count` and `next_retry_at`.

---

### Dependencies & Boundaries

**Q22: What external crates does the `shared` crate depend on?**

From `shared/Cargo.toml`:

**[dependencies]** (always included):

| Crate | Version | Features | Purpose |
|-------|---------|----------|---------|
| `sqlx` | 0.8.6 | macros, migrate, postgres, uuid, rust_decimal, chrono (no default-features) | Database |
| `uuid` | 1.21.0 | v4, v7, serde | UUID generation |
| `chrono` | 0.4.43 | serde | Date/time |
| `rust_decimal` | 1.4.0 | serde | Money types |
| `serde` | 1.0.228 | derive | Serialization |
| `serde_json` | 1.0.149 | -- | JSON |
| `thiserror` | 2.0.18 | -- | Error derives |
| `jsonwebtoken` | 10.3.0 | rust_crypto | JWT |
| `axum` | 0.8.8 | -- | HTTP framework |
| `tokio` | 1.49.0 | -- | Async runtime |
| `async-trait` | 0.1 | -- | Async trait support |
| `tracing` | 0.1.44 | -- | Structured logging |
| `tracing-subscriber` | 0.3.22 | env-filter | Log filtering |
| `tonic` | 0.14.5 | -- | gRPC |
| `prost` | 0.14.3 | -- | Protobuf |
| `prost-types` | 0.14.3 | -- | Protobuf well-known types |
| `redis` | 1.0.4 | tokio-comp, connection-manager | Redis client |
| `rdkafka` | 0.39 | cmake-build | Kafka client |
| `tokio-stream` | 0.1 | -- | Async streams |
| `tokio-util` | 0.7 | rt | CancellationToken |
| `tonic-prost` | 0.14.5 | -- | gRPC+Protobuf glue |

**Optional dependencies**:

| Crate | Version | Features | Gated by |
|-------|---------|----------|----------|
| `testcontainers-modules` | 0.15 | redis, postgres, kafka | `test-utils` |
| `http-body-util` | 0.1 | -- | `test-utils` |
| `opentelemetry` | 0.28 | -- | `telemetry` |
| `opentelemetry_sdk` | 0.28 | rt-tokio | `telemetry` |
| `opentelemetry-otlp` | 0.28 | grpc-tonic | `telemetry` |
| `tracing-opentelemetry` | 0.29 | -- | `telemetry` |

**[dev-dependencies]**:

| Crate | Version | Features |
|-------|---------|----------|
| `sqlx` | 0.8.6 | runtime-tokio-native-tls |
| `tokio` | 1.49.0 | macros, rt, test-util |
| `proptest` | 1 | -- |

**[build-dependencies]**:

| Crate | Version |
|-------|---------|
| `tonic-prost-build` | 0.14.5 |

---

**Q23: How is the `shared` crate's feature flag system structured?**

From `shared/Cargo.toml`, lines 35-37:

```toml
[features]
test-utils = ["dep:testcontainers-modules", "dep:http-body-util", "tokio/net"]
telemetry = ["dep:opentelemetry", "dep:opentelemetry_sdk", "dep:opentelemetry-otlp", "dep:tracing-opentelemetry"]
```

**`test-utils` feature gates**:
1. The `testcontainers-modules` dependency (Postgres, Redis, Kafka containers)
2. The `http-body-util` dependency (response body parsing in tests)
3. The `tokio/net` feature (TcpListener for test gRPC servers)
4. The entire `pub mod test_utils` module -- in `shared/src/lib.rs` line 21:
   ```rust
   #[cfg(feature = "test-utils")]
   pub mod test_utils;
   ```
5. `MockEventPublisher` and `MockEventHandler` in the events module:
   ```rust
   #[cfg(feature = "test-utils")]
   mod mock;
   #[cfg(feature = "test-utils")]
   pub use mock::MockEventPublisher;
   #[cfg(feature = "test-utils")]
   mod mock_handler;
   #[cfg(feature = "test-utils")]
   pub use mock_handler::MockEventHandler;
   ```

**How service crates depend on shared with test-utils**:

Services typically have in their `Cargo.toml`:
```toml
[dependencies]
shared = { path = "../shared" }

[dev-dependencies]
shared = { path = "../shared", features = ["test-utils"] }
```

The Makefile (`make test SERVICE=identity`) handles this automatically -- the CLAUDE.md mentions "The scripts handle the shared crate's `--features test-utils` flag automatically."

---

**Q24: How do `read_env_or()` and `parse_env_or()` work? How are they used across config structs?**

Defined at `shared/src/config/mod.rs`, lines 9-19:

```rust
pub(crate) fn read_env_or(key: &str, default: String) -> String {
    std::env::var(key).unwrap_or(default)
}

pub(crate) fn parse_env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}
```

Both are `pub(crate)` -- available within the shared crate only, not to downstream services.

**`read_env_or`**: Returns the env var as a `String`, or `default` if unset. Used by:
- `KafkaConfig::new()`: `read_env_or("KAFKA_BROKERS", "localhost:29092".to_string())`
- `RelayConfig::from_env()`: `read_env_or("OUTBOX_RELAY_INSTANCE_ID", Uuid::now_v7().to_string())`

**`parse_env_or`**: Reads the env var, attempts to parse it via `FromStr`, returns `default` if unset OR unparseable. The silent fallback on parse failure is intentional (tested in `relay_config_from_env_ignores_invalid_values`). Used by:
- `RelayConfig::from_env()`: all numeric/bool fields (batch_size, poll_interval_ms, delete_on_publish, etc.)
- `ConsumerConfig::from_env()`: all numeric/bool fields (max_retries, retry_base_delay_ms, auto_create_dlq, etc.)

Config structs that do NOT use these helpers (DbConfig, AuthConfig, RedisConfig) use `std::env::var().expect()` instead -- their values are required and the service panics if unset. The helpers are used only for configs that have sensible defaults.

---

## Observations

1. **Two transaction management patterns coexist**: The `TxContext`/`with_transaction()` system in `transaction_support.rs` (used by service-level code) and the raw `pool.begin()` pattern used by `KafkaEventConsumer`. The consumer uses raw `sqlx::Transaction` and passes `&mut PgConnection` to handlers. If a future persistent jobs system needs to pass a connection to job handlers (as the PRD envisions), it would follow the consumer's pattern rather than `TxContext`.

2. **OutboxRelay and KafkaEventConsumer have near-identical lifecycle structures**: Both wrap `self` in `Arc`, spawn concurrent loops via `tokio::spawn`, use `CancellationToken` with `biased` `tokio::select!`, and join all handles. Both have a main processing loop + a cleanup loop. This pattern is a clear template for any new background task system.

3. **No `require_kafka()` accessor on Infra**: While `require_db()` and `require_redis()` exist, Kafka config is accessed directly via `infra.kafka.as_ref()` in `spawn_consumers()`. This asymmetry means a hypothetical `with_job_runner()` builder method would need to decide which infra access pattern to follow.

4. **Consumer uses raw sqlx::Transaction, not TxContext**: This is significant for the persistent jobs PRD. The `EventHandler` trait receives `&mut sqlx::PgConnection`, not `&mut TxContext`. The consumer manages the transaction boundary itself (begin, idempotency check, handler call, mark processed, commit). A job runner that wants to give handlers transactional access would follow this same pattern.

5. **Mock/test types are feature-gated**: `MockEventPublisher` and `MockEventHandler` are behind `#[cfg(feature = "test-utils")]`. The `FailureEscalation` trait is always available (not gated), but `LogFailureEscalation` is the only built-in implementation.

6. **Configuration asymmetry**: `RelayConfig` and `ConsumerConfig` use `parse_env_or` for graceful defaults, while `DbConfig` and `AuthConfig` panic on missing env vars. The pattern for a new `JobRunnerConfig` would likely follow `RelayConfig` (env vars with defaults + `Default` impl).

7. **All outbox DDL is duplicated per service**: Each service's migration includes the full `outbox_events` + `processed_events` + trigger DDL. There is no shared migration mechanism at runtime.

8. **`outbox/LIFECYCLE.md` is comprehensive internal documentation**: Located at `shared/src/outbox/LIFECYCLE.md`, it contains ASCII diagrams of the happy path, all unhappy paths (Kafka failure, retries exhausted, relay crash, tx rollback), per-aggregate ordering examples, concurrent relay safety, consumer-side flow, and a state machine summary. This is a strong precedent for documenting a persistent jobs lifecycle in the same style.

8. **The `ServiceBuilder` does not currently spawn the OutboxRelay**: Despite the relay being part of the shared crate, there is no `with_outbox_relay()` or similar builder method. The PRD for persistent jobs mentions `ServiceBuilder::with_job_runner()` integration, which would be a new pattern alongside the existing `with_consumers()`.

## Could Not Determine

1. **How the OutboxRelay is actually started in production services**: There is no `with_outbox_relay()` method on `ServiceBuilder`, and no service `main.rs` file shows the relay being started. The relay exists as infrastructure but its production wiring is not visible in the current codebase -- it may be deferred or handled outside the checked-in code.

2. **Feature-gating of `MockEventPublisher` in non-test contexts**: The `mock.rs` file has `#[cfg(feature = "test-utils")]` at the module level in `events/mod.rs`, but the `MockEventPublisher` itself (in `shared/src/events/mock.rs`) does not have any feature gates on its own structs. If the feature gate were removed from `mod.rs`, it would compile without `test-utils`. This may be intentional for dev-mode usage (the payment service uses `MockPaymentGateway::always_succeeds()` in its main.rs).

3. **How the `make test` scripts handle `--features test-utils`**: The CLAUDE.md mentions automatic handling but the scripts themselves (in `scripts/`) were not explored.
