# Structure: Persistent Job System

## Key Types & Interfaces

```rust
// --- types.rs ---
pub enum JobStatus { Pending, Running, Completed, Failed, DeadLettered, Cancelled }
pub enum JobError { Transient(String), Permanent(String) }
pub enum JobSchedule { Cron(cron::Schedule), Interval(Duration) }
pub enum DedupStrategy { Skip, Enqueue, Replace }
pub enum RecurringFailurePolicy { Die, ResetToNext }

pub struct Job { /* all columns — includes schedule: Option<String> for trigger (R5) */ }
pub struct JobInsert { pub job_type: String, pub payload: Value, pub config: Option<JobConfig> }
pub struct JobConfig { pub max_retries: Option<u32>, pub timeout_seconds: Option<u32>,
                       pub dedup_strategy: DedupStrategy, pub dedup_key: Option<String> }
pub struct RecurringJobDefinition { pub job_type: String, pub schedule: JobSchedule,
    pub payload: Value, pub dedup_key: String, pub config: Option<JobConfig>,
    pub failure_policy: RecurringFailurePolicy }

// --- registry.rs ---
#[async_trait]
pub trait JobHandler: Send + Sync {
    fn job_type(&self) -> &str;
    async fn execute(&self, payload: &Value, pool: &PgPool) -> Result<(), JobError>;
}
pub struct JobRegistry { handlers: HashMap<String, Arc<dyn JobHandler>>,
                         recurring: Vec<RecurringJobDefinition> }

// --- config.rs ---
pub struct JobRunnerConfig { /* D13 fields, from_env() */ }

// --- runner.rs ---
pub struct JobRunner { pool: PgPool, registry: JobRegistry, config: JobRunnerConfig,
                       in_flight: AtomicUsize, drain_notify: Notify }

/// RAII guard: decrements in_flight + signals drain_notify on Drop (panic-safe).
struct InFlightGuard<'a> { counter: &'a AtomicUsize, notify: &'a Notify }

impl JobRunner {
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken);
    // run() = tokio::join!(3 loops) then drain (wait for in_flight → 0 via drain_notify)
}

// --- repository.rs (free functions) ---
pub async fn enqueue_job(executor: impl PgExec<'_>, ...) -> Result<Uuid, AppError>;
pub async fn seed_recurring_job(executor: impl PgExec<'_>, ...) -> Result<Option<Uuid>, AppError>;
pub async fn claim_batch(pool: &PgPool, ...) -> Result<Vec<Job>, AppError>;
pub async fn mark_completed(pool: &PgPool, id: Uuid) -> Result<(), AppError>;
pub async fn mark_retry_or_failed(pool: &PgPool, id: Uuid, err: &str, max_retries: u32) -> Result<(), AppError>;
pub async fn mark_dead_lettered(pool: &PgPool, id: Uuid, err: &str) -> Result<(), AppError>;
pub async fn reset_recurring(pool: &PgPool, id: Uuid, next_run_at: DateTime<Utc>) -> Result<(), AppError>;
pub async fn release_stale_locks(pool: &PgPool, ...) -> Result<u64, AppError>;
pub async fn cleanup_completed(pool: &PgPool, max_age_secs: i64) -> Result<u64, AppError>;
pub async fn cancel_job(pool: &PgPool, id: Uuid) -> Result<(), AppError>;
pub async fn retry_dead_lettered(pool: &PgPool, id: Uuid) -> Result<(), AppError>;
```

---

## Phases

### Phase 1: Tracer Bullet — One-Shot Happy Path
**Covers:** Schema, one-shot types, enqueue, claim, execute, mark completed — end-to-end for a one-shot job that succeeds on first attempt.
**Introduces:**
- Migration: `persistent_jobs` table (includes `schedule` column for R5 trigger), `job_status` CHECK, partial indexes (D2), status transition trigger (R5), NOTIFY trigger (D16)
- `shared/src/jobs/` module: `mod.rs`, `types.rs` (one-shot types + `Job` struct with all columns), `config.rs`, `registry.rs`
- `repository.rs`: `enqueue_job()`, `claim_batch()`, `mark_completed()`
- `runner.rs`: `JobRunner` with `claim_and_execute_loop` only (NOTIFY + poll fallback), `InFlightGuard` Drop pattern, `drain_notify`
**Test checkpoint:**
- Migration trigger tests (valid + invalid transitions, recurring vs one-shot gate)
- End-to-end: enqueue → runner claims → handler executes → row marked completed
- NOTIFY wakeup: enqueue triggers immediate claim (not waiting for poll_interval)
- Concurrent claim: two runners competing for same jobs → SKIP LOCKED guarantees no double-claim
- Handler panic: verify InFlightGuard decrements counter on panic (spawn task that panics, assert in_flight returns to 0)

### Phase 2: Error Handling — Retry, Timeout, Dead Letter
**Covers:** Transient error retry with exponential backoff, permanent error dead-lettering, tokio timeout.
**Introduces:**
- `repository.rs`: `mark_retry_or_failed()` (retry-or-terminal in SQL, same as outbox), `mark_dead_lettered()`
- `runner.rs`: `tokio::time::timeout` wrapping, error dispatch (Transient → retry/failed, Permanent → dead_lettered)
**Depends on:** Phase 1 (runner loop, repository base)
**Test checkpoint:**
- Transient error retries with backoff (verify `next_run_at` progression: 2s, 4s, 8s...)
- Max retries exhausted → `failed` terminal
- Permanent error → `dead_lettered` immediately
- Timeout → stays `running` (stale lock, recovered in Phase 3)

### Phase 3: Recovery & Cleanup Loops
**Covers:** Stale lock recovery loop, cleanup loop, graceful shutdown drain — completing the 3-loop runner from D6.
**Introduces:**
- `repository.rs`: `release_stale_locks()`, `cleanup_completed()` (batched `LIMIT 1000`)
- `runner.rs`: `stale_lock_recovery_loop`, `cleanup_loop` (drain pattern from outbox R4)
- `runner.rs`: `JobRunner::run()` now spawns all 3 loops with `tokio::join!`, then awaits `drain_notify` until `in_flight == 0`
**Depends on:** Phase 2 (timeout leaves stale locks that this phase recovers)
**Test checkpoint:**
- Stale lock detection: manually set `locked_at` in the past → verify `release_stale_locks` resets to `pending`
- Cleanup: insert old completed rows → verify batched deletion, verify shutdown check between batches
- Shutdown drain: cancel token while job running → verify run() waits for in-flight to complete before returning

### Phase 4: Recurring Jobs
**Covers:** Cron and interval scheduling, seed at startup, reset-in-place, failure policies, missed tick warning.
**Introduces:**
- `types.rs`: `JobSchedule`, `RecurringJobDefinition`, `RecurringFailurePolicy` (new types — Phase 1 `Job` struct already has `schedule` column)
- `repository.rs`: `seed_recurring_job()`, `reset_recurring()`
- `registry.rs`: `register_recurring()`, `recurring_definitions()` accessor
- `runner.rs`: seed on startup, `completed → pending` reset with next `next_run_at`, `RecurringFailurePolicy` dispatch, missed tick count + warning log
- Depends on `cron` crate for schedule parsing
**Depends on:** Phase 3 (full runner lifecycle)
**Test checkpoint:**
- Seed idempotency (`ON CONFLICT DO NOTHING` — two calls, one row)
- Cron next-tick computation from `now()`, interval next-tick (`now + duration`)
- Reset-in-place: verify same row ID, `attempts` reset to 0
- `Die` policy: retries exhausted → `failed`
- `ResetToNext` policy: retries exhausted → `pending` with next tick
- Missed tick warning logged when overrun

### Phase 5: Deduplication & Admin Operations *(deferrable — Phase 6 can proceed without this)*
**Covers:** Skip/Enqueue/Replace dedup strategies for one-shot jobs, cancel and retry-from-terminal admin operations.
**Introduces:**
- `repository.rs`: dedup logic in `enqueue_job()` (Skip: catch unique violation, Replace: cancel existing + insert), `cancel_job()`, `retry_dead_lettered()`
- Uses partial unique index `(job_type, dedup_key) WHERE status IN ('pending', 'running')` from Phase 1 migration
**Depends on:** Phase 4 (full feature set before admin operations)
**Test checkpoint:**
- Skip: duplicate enqueue returns Ok without inserting
- Enqueue: two jobs with same type coexist
- Replace: pending job cancelled, new one inserted; running job not replaced
- Cancel from `pending`
- Retry from `dead_lettered` and `failed` → `pending`

### Phase 6: ServiceBuilder Integration
**Covers:** Wire `with_job_runner()` into `ServiceBuilder`, create migration template.
**Introduces:**
- `server.rs`: `with_job_runner(factory)` method (stores registry factory closure), `spawn_job_runner()` in `run()`/`run_with_grpc()`
- `.plan/persistent-jobs-migration-template.sql` — canonical template for services to copy
- Update `shared/CLAUDE.md` with jobs module documentation
**Depends on:** Phase 4 (core jobs module complete; Phase 5 is deferrable)
**Test checkpoint:**
- `ServiceBuilder::new("svc").with_job_runner(...)` compiles and stores factory
- Migration template applies cleanly against TestDb
- Verify `JobRunnerConfig::from_env()` reads env vars with correct defaults
