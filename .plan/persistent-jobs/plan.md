# Plan: Persistent Job System

> Design: [design.md](design.md)
> Structure: [structure.md](structure.md)
> Research: [research-jobs.md](research-jobs.md)

---

## Phase 1: Tracer Bullet — One-Shot Happy Path

### Changes

- **`shared/Cargo.toml`**: Add `cron = "0.15"` dependency (needed from Phase 4, but add now to avoid mid-stream Cargo.toml edits)
- **`shared/src/lib.rs`**: Add `pub mod jobs;`
- **`shared/src/jobs/mod.rs`**: Re-export hub following outbox pattern:
  ```rust
  mod config;
  mod registry;
  mod repository;
  mod runner;
  pub(crate) mod types;

  pub use config::{JobConfig, JobRunnerConfig};
  pub use registry::{JobHandler, JobRegistry};
  pub use repository::*;
  pub use runner::JobRunner;
  pub use types::{Job, JobError, JobInsert, JobStatus};
  // Phase 4 will add: JobSchedule, RecurringJobDefinition, RecurringFailurePolicy, DedupStrategy
  ```
- **`shared/src/jobs/types.rs`**: Define one-shot types:
  - `JobStatus` enum (Pending, Running, Completed, Failed, DeadLettered, Cancelled) — `sqlx::Type` with `rename_all = "snake_case"`, `Display` impl
  - `Job` struct — `sqlx::FromRow`, all columns including `schedule: Option<String>` (nullable, used by trigger R5)
  - `JobInsert` struct — `job_type`, `payload: Value`, `config: Option<JobConfig>`
  - `JobError` enum — `Transient(String)`, `Permanent(String)`
  - `DedupStrategy` enum — `Skip`, `Enqueue`, `Replace` (with `Default` = Skip)
- **`shared/src/jobs/config.rs`**: Define `JobRunnerConfig` and `JobConfig`:
  - `JobRunnerConfig` with `from_env()` and `Default` — same pattern as `RelayConfig` (uses `parse_env_or`, `read_env_or` from config module)
  - `JobConfig` — per-job overrides: `max_retries`, `timeout_seconds`, `dedup_strategy`, `dedup_key`
- **`shared/src/jobs/registry.rs`**: Define `JobHandler` trait and `JobRegistry`:
  - `JobHandler` trait: `fn job_type(&self) -> &str`, `async fn execute(&self, payload: &Value, pool: &PgPool) -> Result<(), JobError>`
  - `JobRegistry`: `HashMap<String, Arc<dyn JobHandler>>`, `register()`, `get()`, `recurring: Vec<RecurringJobDefinition>` (empty until Phase 4)
- **`shared/src/jobs/repository.rs`**: Implement core repo functions:
  - `enqueue_job(executor: impl PgExec<'_>, job_type, payload, config) -> Result<Uuid, AppError>` — INSERT with defaults, catches unique constraint violation for Skip dedup (returns Ok with existing ID)
  - `claim_batch(pool: &PgPool, limit: i32, instance_id: &str) -> Result<Vec<Job>, AppError>` — CTE with `FOR UPDATE SKIP LOCKED` from D3
  - `mark_completed(executor: impl PgExecutor<'_>, id: Uuid) -> Result<(), AppError>` — UPDATE status='completed', clear lock
- **`shared/src/jobs/runner.rs`**: Implement `JobRunner` with claim_and_execute_loop:
  - Fields: `pool`, `registry`, `config`, `in_flight: AtomicUsize`, `drain_notify: Notify`
  - `InFlightGuard` struct with Drop (decrements counter + notifies `drain_notify`)
  - `run(self: Arc<Self>, shutdown: CancellationToken)` — spawns claim_and_execute_loop only (other loops added in Phase 3)
  - `claim_and_execute_loop`: PgListener on `'persistent_jobs'` channel, `biased` select! with shutdown, compute dynamic limit, claim_batch, spawn task per job with InFlightGuard
  - `connect_listener` helper — same pattern as outbox relay (fallback to poll on failure)
- **`shared/tests/migrations/`**: Add migration file for persistent_jobs:
  - `000004_persistent_jobs.sql` (or next number): table, CHECK constraint, partial indexes (D2), status transition trigger (R5 from design doc), NOTIFY trigger (D16)
- **`shared/tests/job_migration_test.rs`**: Migration and trigger tests

### Tests

- [ ] **trigger_function_exists**: Verify `job_enforce_status_transition` and `persistent_jobs_after_insert` triggers exist
- [ ] **status_check_constraint**: INSERT with invalid status rejected
- [ ] **valid_transitions**: `pending→running`, `running→completed`, `running→failed`, `running→dead_lettered`, `running→pending` (retry), `pending→cancelled`, `dead_lettered→pending`, `failed→pending`, self-transitions
- [ ] **invalid_transitions**: `completed→running`, `failed→running`, `cancelled→running`, etc. — verify `check_violation` error
- [ ] **recurring_gate**: `completed→pending` succeeds when `schedule IS NOT NULL`, fails when `schedule IS NULL`
- [ ] **enqueue_and_claim_happy_path**: enqueue job → claim_batch(limit=1) → verify returned Job has status=running, locked_by set, attempts=1
- [ ] **claim_skip_locked**: enqueue 2 jobs, claim_batch(limit=1) twice in separate transactions → verify no overlap (different job IDs)
- [ ] **claim_respects_next_run_at**: enqueue job with `next_run_at` in future → claim_batch returns empty
- [ ] **mark_completed**: claim job → mark_completed → verify status=completed, locked_by=NULL
- [ ] **runner_end_to_end**: start JobRunner with TestHandler that records invocations → enqueue job → assert handler called with correct payload within 2s
- [ ] **notify_wakeup**: start runner with long poll_interval (60s) → enqueue job → assert handler called within 1s (NOTIFY triggers immediate claim, not waiting for poll)
- [ ] **concurrent_claim**: start 2 runners against same pool → enqueue 1 job → assert exactly 1 handler invocation
- [ ] **handler_panic_safety**: register handler that panics → enqueue job → verify in_flight returns to 0 after task completes

### Acceptance Criteria

- [ ] `make test SERVICE=shared` passes — all existing tests + new job tests green
- [ ] enqueue → claim → execute → completed lifecycle works end-to-end in a test

---

## Phase 2: Error Handling — Retry, Timeout, Dead Letter

### Changes

- **`shared/src/jobs/repository.rs`**: Add error-handling repo functions:
  - `mark_retry_or_failed(executor, id, error, max_retries) -> Result<(), AppError>` — single UPDATE with CASE: if `attempts >= max_retries` then `status='failed'` (terminal), else `status='pending'` with `next_run_at = NOW() + 2^min(attempts, 10)` — same SQL pattern as outbox `mark_retry_or_failed()`
  - `mark_dead_lettered(executor, id, error) -> Result<(), AppError>` — UPDATE status='dead_lettered', set last_error, clear lock
- **`shared/src/jobs/runner.rs`**: Extend spawned task logic:
  - Wrap `handler.execute()` in `tokio::time::timeout(timeout_duration)`
  - On `Ok(Ok(()))` → `mark_completed()`
  - On `Ok(Err(JobError::Transient(msg)))` → `mark_retry_or_failed()` with effective max_retries (job-level override or runner default)
  - On `Ok(Err(JobError::Permanent(msg)))` → `mark_dead_lettered()`
  - On `Err(_)` (timeout) → log warning, leave as `running` (stale lock recovery handles it in Phase 3)

### Tests

- [ ] **transient_retry_backoff**: handler returns Transient 3 times → verify `next_run_at` increases exponentially (2s, 4s, 8s), status stays `pending`, attempts increments
- [ ] **transient_exhausted_becomes_failed**: set max_retries=2, handler returns Transient → after 2 attempts, status=`failed`, `last_error` set
- [ ] **permanent_dead_lettered**: handler returns Permanent → status=`dead_lettered` immediately, attempts=1, `last_error` set
- [ ] **timeout_leaves_running**: handler sleeps longer than timeout → job stays `running`, `locked_by` still set (stale lock recovery will handle)
- [ ] **runner_retry_end_to_end**: handler fails transiently once then succeeds → verify job eventually completed (runner re-claims after backoff)

### Acceptance Criteria

- [ ] All error paths (transient, permanent, timeout) result in correct status transitions
- [ ] Exponential backoff formula matches outbox: `2^min(attempts, 10)` seconds

---

## Phase 3: Recovery & Cleanup Loops

### Changes

- **`shared/src/jobs/repository.rs`**: Add recovery/cleanup repo functions:
  - `release_stale_locks(executor, stale_timeout_secs) -> Result<u64, AppError>` — UPDATE jobs WHERE `status='running'` AND `locked_at < NOW() - timeout` → set `status='pending'`, clear lock. (Note: outbox stale lock recovery targets `status='pending'` because it doesn't change status on claim; jobs target `status='running'` because claim transitions `pending→running`)
  - `cleanup_completed(executor, max_age_secs) -> Result<u64, AppError>` — DELETE with subquery LIMIT 1000 (same batched pattern as outbox `cleanup_published`)
- **`shared/src/jobs/runner.rs`**: Add remaining loops and shutdown drain:
  - `stale_lock_recovery_loop(runner, shutdown)` — periodic check using `biased` select! with shutdown. Calls `release_stale_locks()`.
  - `cleanup_loop(runner, shutdown)` — periodic drain loop. Inner loop calls `cleanup_completed()` until 0 rows, with `shutdown.is_cancelled()` check between batches.
  - Update `run()`: spawn all 3 loops with `tokio::join!`, then **drain phase** — loop on `drain_notify.notified()` until `in_flight.load() == 0` (with a timeout equal to `stale_lock_timeout` as safety net)

### Tests

- [ ] **stale_lock_recovery**: manually insert job with `status='running'`, `locked_at` = 10 minutes ago → call `release_stale_locks(timeout=300)` → verify status=`pending`, locked_by=NULL, locked_at=NULL
- [ ] **stale_lock_skips_recent**: insert running job with `locked_at` = 1 second ago → `release_stale_locks(timeout=300)` → verify still `running` (not stale)
- [ ] **cleanup_batched_deletion**: insert 2500 completed jobs with old `updated_at` → call `cleanup_completed` in drain loop → verify all deleted, verify it took 3 batches (1000+1000+500)
- [ ] **cleanup_preserves_failed**: insert failed + dead_lettered jobs with old `updated_at` → cleanup → verify NOT deleted
- [ ] **cleanup_preserves_recent_completed**: insert completed job with recent `updated_at` → cleanup → verify NOT deleted
- [ ] **shutdown_drain**: start runner, enqueue job with handler that sleeps 2s → cancel shutdown token after 100ms → verify `run()` waits for handler to finish (doesn't return immediately)

### Acceptance Criteria

- [ ] All 3 loops run concurrently and exit on shutdown
- [ ] `run()` does not return until all in-flight jobs complete (drain phase works)
- [ ] Cleanup deletes in batches of 1000 with shutdown checks between batches

---

## Phase 4: Recurring Jobs

### Changes

- **`shared/src/jobs/types.rs`**: Add recurring types:
  - `JobSchedule` enum — `Cron(cron::Schedule)`, `Interval(Duration)`
  - `RecurringJobDefinition` struct — `job_type`, `schedule`, `payload`, `dedup_key`, `config`, `failure_policy`
  - `RecurringFailurePolicy` enum — `Die` (default), `ResetToNext`
- **`shared/src/jobs/mod.rs`**: Add re-exports for `JobSchedule`, `RecurringJobDefinition`, `RecurringFailurePolicy`
- **`shared/src/jobs/registry.rs`**: Add `register_recurring()` and `recurring_definitions()`:
  - `register_recurring(def: RecurringJobDefinition)` — stores definition AND registers the handler (handler must already be registered via `register()`)
  - `recurring_definitions() -> &[RecurringJobDefinition]`
- **`shared/src/jobs/repository.rs`**: Add recurring repo functions:
  - `seed_recurring_job(executor, definition) -> Result<Option<Uuid>, AppError>` — INSERT with `ON CONFLICT (job_type, dedup_key) WHERE status IN ('pending', 'running') DO NOTHING`, returns None if slot exists
  - `reset_recurring(executor, id, next_run_at) -> Result<(), AppError>` — UPDATE: `status='pending'`, `next_run_at=$2`, `attempts=0`, clear lock, clear `last_error`
- **`shared/src/jobs/runner.rs`**: Extend for recurring:
  - `seed_recurring_jobs()` — called at start of `run()`, iterates `registry.recurring_definitions()`, calls `seed_recurring_job()` for each
  - After `mark_completed()`: check if `job.schedule IS NOT NULL` → if yes, compute next `next_run_at` from schedule, call `reset_recurring()`. Log warning with missed tick count if overrun.
  - After retry exhaustion for recurring job: check `failure_policy` → `Die` does nothing (already `failed`), `ResetToNext` calls `reset_recurring()` with next tick instead
  - Helper: `compute_next_run_at(schedule: &JobSchedule) -> DateTime<Utc>` — for Cron: iterate schedule from `Utc::now()`, take first; for Interval: `Utc::now() + duration`
  - Helper: `count_missed_ticks(schedule: &JobSchedule, last_run: DateTime<Utc>) -> usize` — count ticks between `last_run` and `now`

### Tests

- [ ] **seed_recurring_idempotent**: call `seed_recurring_job()` twice with same definition → first returns `Some(id)`, second returns `None`, verify 1 row in DB
- [ ] **seed_concurrent**: two concurrent seed calls (separate pool connections) → no error, exactly 1 row
- [ ] **cron_next_tick**: compute_next_run_at with cron `"0 0 * * * *"` (every hour at :00) → verify returns next hour boundary
- [ ] **interval_next_tick**: compute_next_run_at with Interval(60s) → verify returns ~now + 60s
- [ ] **reset_in_place**: seed recurring → claim → mark_completed → reset_recurring → verify same row ID, status=`pending`, attempts=0, next_run_at updated
- [ ] **recurring_end_to_end**: register recurring handler (interval 1s) → start runner → assert handler called at least twice within 5s
- [ ] **die_policy**: recurring handler returns Transient until retries exhausted → verify status=`failed` (slot dead)
- [ ] **reset_to_next_policy**: recurring handler with `ResetToNext` exhausts retries → verify status=`pending` with future `next_run_at`
- [ ] **missed_tick_warning**: seed recurring cron job, manually set `next_run_at` to 3 ticks ago → claim+complete+reset → verify warning logged with missed_ticks=3 (use `tracing_subscriber` test layer or assert on `next_run_at` being computed from now, not from old schedule)

### Acceptance Criteria

- [ ] Recurring job slot cycles `pending→running→pending` indefinitely
- [ ] `ON CONFLICT DO NOTHING` prevents duplicate slots on multi-instance startup
- [ ] `RecurringFailurePolicy::Die` and `ResetToNext` both work correctly

---

## Phase 5: Deduplication & Admin Operations *(deferrable)*

### Changes

- **`shared/src/jobs/repository.rs`**: Extend `enqueue_job()` for Replace strategy + add admin ops:
  - `enqueue_job()`: add `DedupStrategy` dispatch:
    - `Skip` (existing): catch unique constraint violation → return Ok
    - `Enqueue`: no dedup_key set (`NULL`), unique index doesn't apply → always inserts
    - `Replace`: query for existing pending row with same `(job_type, dedup_key)` → cancel it → insert new row. Running jobs are NOT cancelled.
  - `cancel_job(executor, id) -> Result<(), AppError>` — UPDATE `status='cancelled'` WHERE `status='pending'`
  - `retry_dead_lettered(executor, id) -> Result<(), AppError>` — UPDATE `status='pending'`, `attempts=0`, `next_run_at=NOW()`, clear lock/error WHERE `status IN ('dead_lettered', 'failed')`

### Tests

- [ ] **skip_dedup**: enqueue with same `(job_type, dedup_key)` twice → first succeeds, second returns Ok (no error, no second row)
- [ ] **skip_dedup_allows_after_completion**: enqueue → complete → enqueue same key → succeeds (completed job not in unique index)
- [ ] **enqueue_no_dedup**: enqueue twice with `Enqueue` strategy (no dedup_key) → 2 rows exist
- [ ] **replace_cancels_pending**: enqueue with `Replace` → enqueue again with `Replace` → first job `cancelled`, second `pending`
- [ ] **replace_skips_running**: enqueue → claim (now running) → enqueue with `Replace` → running job untouched, new pending job created
- [ ] **cancel_pending**: enqueue → cancel → status=`cancelled`
- [ ] **cancel_non_pending_fails**: claim job (now running) → cancel → error (trigger rejects `running→cancelled`)
- [ ] **retry_dead_lettered**: mark job as dead_lettered → retry_dead_lettered → status=`pending`, attempts=0
- [ ] **retry_failed**: mark job as failed → retry (same function) → status=`pending`, attempts=0

### Acceptance Criteria

- [ ] All 3 dedup strategies work correctly
- [ ] Admin retry resets job to clean `pending` state

---

## Phase 6: ServiceBuilder Integration

### Changes

- **`shared/src/jobs/config.rs`**: Ensure `JobRunnerConfig` is re-exported from `shared::jobs`
- **`shared/src/server.rs`**: Add `with_job_runner()` and `spawn_job_runner()`:
  - Add `job_runner_factory: Option<JobRunnerFactory>` field to `ServiceBuilder` (type alias: `Box<dyn FnOnce(&Infra) -> JobRegistry>`)
  - `with_job_runner(factory)` — stores factory closure. Does NOT add any deps (job runner uses the service's existing PgPool).
  - `spawn_job_runner(name, factory, infra, shutdown)` — calls factory to get registry, creates `JobRunner::new(pool, registry, config)`, spawns `runner.run(shutdown)` as background task
  - Call `spawn_job_runner` in both `run()` and `run_with_grpc()` alongside `spawn_consumers` and `spawn_relay`
- **`.plan/persistent-jobs-migration-template.sql`**: Create canonical migration template following outbox template format:
  - Header comments with usage instructions
  - `persistent_jobs` table DDL
  - Partial indexes (D2)
  - Status transition trigger with recurring gate (R5)
  - NOTIFY trigger (D16)
- **`shared/CLAUDE.md`**: Add `jobs` module documentation section (types, repository functions, runner, config, ServiceBuilder integration example)

### Tests

- [ ] **with_job_runner_compiles**: `ServiceBuilder::new("svc").with_db("DB_URL").with_job_runner(|infra| { JobRegistry::new() })` — verify it compiles and stores factory (unit test, no actual infra)
- [ ] **job_runner_config_defaults**: `JobRunnerConfig::default()` returns expected values from D13
- [ ] **job_runner_config_from_env**: set env vars → verify `JobRunnerConfig::from_env()` reads them (same pattern as relay_config test)
- [ ] **migration_template_applies**: apply `.plan/persistent-jobs-migration-template.sql` against TestDb → verify table, triggers, indexes exist

### Acceptance Criteria

- [ ] A service can add `with_job_runner()` to its `ServiceBuilder` chain
- [ ] Migration template works as a copy-paste for any service
- [ ] `shared/CLAUDE.md` updated with jobs module documentation
- [ ] `make test SERVICE=shared` passes — all 618+ existing tests + all new job tests green
