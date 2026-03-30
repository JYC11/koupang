# Research Questions

## Scope

The `shared` crate provides background infrastructure for the entire microservice ecosystem: an outbox relay system, Kafka consumer framework, service bootstrap (`ServiceBuilder`), transaction management, and test utilities. This research explores how these background processing systems are architected, how they integrate with the service lifecycle, and how they manage concurrency, shutdown, retries, and testing -- particularly the patterns that would apply to any new background task system added to the shared crate.

## Questions

### Component Structure

1. What is the full structure of the `shared/src/outbox/` module? List every file, its public types, and its public functions. How are these re-exported through `mod.rs`?

2. What fields does `ServiceBuilder` hold, and what builder methods does it expose? Specifically, how does `with_consumers()` work -- what closure signature does it accept, and how does it interact with the `InfraDep` system?

3. How is the `Infra` struct defined, and what accessor methods does it provide? What resources are currently available through `Infra` (e.g., `db`, `redis`, `kafka`)?

4. How is the `OutboxRelay` struct defined? What fields does it hold, how is it constructed, and what is the signature of its `run()` method?

5. What configuration types exist under `shared/src/config/`? For each config struct, list its fields, defaults, and how values are loaded (environment variables, hardcoded defaults, etc.). Pay special attention to `RelayConfig` and `ConsumerConfig`.

### Data Flow

6. Trace the full lifecycle of `ServiceBuilder::run()`: from infrastructure initialization through HTTP server startup, consumer spawning, and shutdown signal propagation. How does the `CancellationToken` flow from the builder to background tasks?

7. Trace the `OutboxRelay::run()` method end-to-end: how does it spawn its three concurrent loops (relay, stale lock recovery, cleanup)? How does each loop use `tokio::select!` with the `CancellationToken`? How does `PgListener` (LISTEN/NOTIFY) integrate with the relay loop's wake mechanism?

8. Trace the Kafka consumer's message processing flow in `KafkaEventConsumer`: from message receipt through deserialization, the retry loop with `try_process_once`, transaction management, idempotency checks, and DLQ routing. How does backoff work within the retry loop?

9. How does `with_transaction()` in `shared/src/db/transaction_support.rs` work? What is the `TxContext` type, what lifetime does it carry, and how does `as_executor()` return a `&mut PgConnection`? How does `with_nested_transaction()` differ?

### Patterns & Conventions

10. What is the `EventHandler` trait's exact signature? How does the Kafka consumer call it -- specifically, what database connection does it pass to the handler, and how is that connection part of the consumer's own transaction?

11. How do services register Kafka consumers with `ServiceBuilder`? Show the `ConsumerRegistration` struct fields and a concrete example from any service's `main.rs` (e.g., order or payment).

12. How does `claim_batch()` in the outbox repository use `FOR UPDATE SKIP LOCKED`? What is the two-step CTE strategy (oldest per aggregate, then lock), and how does it enforce per-aggregate ordering?

13. How does the outbox system handle retry and failure escalation? Trace the path from a failed publish attempt through `mark_retry_or_failed()` (including the SQL for exponential backoff computation) to the `FailureEscalation` trait invocation when retries are exhausted.

14. What SQL migration conventions does the project follow? Examine the migration files under `shared/tests/migrations/` (outbox_events, processed_events, status transition trigger) and at least one service migration (e.g., `order/migrations/`). How are migration directories referenced in `ServiceBuilder::with_db()` vs `TestDb::start()`?

### Testing

15. How does `TestDb::start()` work? Describe the shared container pattern (OnceCell, template database, per-test database creation via `CREATE DATABASE ... TEMPLATE`). What is the lifecycle of the container vs individual test databases?

16. How are the outbox relay integration tests structured in `shared/tests/outbox_relay_tests.rs`? What test helpers exist (e.g., `FailingPublisher`, `AlwaysFailPublisher`, `TrackingEscalation`, `start_relay`)? How do tests control timing (fast relay config, manual `next_retry_at` updates)?

17. How are Kafka consumer integration tests structured in `shared/tests/consumer_tests.rs`? How does `TestKafka::start()` work, and how does `TestConsumer` receive and verify messages?

18. What test utilities exist under `shared/src/test_utils/`? List each module (db, redis, kafka, events, auth, http) and its key exports. How does `make_envelope()` in `test_utils/events.rs` work?

### Error Handling & Edge Cases

19. How does the outbox relay handle individual event failures without aborting the entire batch? What happens when `mark_published` fails after a successful Kafka publish? How does the Redis dedup cache (`shared/src/outbox/dedup.rs`) prevent duplicate publishes in this scenario?

20. How does the Kafka consumer distinguish between transient and permanent handler errors (`HandlerError::Transient` vs `HandlerError::Permanent`)? What are the different `ProcessResult` and `AttemptResult` enum variants, and how do they map to offset commit decisions?

21. How does the outbox status transition trigger (`000003_outbox_status_transition_trigger.sql`) enforce valid state machine transitions at the database level? What transitions are allowed and which are rejected?

### Dependencies & Boundaries

22. What external crates does the `shared` crate depend on? List the key dependencies from `shared/Cargo.toml`, especially: `sqlx` (features), `tokio`/`tokio-util` (features), `rdkafka`, `redis`, `chrono`, `uuid`, and any optional/feature-gated dependencies (e.g., `testcontainers-modules` behind `test-utils`).

23. How is the `shared` crate's feature flag system structured? What does the `test-utils` feature gate, and how do service crates depend on `shared` with this feature enabled for tests?

24. How do the `shared/src/config/mod.rs` helper functions `read_env_or()` and `parse_env_or()` work? How are they used across config structs to provide environment-variable-driven configuration with defaults?

## External Research Targets

- https://github.com/jobrunr/jobrunr — Java job scheduler with RDB backing. Study: table schema, claiming strategy, retry/backoff, recurring job scheduling, state machine transitions
- https://github.com/quartz-scheduler/quartz — Battle-tested Java job scheduler. Study: JDBC job store schema, clustering/lock strategy, cron trigger implementation, misfire handling
- [agent-sourced] Other well-known RDB-backed job queues (e.g., Oban for Elixir/Postgres, GoodJob for Ruby/Postgres, Hangfire for .NET/SQL Server) — schema design patterns, claiming strategies, deduplication approaches
- [agent-sourced] `cron` crate (crates.io/docs.rs) — how to parse and evaluate cron expressions in Rust, 7-field syntax support, timezone handling
