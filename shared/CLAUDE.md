# Shared Crate

Reusable libraries and infrastructure code shared across all microservices.

## File Layout

```
shared/src/
├── lib.rs                     # re-exports all modules
├── server.rs                  # ServiceBuilder, Infra, GrpcConfig — composable service bootstrap
├── observability.rs           # init_tracing() — console + optional OTLP exporter (feature: `telemetry`)
├── health.rs                  # health_routes() → GET /health
├── errors.rs                  # AppError enum → IntoResponse
├── responses.rs               # ok(), success(), created()
├── rules.rs                   # Rule<A> algebra — composable business rules as data (ADR-012)
├── dto_helpers.rs             # fmt_id(), fmt_datetime(), fmt_datetime_opt()
├── circuit_breaker.rs         # CircuitBreaker, CircuitBreakerConfig, BreakerStatus — generic count-based sliding window
├── distributed_lock.rs        # DistributedLock, LockGuard, LockError — Redis SETNX + Lua atomic release
├── auth/
│   ├── jwt.rs                 # jwt:: free functions (generate/validate tokens), CurrentUser, AccessTokenClaims, JwtTokens
│   ├── middleware.rs          # AuthMiddleware (::new for identity, ::new_claims_based for others)
│   ├── guards.rs              # require_access(), require_admin()
│   └── role.rs                # Role enum (Buyer, Seller, Admin)
├── db/
│   ├── mod.rs                 # init_db() → Result, PgPool, PgExec, PgConnection
│   ├── transaction_support.rs # TxContext, with_transaction(), with_nested_transaction() — logs rollback errors
│   └── pagination_support.rs  # keyset_paginate(), get_cursors(), PaginationParams (Default), PaginationRes, PaginatedResponse, HasId
├── config/
│   ├── db_config.rs           # DbConfig::new(env_key)
│   ├── auth_config.rs         # AuthConfig::new(), ::for_tests()
│   ├── redis_config.rs        # RedisConfig::new(), ::try_new()
│   ├── kafka_config.rs        # KafkaConfig::new(), ::from_brokers(), Default
│   ├── relay_config.rs        # RelayConfig::from_env(), Default
│   ├── job_runner_config.rs   # JobRunnerConfig::from_env(), Default
│   └── consumer_config.rs     # ConsumerConfig::new(group_id, topics), ::from_env()
├── jobs/
│   ├── mod.rs                 # re-exports
│   ├── types.rs               # Job, JobName, JobStatus, JobError, JobConfig, JobSchedule, RecurringJobDefinition, RecurringFailurePolicy, DedupStrategy
│   ├── registry.rs            # JobHandler trait, JobRegistry (register, register_recurring)
│   ├── repository.rs          # enqueue_job, claim_batch, mark_completed, mark_retry_or_failed, mark_dead_lettered, seed_recurring_job, reset_recurring, release_stale_locks, cleanup_completed
│   └── runner.rs              # JobRunner — 3-loop background task (claim/execute, stale lock recovery, cleanup), InFlightGuard, compute_next_run_at, count_missed_ticks
├── events/
│   ├── mod.rs                 # re-exports
│   ├── types.rs               # EventEnvelope, EventMetadata, EventType, AggregateType, SourceService
│   ├── publisher.rs           # EventPublisher trait (async publish)
│   ├── admin.rs               # KafkaAdmin, TopicSpec — idempotent topic creation
│   ├── producer.rs            # KafkaEventPublisher — impl EventPublisher via rdkafka
│   ├── consumer.rs            # KafkaEventConsumer, EventHandler, HandlerError, ConsumerConfig — consumer with DLQ
│   ├── health.rs              # KafkaHealthChecker, KafkaHealth, KafkaHealthStatus — broker connectivity check
│   ├── metrics.rs             # ConsumerMetricsCollector, ConsumerMetrics — in-memory consumer counters
│   ├── mock.rs                # MockEventPublisher (captures events in Arc<Mutex<Vec>>)
│   └── mock_handler.rs        # MockEventHandler (test-utils) — queued results + received tracking
├── outbox/
│   ├── mod.rs                 # re-exports
│   ├── types.rs               # OutboxEvent, OutboxInsert, OutboxStatus, FailureEscalation trait
│   ├── repository.rs          # insert, claim_batch, mark_published, delete_published, mark_retry_or_failed, release_stale_locks, cleanup
│   ├── processed.rs           # is_event_processed, mark_event_processed, cleanup_processed_events
│   ├── relay.rs               # OutboxRelay — background task: claim → publish → ack; optional Redis dedup
│   ├── dedup.rs               # Redis dedup cache — is_published(), mark_published() for relay duplicate prevention
│   └── metrics.rs             # collect_outbox_metrics → OutboxMetrics
├── cache/mod.rs               # init_redis() → Result, init_optional_redis(), RedisCache (generic JSON cache)
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
| `server` | `ServiceBuilder::new(name).http_port_env().with_db().with_redis().with_consumers(factory).with_outbox_relay().with_job_runner(factory).run(build_app)` — composable bootstrap via `InfraDep` enum (Postgres, Redis, Kafka); `.with_consumers()` spawns Kafka consumers; `.with_outbox_relay()` spawns outbox relay; `.with_job_runner()` spawns persistent job runner; `Infra { db, redis, kafka }` with `require_db()`, `require_redis()`, `require_kafka()` accessors; `ConsumerRegistration { group_id, topics, handler }` |
| `db` | `init_db() → Result`, `PgPool`, `PgExec<'e>` (reads), `PgConnection` (writes) |
| `db::transaction_support` | `with_transaction(pool, closure)`, `with_nested_transaction(tx, closure)`, `TxContext` — logs rollback errors; `From<AppError> for TxError` enables `?` propagation in closures |
| `db::pagination_support` | `keyset_paginate(params, alias, qb)`, `get_cursors(params, rows)`, `PaginationParams` (impl `Default`: limit=20, forward), `PaginationRes<T>`, `PaginatedResponse<T>` (Serialize+Deserialize), `HasId` trait |
| `auth::jwt` | `jwt::generate_access_token(&config, ...)`, `jwt::validate_access_token(&config, token)`, `CurrentUser { id, role }` (axum extractor), `AccessTokenClaims` (axum extractor) |
| `auth::middleware` | `AuthMiddleware::new(auth_config, user_lookup)` (identity), `::new_claims_based(auth_config)` (other services, ADR-008) |
| `auth::guards` | `require_access(user, owner_id)`, `require_admin(user)` |
| `auth::role` | `Role` — Buyer, Seller, Admin |
| `config` | `DbConfig`, `AuthConfig`, `RedisConfig` (`.new()` / `.try_new()`), `KafkaConfig` (`.new()` / `.from_brokers()`), `RelayConfig` (`.from_env()` / `Default`), `JobRunnerConfig` (`.from_env()` / `Default`), `ConsumerConfig` (`.new(group_id, topics)` / `.from_env()`) |
| `jobs` | `JobName` (validated `{ns}.{name}`), `JobHandler` trait, `JobRegistry` (`.register()`, `.register_recurring()`), `enqueue_job()`, `claim_batch()`, `mark_completed()`, `mark_retry_or_failed()`, `mark_dead_lettered()`, `seed_recurring_job()`, `reset_recurring()`, `release_stale_locks()`, `cleanup_completed()`, `JobRunner`, `JobSchedule`, `RecurringJobDefinition`, `RecurringFailurePolicy`, `compute_next_run_at()`, `count_missed_ticks()` |
| `errors` | `AppError` — NotFound, Forbidden, Unauthorized, AlreadyExists, InternalServerError, BadRequest |
| `rules` | `Rule<A>` (`Check`, `All`, `Any`, `Not`), `RuleResult<A>` (`Pass`, `Fail`, `AllOf`, `AnyOf`, `Negated`). Interpreters: `evaluate()`, `evaluate_detailed()`, `describe()`, `collect_checks()`, `collect_failures()`, `failure_messages()`. ADR-012. |
| `new_types::money` | `Price` (non-negative Decimal), `Currency` (3-letter ISO 4217), `Money` (Price+Currency pair, `same_currency()`) |
| `responses` | `ok(data)`, `success(status, msg)`, `created(msg)` |
| `circuit_breaker` | `CircuitBreaker::new(config)`, `.check()` → `Result<(), CircuitOpenError>`, `.record_success()`, `.record_retryable_failure()`, `.status()` → `BreakerStatus`; `CircuitBreakerConfig { window_size, failure_threshold, cooldown }`; thread-safe via `Mutex` |
| `distributed_lock` | `DistributedLock::new(conn)`, `.acquire(key, ttl)`, `.acquire_with_retry(key, ttl, config)` → `Result<LockGuard, LockError>`; `LockGuard::release()` via Lua atomic check-and-delete; `RetryConfig { max_attempts, retry_delay }` |
| `email` | `EmailService` trait, `MockEmailService` |
| `events` | `EventEnvelope` (`.payload_uuid(field)` helper), `EventMetadata`, `EventType`, `AggregateType`, `SourceService`, `EventPublisher` trait, `MockEventPublisher`, `KafkaEventPublisher`, `KafkaAdmin`, `TopicSpec`, `KafkaEventConsumer`, `EventHandler` trait, `HandlerError`, `ConsumerConfig`, `MockEventHandler`, `KafkaHealthChecker`, `KafkaHealth`, `KafkaHealthStatus`, `ConsumerMetricsCollector`, `ConsumerMetrics` |
| `outbox` | `OutboxInsert::from_envelope(topic, envelope)`, `insert_outbox_event()`, `claim_batch()`, `mark_published()`, `mark_retry_or_failed()`, `RelayConfig`, `FailureEscalation` trait, `OutboxRelay` (`.with_redis()` for dedup), `RelayHeartbeat` |
| `outbox::dedup` | `is_published(conn, event_id)`, `mark_published(conn, event_id)` — Redis-based duplicate publish prevention |
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
| `test_utils::events::make_envelope(event_type, aggregate_id, extra)` | Test envelope builder; auto-derives source_service and aggregate_type from event_type |

## Transactional Outbox (events + outbox modules)

Services publish domain events via a local `outbox_events` table (same transaction as business operation). A background relay publishes to Kafka.

Full lifecycle: [docs/OUTBOX_LIFECYCLE.md](../docs/OUTBOX_LIFECYCLE.md) | Migration template: `.plan/outbox-migration-template.sql`

```rust
// Producer (inside a transaction):
let insert = OutboxInsert::from_envelope("order.events", &envelope)
    .with_metadata(capture_trace_context());
insert_outbox_event(&mut *tx, &insert).await?;

// Consumer (idempotent processing):
if is_event_processed(&pool, event_id).await? { return Ok(()); }
// ... handle event ...
mark_event_processed(&pool, event_id, "OrderCreated", "catalog").await?;
```

## Kafka Infrastructure

Full guide: [docs/KAFKA_GUIDE.md](../docs/KAFKA_GUIDE.md) (topic management, publishing, testing, consumer implementation)

## Persistent Jobs (jobs module)

Services run background jobs via a local `persistent_jobs` table. Runner claims, executes, and marks jobs with retry, timeout, dead-letter, and recurring support.

Full lifecycle: [docs/PERSISTENT_JOB_LIFECYCLE.md](../docs/PERSISTENT_JOB_LIFECYCLE.md) | Migration template: `.plan/persistent-jobs-migration-template.sql`

```rust
// Register handler:
impl JobHandler for MyJob {
    fn job_type(&self) -> &str { "payment.disburse" }  // valid JobName: {ns}.{name}
    async fn execute(&self, payload: &Value, pool: &PgPool) -> Result<(), JobError> { Ok(()) }
}

// ServiceBuilder integration:
ServiceBuilder::new("payment")
    .with_db("PAYMENT_DB_URL")
    .with_job_runner(|infra| {
        let mut registry = JobRegistry::new();
        registry.register(Arc::new(MyJob::new(infra.require_db().clone())));
        registry
    })
    .run(|infra| app(app_state)).await

// Enqueue (inside transaction or standalone):
let name = JobName::new("payment.disburse").unwrap();
enqueue_job(&pool, &name, &json!({"order_id": id}), None).await?;
```

## Key Traits to Implement Per Service

| Trait | Module | When |
|-------|--------|------|
| `HasId` | `db::pagination_support` | Any paginated entity — `fn id(&self) -> Uuid` |
| `GetCurrentUser` | `auth::middleware` | Identity service only (others use claims-based) |
| `EmailService` | `email` | If service sends emails (use `MockEmailService` for dev) |
| `EventPublisher` | `events::publisher` | Publish events to Kafka (use `MockEventPublisher` for tests) |
| `EventHandler` | `events::consumer` | Consume events from Kafka (use `MockEventHandler` for tests) |
| `JobHandler` | `jobs::registry` | Background job processing (implement `job_type()` + `execute()`) |
| `FailureEscalation` | `outbox::types` | Handle permanently failed outbox events (default: `LogFailureEscalation`) |
