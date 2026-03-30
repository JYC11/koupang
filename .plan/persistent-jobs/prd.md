# PRD: Persistent Job System

## Problem Statement

The koupang ecommerce platform has several use cases that require reliable background work execution — seller disbursements, payment authorization expiry checks, ledger reconciliation, and bulk product imports. Currently, the only background processing mechanisms are the outbox relay (event publishing) and Kafka consumers (event handling), both purpose-built for event-driven workflows. There is no general-purpose job system for scheduling, executing, and monitoring arbitrary background tasks with reliability guarantees.

Without a persistent job system, future features will either require ad-hoc `tokio::spawn` loops (unreliable — no persistence, no retry, lost on crash) or bolting job-like behavior onto the event system (wrong abstraction — events represent facts, not commands).

## Solution

Build a Postgres-backed persistent job system in `shared` that any service can use via `ServiceBuilder`. The system supports one-shot, delayed, and recurring (cron) jobs with at-least-once execution guarantees, configurable retry with exponential backoff, dead-letter handling, and transactional job execution via the existing `with_transaction` / `TxContext` pattern.

The system mirrors proven patterns already in the codebase (outbox relay's `FOR UPDATE SKIP LOCKED` claiming, `CancellationToken` graceful shutdown, trait-based handlers like `EventHandler`) while providing a clean, general-purpose API for scheduling any kind of background work.

## User Stories

1. As a service developer, I want to enqueue a one-shot job inside a business transaction, so that the job is only created if the transaction commits successfully
2. As a service developer, I want to define a job handler by implementing a trait, so that the pattern is consistent with how I already implement `EventHandler` for Kafka consumers
3. As a service developer, I want to register recurring jobs with cron expressions, so that periodic tasks like disbursements run on a predictable schedule
4. As a service developer, I want to register recurring jobs with fixed intervals, so that simple polling tasks don't need cron expression complexity
5. As a service developer, I want job handlers to receive a `PgConnection` inside a transaction, so that business work and job completion are atomic
6. As a service developer, I want failed jobs to retry with exponential backoff, so that transient failures (DB timeouts, network blips) self-heal without manual intervention
7. As a service developer, I want permanently failed jobs to land in a dead-letter state, so that operators can investigate and manually retry them
8. As a service developer, I want to configure max retries, timeout, and deduplication per job type, so that different jobs get appropriate reliability settings
9. As a service developer, I want global defaults for retries, timeout, and concurrency, so that I don't have to configure every job individually
10. As a service developer, I want per-job configuration to override global defaults, so that critical jobs can have stricter settings than the baseline
11. As a service developer, I want deduplication to prevent duplicate job instances (skip, enqueue, or replace), so that recurring schedulers don't create duplicate runs when the previous one is still in progress
12. As a service developer, I want the job runner to start via `ServiceBuilder::with_job_runner()`, so that it integrates with the existing service bootstrap lifecycle
13. As a service developer, I want the job runner to shut down gracefully via `CancellationToken`, so that in-flight jobs complete before the process exits
14. As a service developer, I want the runner to recover stale locks from crashed instances, so that jobs don't get stuck permanently
15. As a service developer, I want completed jobs to be cleaned up periodically, so that the jobs table doesn't grow unbounded
16. As a service developer, I want to run the job runner as a standalone binary, so that I can scale job processing independently from the HTTP server
17. As a service developer, I want to run the job runner embedded in my service process, so that simple deployments don't need a separate binary
18. As an operator, I want API endpoints to retry a dead-lettered job, so that I can recover from failures without direct DB access
19. As an operator, I want API endpoints to cancel a scheduled job, so that I can stop a job that should no longer run
20. As an operator, I want API endpoints to view job status, so that I can monitor the system without querying the database directly
21. As a service developer, I want jobs to have a configurable execution timeout, so that hung jobs are killed and retried rather than blocking the runner forever
22. As a service developer, I want the framework to ship with an example job, so that I have a working reference for implementing my own jobs

## Implementation Decisions

### Storage: Postgres

Jobs are stored in a `persistent_jobs` table in each service's database. This aligns with the existing pattern (outbox uses per-service Postgres tables) and enables transactional job enqueuing — a job can be inserted in the same transaction as business data.

### Job Table Schema

The `persistent_jobs` table includes: id (UUID v7), job_type (text), payload (JSONB), status (enum: pending/running/completed/failed/dead_lettered/cancelled), schedule (JSONB — null for one-shot, cron expression or interval for recurring), attempts (int), max_retries (int), next_run_at (timestamptz), locked_by (text, nullable), locked_at (timestamptz, nullable), timeout_seconds (int, nullable), dedup_key (text, nullable), created_at, updated_at. A unique partial index on `(job_type, dedup_key) WHERE status IN ('pending', 'running')` enforces deduplication for the Skip and Replace strategies.

### Claiming: FOR UPDATE SKIP LOCKED

The runner claims jobs using the same `FOR UPDATE SKIP LOCKED` pattern proven in the outbox relay. This prevents multiple runner instances from claiming the same job without blocking each other. Jobs are claimed in FIFO order by `next_run_at`.

### Handler Trait: `JobHandler`

```
trait JobHandler: Send + Sync {
    fn job_type(&self) -> &str;
    async fn execute(&self, payload: &Value, tx: &mut PgConnection) -> Result<(), JobError>;
}
```

The handler receives a mutable `PgConnection` inside the runner's transaction. The runner marks the job as completed in the same transaction. If the handler returns `JobError::transient(msg)`, the job is retried with backoff. If `JobError::permanent(msg)`, it's dead-lettered immediately.

This mirrors the existing `EventHandler` trait pattern.

### Error Type: `JobError`

Two variants: `Transient(String)` and `Permanent(String)`. Transient errors trigger retry with exponential backoff (2^min(n, 10) seconds, same formula as outbox). Permanent errors skip retry and move the job directly to dead-letter status.

### Registry: `JobRegistry`

A `HashMap<String, Arc<dyn JobHandler>>` that maps job type strings to handler implementations. Registered at service startup, passed to the runner. Unknown job types are dead-lettered with an error message.

### Runner: Three Concurrent Loops

Mirrors the outbox relay architecture:

1. **Claim & execute loop** — polls for ready jobs (`next_run_at <= now AND status = 'pending'`), claims a batch, dispatches to handlers. Uses a `tokio::Semaphore` to bound concurrency to `max_concurrent_jobs`. Woken by PgListener NOTIFY or falls back to `poll_interval`.
2. **Stale lock recovery loop** — periodically frees jobs locked longer than `stale_lock_timeout` by crashed runners. Resets status to pending for retry.
3. **Scheduler loop** — for recurring jobs that just completed, computes the next cron tick or interval and inserts the next run. Checks deduplication before inserting.

### Concurrency: Semaphore-Bounded

The runner uses `tokio::sync::Semaphore` with `max_concurrent_jobs` permits. Each claimed job acquires a permit before execution. This is globally configurable via `JobRunnerConfig`.

### Deduplication

Three strategies, configurable per job:

- **Skip** — if a job with the same `(job_type, dedup_key)` is already pending or running, the new enqueue is silently dropped
- **Enqueue** — no deduplication; multiple instances can run concurrently
- **Replace** — cancel the existing pending job and enqueue the new one (running jobs are not replaced)

Global default is `Skip`. Per-job `JobConfig` can override.

### Timeout

Jobs have a configurable execution timeout. The runner wraps `handler.execute()` in `tokio::time::timeout()`. If the timeout fires, the job is marked as failed with a transient error (eligible for retry). Global default is configurable; per-job override via `JobConfig`.

### Retry with Exponential Backoff

Same formula as the outbox relay: `next_run_at = now + 2^min(attempts, 10) seconds`. After `max_retries` attempts, the job moves to dead-letter status. Global default max retries is configurable; per-job override via `JobConfig`.

### ServiceBuilder Integration

A new method `ServiceBuilder::with_job_runner(config, registry_factory)` spawns the job runner as a background task alongside the HTTP server and Kafka consumers. The runner shares the service's `PgPool` and `CancellationToken`.

### Standalone Runner Binary

The runner can also be instantiated directly without `ServiceBuilder`, for deployments that want a dedicated job worker process. It only needs a `PgPool`, `JobRunnerConfig`, and `JobRegistry`.

### LISTEN/NOTIFY for Near-Instant Dispatch

Like the outbox relay, the jobs table has a trigger that fires `pg_notify('persistent_jobs', id)` on INSERT. The runner's claim loop listens on this channel for near-instant job pickup, with `poll_interval` as fallback.

### Migration Template

A migration template (similar to the outbox migration template) will be provided for services to copy into their migrations directory. It creates the `persistent_jobs` table, status transition trigger, and NOTIFY trigger.

### Cron Parsing

The `cron` crate will be used for parsing and evaluating cron expressions. Standard 7-field cron syntax.

### Example Job

A `HealthCheckJob` that logs a message with the current timestamp. Demonstrates both one-shot (enqueue once, runs immediately) and recurring (cron schedule) usage patterns.

### Admin API Endpoints (Deferred)

Manual intervention endpoints (retry dead-lettered, cancel scheduled, list jobs) are designed as part of this PRD but implementation is deferred to a follow-up. The job repository functions (`retry_dead_lettered()`, `cancel_job()`, `list_jobs()`) will be built as part of the core framework to enable future admin routes.

## Testing Decisions

### What makes a good test

Tests should verify external behavior through the public API, not internal implementation details. A test should break only when the system's behavior changes, not when the internal structure is refactored. Tests should use real Postgres via testcontainers (ADR-004), not mocks.

### Modules to test

1. **Repository** (`jobs/repository.rs`) — unit-style integration tests against real Postgres. Test: enqueue, claim_batch (verify SKIP LOCKED behavior), complete, fail, dead_letter, stale lock recovery, deduplication (all three strategies), cleanup, schedule computation. This is the highest-value test target because it validates the core state machine.

2. **Runner** (`jobs/runner.rs`) — integration tests with real Postgres. Test: end-to-end job execution (enqueue → claim → execute → complete), retry on transient error, dead-letter on permanent error, dead-letter after max retries exhausted, timeout behavior, graceful shutdown (in-flight job completes), stale lock recovery, recurring job re-scheduling, concurrency limiting (multiple jobs, bounded semaphore).

3. **Registry** (`jobs/registry.rs`) — simple unit tests. Test: register handler, dispatch to correct handler, unknown job type returns error.

4. **Config** (`jobs/config.rs`) — unit tests for per-job override merging with global defaults.

### Prior art

- `shared/tests/outbox_relay_tests.rs` — integration tests for the outbox relay (claim/publish/ack lifecycle, stale lock recovery). Closest structural analog.
- `shared/tests/consumer_tests.rs` — integration tests for Kafka consumer (retry, DLQ, idempotency).
- Both use `TestDb::start()` for real Postgres containers.

## Out of Scope

- **Job dashboard UI** — no frontend; admin visibility will come via API endpoints in a follow-up
- **Admin API endpoint implementation** — repository functions will exist, but route mounting is deferred to a backoffice/admin service discussion
- **Priority queuing** — FIFO only for MVP; priority levels can be added later by adding a `priority` column and adjusting the claim query's ORDER BY
- **Distributed rate limiting** — no cross-instance rate limiting; concurrency is per-runner-instance
- **Job dependencies / DAGs** — no job chaining or dependency graphs; each job is independent
- **Event-triggered jobs** — while the framework supports one-shot jobs that can be enqueued from a Kafka handler, there is no built-in "trigger job on event" wiring; that composition is left to the service developer
- **Redis-backed queues** — Postgres only; Redis is not used for job storage
- **Metrics / observability integration** — structured logging yes, but Prometheus metrics for job counts/durations are deferred

## Further Notes

- The persistent job system intentionally mirrors the outbox relay's architecture (three loops, FOR UPDATE SKIP LOCKED, CancellationToken, LISTEN/NOTIFY). This reduces cognitive load — developers who understand the outbox already understand the job runner.
- The `JobHandler` trait receiving `&mut PgConnection` inside a transaction is the key differentiator from external libraries. This enables atomic business-work-plus-job-completion, which is critical for correctness in a system that already uses `with_transaction` pervasively.
- For recurring jobs, the scheduler loop inserts the *next* run as a new row rather than updating the existing row. This preserves execution history (completed rows stay in the table until cleanup) and avoids race conditions between the executor completing a job and the scheduler re-enqueuing it.
- The dedup_key is separate from the job's UUID. For recurring jobs, the dedup_key is typically the job_type itself (e.g., "seller_disbursement"). For one-shot jobs triggered by business events, it might be an aggregate_id (e.g., order UUID) to prevent duplicate processing.
