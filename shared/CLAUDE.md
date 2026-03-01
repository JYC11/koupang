# Shared Crate

Reusable libraries and infrastructure code shared across all microservices.

## File Layout

```
shared/src/
├── lib.rs                     # re-exports all modules
├── server.rs                  # run_service_with_infra(), ServiceConfig, GrpcConfig, NoGrpc
├── observability.rs           # init_tracing()
├── health.rs                  # health_routes() → GET /health
├── errors.rs                  # AppError enum → IntoResponse
├── responses.rs               # ok(), success(), created()
├── dto_helpers.rs             # fmt_id(), fmt_datetime(), fmt_datetime_opt()
├── auth/
│   ├── jwt.rs                 # JwtService, CurrentUser, AccessTokenClaims, JwtTokens
│   ├── middleware.rs          # AuthMiddleware (::new for identity, ::new_claims_based for others)
│   ├── guards.rs              # require_access(), require_admin()
│   └── role.rs                # Role enum (Buyer, Seller, Admin)
├── db/
│   ├── mod.rs                 # init_db(), PgPool, PgExec, PgConnection
│   ├── transaction_support.rs # TxContext, with_transaction(), with_nested_transaction()
│   └── pagination_support.rs  # keyset_paginate(), get_cursors(), PaginationParams (Default), PaginationRes, HasId
├── config/
│   ├── db_config.rs           # DbConfig::new(env_key)
│   ├── auth_config.rs         # AuthConfig::new(), ::for_tests()
│   ├── redis_config.rs        # RedisConfig::new(), ::try_new()
│   └── kafka_config.rs        # KafkaConfig::new(), ::from_brokers(), Default
├── events/
│   ├── mod.rs                 # re-exports
│   ├── types.rs               # EventEnvelope, EventMetadata, EventType, AggregateType, SourceService
│   ├── publisher.rs           # EventPublisher trait (async publish)
│   ├── admin.rs               # KafkaAdmin, TopicSpec — idempotent topic creation
│   ├── producer.rs            # KafkaEventPublisher — impl EventPublisher via rdkafka
│   └── mock.rs                # MockEventPublisher (captures events in Arc<Mutex<Vec>>)
├── outbox/
│   ├── mod.rs                 # re-exports
│   ├── types.rs               # OutboxEvent, OutboxInsert, OutboxStatus, RelayConfig, FailureEscalation trait
│   ├── repository.rs          # insert, claim_batch, mark_published, delete_published, mark_retry_or_failed, release_stale_locks, cleanup
│   ├── processed.rs           # is_event_processed, mark_event_processed, cleanup_processed_events
│   └── metrics.rs             # collect_outbox_metrics → OutboxMetrics
├── cache/mod.rs               # init_redis(), init_optional_redis()
├── email/mod.rs               # EmailService trait, EmailMessage, MockEmailService
├── grpc/mod.rs                # grpc::identity (generated protobuf)
└── test_utils/                # behind `test-utils` feature
    ├── auth.rs                # test_auth_config(), test_token(), seller_user(), buyer_user(), admin_user()
    ├── http.rs                # body_bytes(), body_json(), json_request(), authed_json_request(), authed_get(), authed_delete()
    ├── db.rs                  # TestDb::start(migrations_dir)
    ├── redis.rs               # TestRedis::start()
    ├── kafka.rs               # TestKafka::start(), TestConsumer::new(brokers, topic), ReceivedMessage
    └── grpc.rs                # start_test_grpc_server()
```

## Key APIs

| Module | Key exports |
|--------|-------------|
| `server` | `run_service_with_infra(ServiceConfig, grpc, build_app)` — full bootstrap |
| `db` | `init_db()`, `PgPool`, `PgExec<'e>` (reads), `PgConnection` (writes) |
| `db::transaction_support` | `with_transaction(pool, closure)`, `with_nested_transaction(tx, closure)`, `TxContext` |
| `db::pagination_support` | `keyset_paginate(params, alias, qb)`, `get_cursors(params, rows)`, `PaginationParams` (impl `Default`: limit=20, forward), `PaginationRes<T>`, `HasId` trait |
| `auth::jwt` | `JwtService::new(AuthConfig)`, `CurrentUser { id, role }` (axum extractor), `AccessTokenClaims` (axum extractor) |
| `auth::middleware` | `AuthMiddleware::new(jwt, user_lookup)` (identity), `::new_claims_based(jwt)` (other services, ADR-008) |
| `auth::guards` | `require_access(user, owner_id)`, `require_admin(user)` |
| `auth::role` | `Role` — Buyer, Seller, Admin |
| `config` | `DbConfig`, `AuthConfig`, `RedisConfig` (`.new()` / `.try_new()`), `KafkaConfig` (`.new()` / `.from_brokers()`) |
| `errors` | `AppError` — NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest |
| `responses` | `ok(data)`, `success(status, msg)`, `created(msg)` |
| `email` | `EmailService` trait, `MockEmailService` |
| `events` | `EventEnvelope`, `EventMetadata`, `EventType`, `AggregateType`, `SourceService`, `EventPublisher` trait, `MockEventPublisher`, `KafkaEventPublisher`, `KafkaAdmin`, `TopicSpec` |
| `outbox` | `OutboxInsert::from_envelope(topic, envelope)`, `insert_outbox_event()`, `claim_batch()`, `mark_published()`, `mark_retry_or_failed()`, `RelayConfig`, `FailureEscalation` trait |
| `outbox::processed` | `is_event_processed()`, `mark_event_processed()`, `cleanup_processed_events()` |
| `outbox::metrics` | `collect_outbox_metrics()` → `OutboxMetrics { pending_count, failed_count, published_count, oldest_pending_age_secs }` |

## Test Utilities (feature: `test-utils`)

| Helper | Purpose |
|--------|---------|
| `test_utils::auth::test_auth_config()` | Deterministic AuthConfig (3600s access, 7200s refresh) |
| `test_utils::auth::test_token(user)` | JWT access token for a `CurrentUser` |
| `test_utils::auth::{seller,buyer,admin}_user()` | `CurrentUser` with random UUID and role |
| `test_utils::http::body_bytes/body_json` | Parse response body |
| `test_utils::http::json_request` | Unauthenticated JSON request builder |
| `test_utils::http::authed_json_request` | Authenticated JSON request builder |
| `test_utils::http::authed_get/authed_delete` | Authenticated GET/DELETE builders |
| `test_utils::db::TestDb::start(dir)` | Shared Postgres 18 container; per-test DB via `CREATE DATABASE ... TEMPLATE` |
| `test_utils::redis::TestRedis::start()` | Shared Redis container; `FLUSHDB` per test for isolation |
| `test_utils::kafka::TestKafka::start()` | Shared Kafka container (KRaft); topic isolation via unique names |
| `test_utils::kafka::TestConsumer::new(brokers, topic)` | Kafka consumer with retry on transient errors; `.recv()` → `ReceivedMessage` |

## Transactional Outbox (events + outbox modules)

Services publish domain events by writing to a local `outbox_events` table inside the same
database transaction as the business operation. A background relay reads and publishes to Kafka.

### Producer side (writing events)

```rust
use shared::events::{EventEnvelope, EventMetadata, EventType, AggregateType, SourceService};
use shared::outbox::{OutboxInsert, insert_outbox_event, capture_trace_context};

// Inside a transaction:
let metadata = EventMetadata::new(EventType::OrderCreated, AggregateType::Order, order_id, SourceService::Order);
let envelope = EventEnvelope::new(metadata, serde_json::to_value(&payload)?);
let insert = OutboxInsert::from_envelope("order.events", &envelope)
    .with_metadata(capture_trace_context());
insert_outbox_event(&mut *tx, &insert).await?;
```

### Consumer side (idempotent processing)

```rust
use shared::outbox::{is_event_processed, mark_event_processed};

if is_event_processed(&pool, event_id).await? {
    return Ok(()); // already handled
}
// ... handle event ...
mark_event_processed(&pool, event_id, "OrderCreated", "catalog").await?;
```

### Relay lifecycle (claim → publish → ack)

```
claim_batch(pool, batch_size, instance_id)   → Vec<OutboxEvent>  (FOR UPDATE SKIP LOCKED)
  ├─ success → mark_published(pool, id)      → status='published', lock cleared
  ├─ success → delete_published(pool, id)    → row deleted (delete_on_publish mode)
  └─ failure → mark_retry_or_failed(pool, id, err) → exponential backoff (2^min(n,10) secs)
```

### Key design guarantees

- **Per-aggregate ordering**: `claim_batch` uses `DISTINCT ON (aggregate_id)` — only the oldest pending event per aggregate is claimed
- **No duplicate delivery**: `FOR UPDATE SKIP LOCKED` prevents two relays from claiming the same event
- **Exponential backoff**: retry delays are 2s, 4s, 8s, ..., capped at 1024s
- **Stale lock recovery**: `release_stale_locks()` frees events locked by crashed relays
- **LISTEN/NOTIFY**: `pg_notify('outbox_events', id)` trigger wakes the relay on insert
- **DB-enforced state machine**: `outbox_enforce_status_transition` trigger rejects invalid transitions (e.g. `published → pending`, `failed → *`)

### Migration template

Copy `.plan/outbox-migration-template.sql` into your service's migration directory.
Creates both `outbox_events` (producer) and `processed_events` (consumer) tables.

## Kafka Infrastructure (events module)

### Topic management

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

### Publishing events directly (used by the relay)

```rust
use shared::events::{KafkaEventPublisher, EventPublisher};

let publisher = KafkaEventPublisher::new(&config)?;
publisher.publish("order.events", &envelope).await?;
// Payload: JSON-serialized EventEnvelope
// Key: aggregate_id (partition affinity)
// Headers: event_id, event_type, aggregate_type, aggregate_id, source_service,
//          correlation_id (if set), causation_id (if set)
```

### Testing with Kafka

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

## Key Traits to Implement Per Service

| Trait | Module | When |
|-------|--------|------|
| `HasId` | `db::pagination_support` | Any paginated entity — `fn id(&self) -> Uuid` |
| `GetCurrentUser` | `auth::middleware` | Identity service only (others use claims-based) |
| `EmailService` | `email` | If service sends emails (use `MockEmailService` for dev) |
| `EventPublisher` | `events::publisher` | Publish events to Kafka (use `MockEventPublisher` for tests) |
| `FailureEscalation` | `outbox::types` | Handle permanently failed outbox events (default: `LogFailureEscalation`) |
