# Design: Persistent Job System

## Current State

The `shared` crate has two background processing systems:

- **OutboxRelay** -- three concurrent loops (relay, stale lock recovery, cleanup), `FOR UPDATE SKIP LOCKED` claiming, `CancellationToken` shutdown, `PgListener` LISTEN/NOTIFY wakeup
- **KafkaEventConsumer** -- message processing with retry/backoff, DLQ, idempotency, transient vs permanent error distinction

Both share a lifecycle pattern: `Arc<Self>` + `tokio::spawn` per loop + `CancellationToken` + `tokio::join!`. Two transaction patterns coexist: `TxContext`/`with_transaction()` for business code (caller controls boundary), raw `pool.begin()` for infrastructure code (framework controls boundary).

No general-purpose job system exists. Ad-hoc `tokio::spawn` loops would be unreliable (no persistence, no retry, lost on crash). Bolting job behavior onto events is the wrong abstraction (events are facts, not commands).

## Desired End State

A `shared::jobs` module any service can use via `ServiceBuilder::with_job_runner()`. Supports one-shot, delayed, and recurring (cron) jobs with at-least-once execution, configurable retry with exponential backoff, dead-letter handling, and two-phase job execution with idempotent handlers.

## Module Structure

`shared/src/jobs/` mirrors the outbox layout:

| File | Public types | Visibility pattern |
|------|-------------|-------------------|
| `mod.rs` | Re-export hub | Same as `outbox/mod.rs` |
| `types.rs` | `JobStatus`, `Job`, `JobInsert`, `JobError`, `JobSchedule`, `DedupStrategy`, `RecurringJobDefinition`, `RecurringFailurePolicy` | `pub(crate)` module, items re-exported |
| `repository.rs` | `enqueue_job()`, `seed_recurring_job()`, `claim_batch()`, `mark_completed()`, `mark_failed()`, `mark_dead_lettered()`, `reset_recurring()`, `release_stale_locks()`, `cleanup_completed()`, `cancel_job()`, `retry_dead_lettered()` | private module, `pub use repository::*` |
| `runner.rs` | `JobRunner` | private module, re-exported |
| `registry.rs` | `JobRegistry` | private module, re-exported |
| `config.rs` | `JobRunnerConfig`, `JobConfig` | re-exported via `pub use` |

## Resolved Decisions

### D1: State Machine -- 5 States

```
pending ──> running ──> completed (one-shot: deleted by cleanup)
   ^           |           |
   |           |           └──> pending (recurring: reset-in-place)
   |           |
   |           ├──> pending (retry with backoff, for transient errors)
   |           |
   |           ├──> failed (terminal: retries exhausted via transient errors)
   |           |
   |           └──> dead_lettered (terminal: permanent error)
   |
   └── cancelled (admin action)
```

States: `pending`, `running`, `completed`, `failed`, `dead_lettered`, `cancelled`.

- `pending`: ready to be claimed (or waiting for `next_run_at`)
- `running`: claimed by a runner, in-flight
- `completed`: handler returned Ok (one-shot jobs stay here until cleanup deletes them; recurring jobs are immediately reset to `pending` with new `next_run_at`)
- `failed`: terminal -- retries exhausted after transient errors. Operator may retry after fixing environment issues.
- `dead_lettered`: terminal -- handler returned permanent error. Requires code or data investigation.
- `cancelled`: manually cancelled via admin API

No separate `scheduled`/`retryable` states (Oban has these). `pending` covers both -- distinguished by whether `next_run_at` is in the past or future.

Allowed transitions enforced by DB trigger:
- `pending -> running` (claim)
- `running -> completed | failed | dead_lettered` (execution result)
- `running -> pending` (retry with backoff for transient errors)
- `completed -> pending` (recurring job reset-in-place)
- `pending -> cancelled` (admin action)
- `dead_lettered -> pending` (admin retry)
- `failed -> pending` (admin retry)
- Self-transitions allowed

**Note:** Add DB-level constraints to enforce different allowed transitions for recurring vs one-shot jobs (e.g., `completed -> pending` only valid when `schedule IS NOT NULL`).

### D2: No Archive Table -- Delete Completed, Retain Terminal

The cleanup loop deletes completed jobs older than `cleanup_max_age`. `failed` and `dead_lettered` rows are retained indefinitely until an operator retries or manually cleans them up.

No archive table for MVP. Execution history comes from structured logs.

Partial indexes on the main table:
- `(next_run_at) WHERE status = 'pending'` -- claim query
- `(job_type, dedup_key) WHERE status IN ('pending', 'running')` -- dedup (UNIQUE)
- `(locked_at) WHERE status = 'running'` -- stale lock detection
- `(status, updated_at) WHERE status = 'completed'` -- cleanup

### D3: Claiming via FOR UPDATE SKIP LOCKED

Same pattern as outbox `claim_batch()`. Simpler than outbox because jobs have no per-aggregate ordering requirement:

```sql
WITH claimable AS (
    SELECT id FROM persistent_jobs
    WHERE status = 'pending' AND next_run_at <= NOW()
    ORDER BY next_run_at ASC
    FOR UPDATE SKIP LOCKED
    LIMIT $1
)
UPDATE persistent_jobs
SET status = 'running', locked_by = $2, locked_at = NOW(), attempts = attempts + 1
FROM claimable
WHERE persistent_jobs.id = claimable.id
RETURNING persistent_jobs.*
```

The `LIMIT $1` is dynamically computed as `max_concurrent_jobs - in_flight_count` to prevent claiming more than the runner can execute.

### D4: JobHandler Trait -- Two-Phase Execution

```rust
#[async_trait]
pub trait JobHandler: Send + Sync {
    fn job_type(&self) -> &str;
    async fn execute(&self, payload: &Value, pool: &PgPool) -> Result<(), JobError>;
}
```

**Two-phase execution model:**
1. **Phase 1 (claim):** Runner claims the job in a short transaction (`pending -> running`).
2. **Phase 2 (execute):** Handler receives `&PgPool` and manages its own transactions via `with_transaction()`. No wrapping runner transaction.
3. **Phase 3 (mark):** Runner marks the job completed/failed in a short transaction.

This avoids holding a long-running transaction open for the entire job duration (which would hold connection pool slots, prevent VACUUM, and hold row locks). The tradeoff: handler's business work and the completion mark are not atomic.

**Idempotency is a documented requirement on all job handlers.** At-least-once delivery means a job may be re-executed if the runner crashes between handler success and completion marking. This is the same contract Kafka event handlers already follow in this codebase.

### D5: JobError -- Transient vs Permanent

```rust
pub enum JobError {
    Transient(String),
    Permanent(String),
}
```

- `Transient` -> retry with backoff (or `failed` if retries exhausted)
- `Permanent` -> `dead_lettered` immediately

Matches `HandlerError` semantics from the Kafka consumer.

### D6: Runner Architecture -- Three Loops

```
JobRunner::run(self, shutdown: CancellationToken)
  ├── claim_and_execute_loop(Arc<Self>, shutdown)  -- main work
  ├── stale_lock_recovery_loop(Arc<Self>, shutdown) -- free crashed jobs
  └── cleanup_loop(Arc<Self>, shutdown)             -- delete old completed jobs
```

Each loop uses `biased` `tokio::select!` with shutdown as highest priority. Same pattern as `OutboxRelay::run()`.

**Claim & execute loop:**
1. Wait for PgListener NOTIFY or poll_interval (whichever first)
2. Compute available capacity: `max_concurrent_jobs - in_flight_count`
3. `claim_batch()` with `LIMIT = available_capacity` (skip if 0)
4. For each claimed job, spawn a tokio task, increment in-flight counter
5. In each task: call `handler.execute(payload, pool)` wrapped in `tokio::time::timeout(timeout_seconds)` -> mark completed/failed/dead_lettered -> decrement in-flight counter
6. If recurring and completed: reset row to `pending` with new `next_run_at`
7. If recurring and retries exhausted: check `RecurringFailurePolicy` -- `ResetToNext` reschedules, `Die` goes to `failed`

**No semaphore needed.** The dynamic `LIMIT` on the claim query naturally bounds concurrency. An `AtomicUsize` counter tracks in-flight jobs.

### D7: Recurring Jobs -- Reset-in-Place with Configurable Failure Policy

Recurring jobs use a **slot model**: a single row cycles `pending -> running -> pending` forever. No new rows are inserted on completion.

**At startup:** Runner calls `seed_recurring_job()` for each `RecurringJobDefinition` in the registry. Uses `INSERT ... ON CONFLICT DO NOTHING` on the `(job_type, dedup_key)` unique index -- safe for multi-instance startup.

**After successful execution:** The runner resets the row to `pending` with `next_run_at` computed from `now()` (next cron tick or `now + interval`). The `attempts` counter is reset to 0.

**Missed tick handling:** If the job overran its schedule, `next_run_at` is computed from `now()`, skipping missed ticks. A warning is logged with the number of missed ticks.

**After failed execution (retries exhausted):** Configurable via `RecurringFailurePolicy`:
- **`Die`** (default): Row goes to `failed`. Slot is dead until operator retries. Safer -- don't silently continue if something is fundamentally broken.
- **`ResetToNext`**: Row is reset to `pending` with `next_run_at` at the next tick. A warning is logged. Use for jobs where continued scheduling matters more than individual run success (e.g., health checks).

```rust
pub enum RecurringFailurePolicy {
    Die,          // default: go to `failed`, require manual retry
    ResetToNext,  // reset to pending with next cron/interval tick
}

pub struct RecurringJobDefinition {
    pub job_type: String,
    pub schedule: JobSchedule,        // Cron(Schedule) | Interval(Duration)
    pub payload: Value,
    pub dedup_key: String,
    pub config: Option<JobConfig>,
    pub failure_policy: RecurringFailurePolicy,  // default: Die
}
```

### D8: Deduplication -- Three Strategies (One-Shot Jobs Only)

Per-job `DedupStrategy` (only relevant for one-shot jobs; recurring jobs use the slot model):
- **Skip** (default): unique partial index on `(job_type, dedup_key) WHERE status IN ('pending', 'running')` rejects duplicates. Application catches the constraint violation and returns Ok.
- **Enqueue**: no dedup_key set, multiple instances can coexist.
- **Replace**: cancel existing pending row, insert new one. Running jobs are not replaced.

### D9: Retry with Exponential Backoff

Same formula as outbox: `next_run_at = now + 2^min(attempts, 10) seconds`.

After `max_retries` attempts exhausted -> `failed` (terminal).

Permanent errors -> `dead_lettered` immediately (no retry).

The `mark_failed()` repository function computes backoff in SQL (same as `mark_retry_or_failed()` in outbox). When retries are not exhausted, the job goes `running -> pending` (with new `next_run_at`). When exhausted, it goes `running -> failed`.

### D10: Timeout -- Tokio Only

`tokio::time::timeout(timeout_seconds)` wraps `handler.execute()`. If the timeout fires, the handler's in-progress work is cancelled (any open transactions are rolled back via `Drop`). The job stays in `running` with a stale lock; the recovery loop frees it and resets to `pending` for retry.

No DB-level `statement_timeout` -- with the two-phase model (handler manages own transactions), `SET LOCAL` is not applicable. Tokio timeout handles both DB and non-DB work.

### D11: Cron Expression Format

Direct pass-through to `cron` crate. 6-field format required: `sec min hour dom month dow`. No 5-field auto-conversion. The `cron` crate is timezone-agnostic; we always iterate with `Utc`.

### D12: Enqueue API -- Two Functions

```rust
// One-shot jobs: called by service code, works inside with_transaction()
pub async fn enqueue_job(
    executor: impl PgExec<'_>,
    job_type: &str,
    payload: &Value,
    config: Option<&JobConfig>,
) -> Result<Uuid, AppError>

// Recurring job slot creation: called by runner at startup
pub async fn seed_recurring_job(
    executor: impl PgExec<'_>,
    definition: &RecurringJobDefinition,
) -> Result<Option<Uuid>, AppError>  // None if slot already exists
```

`enqueue_job()` accepts any `PgExec` -- works with both `&PgPool` (standalone) and `&mut PgConnection` (inside `with_transaction`). Same pattern as `insert_outbox_event()`.

`seed_recurring_job()` uses `INSERT ... ON CONFLICT DO NOTHING` on the `(job_type, dedup_key)` unique index. Returns `None` if the slot already exists. Safe for concurrent multi-instance startup.

### D13: Configuration

`JobRunnerConfig` follows `RelayConfig` pattern:

| Field | Type | Default | Env Var |
|-------|------|---------|---------|
| `instance_id` | String | UUID v7 | `JOB_RUNNER_INSTANCE_ID` |
| `max_concurrent_jobs` | usize | 5 | `JOB_RUNNER_MAX_CONCURRENT_JOBS` |
| `poll_interval` | Duration | 1s | `JOB_RUNNER_POLL_INTERVAL_MS` |
| `stale_lock_check_interval` | Duration | 30s | `JOB_RUNNER_STALE_LOCK_CHECK_INTERVAL_SECS` |
| `stale_lock_timeout` | Duration | 300s | `JOB_RUNNER_STALE_LOCK_TIMEOUT_SECS` |
| `cleanup_interval` | Duration | 3600s | `JOB_RUNNER_CLEANUP_INTERVAL_SECS` |
| `cleanup_max_age` | Duration | 7 days | `JOB_RUNNER_CLEANUP_MAX_AGE_SECS` |
| `default_max_retries` | u32 | 5 | `JOB_RUNNER_DEFAULT_MAX_RETRIES` |
| `default_timeout_seconds` | u32 | 300 | `JOB_RUNNER_DEFAULT_TIMEOUT_SECS` |

`JobConfig` (per-job overrides, for one-shot jobs):

| Field | Type | Default |
|-------|------|---------|
| `max_retries` | Option<u32> | None (uses global) |
| `timeout_seconds` | Option<u32> | None (uses global) |
| `dedup_strategy` | DedupStrategy | Skip |
| `dedup_key` | Option<String> | None |

### D14: ServiceBuilder Integration

```rust
ServiceBuilder::new("payment")
    .with_db("PAYMENT_DB_URL")
    .with_job_runner(|infra| {
        let mut registry = JobRegistry::new();
        registry.register(Arc::new(DisbursementJob::new(infra.require_db().clone())));
        registry.register_recurring(RecurringJobDefinition { ... });
        registry
    })
    .run(|infra| app(app_state))
    .await
```

`with_job_runner()` stores a registry factory closure (like `with_consumers()`). In `run()`, the runner is spawned as a background task sharing the service's `PgPool` and `CancellationToken`.

### D15: Migration Template

Services copy a `persistent_jobs` migration template (like the outbox template) containing:
- `job_status` enum type
- `persistent_jobs` table
- Partial indexes (D2)
- Status transition trigger (with recurring vs one-shot constraints)
- NOTIFY trigger on INSERT

### D16: LISTEN/NOTIFY

```sql
CREATE FUNCTION persistent_jobs_notify() RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('persistent_jobs', NEW.id::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER persistent_jobs_after_insert
    AFTER INSERT ON persistent_jobs
    FOR EACH ROW EXECUTE FUNCTION persistent_jobs_notify();
```

Runner listens on `'persistent_jobs'` channel. Falls back to poll on listener failure.

## Out of Scope

- Priority queuing (FIFO only)
- Job dependencies / DAGs
- Table partitioning / sharding
- Admin API routes (repository functions exist, routes deferred)
- Prometheus metrics (structured logging only)
- Redis dedup cache (not needed -- unlike outbox, jobs don't publish to external systems)
- Global concurrency limits across runner instances (per-instance only for MVP; `concurrency_group` column can be added later)

## Research Items (Future Session)

1. **Transaction scope in other libraries:** How do Oban, GoodJob, JobRunr, Hangfire handle transaction scope for handler execution? Do they wrap handlers in a framework-managed transaction or let handlers manage their own?
2. **Missed cron tick handling:** How do other job libraries handle overrun scheduling / missed ticks? (Quartz has misfire instructions, Oban has snooze)
3. **Completed job retention:** How do other libraries handle completed job history? Delete immediately (Quartz), retain with pruner (Oban), archive table, or object storage?
4. **Cleanup batching:** Should the cleanup loop batch deletes (like outbox's 1000-at-a-time pattern) to avoid long-running DELETE transactions?
5. **Recurring vs one-shot DB constraints:** Design DB trigger constraints that enforce valid state transitions differently based on whether `schedule IS NOT NULL`
