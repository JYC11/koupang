# Persistent Job Lifecycle Reference

## Happy Path (One-Shot Job)

```
Service Code                          JobRunner (background loops)          persistent_jobs row
────────────                          ────────────────────────────          ───────────────────
1. enqueue_job(pool, &name,
     &payload, config)
   → INSERT persistent_jobs
     (status=pending,
      next_run_at=NOW())
   ├─ trigger fires ──────────────►  pg_notify('persistent_jobs', id)
                                      ↓ PgListener wakes up
                                     2. claim_batch(pool, limit, "runner-1")
                                        CTE:
                                        ├─ WHERE status='pending'
                                        ├─ AND next_run_at <= NOW()
                                        ├─ ORDER BY next_run_at ASC
                                        └─ FOR UPDATE SKIP LOCKED
                                        → status='running'
                                        → locked_by="runner-1"
                                        → locked_at=NOW()
                                        → attempts += 1

                                     3. tokio::time::timeout(timeout_secs,
                                          handler.execute(&payload, &pool))
                                        → Ok(Ok(())) — handler succeeded

                                     4. mark_completed(pool, id)
                                        → status='completed'              status=completed
                                        → locked_by=NULL                  locked_by=NULL
                                        → locked_at=NULL

                                     (cleanup loop eventually deletes
                                      completed rows older than 7 days)
```

Key guarantees:
- LISTEN/NOTIFY wakes the runner within milliseconds of enqueue
- `FOR UPDATE SKIP LOCKED` prevents double-claiming across concurrent runners
- `InFlightGuard` with `Drop` ensures in-flight counter is decremented even on panic
- Handler idempotency is a documented requirement (at-least-once delivery)

## Unhappy Path 1: Transient Error (Retry with Backoff)

```
JobRunner                                          persistent_jobs row
─────────                                          ───────────────────
1. claim_batch → locked_by="runner-1"              status=running, attempts=1

2. handler.execute() → Err(JobError::Transient)

3. mark_retry_or_failed(id, "db timeout", max_retries=5)
   ├─ attempts (1) < max_retries (5)
   ├─ status → 'pending'                           status=pending
   ├─ next_run_at = NOW() + 2^1 = 2s               next_run_at=+2s
   ├─ last_error = "db timeout"
   └─ locked_by = NULL                              (unlocked, backoff active)

   ... 2 seconds pass ...

4. claim_batch → picks it up again                  status=running, attempts=2

5. handler.execute() → Err(Transient) again

6. mark_retry_or_failed(id, "db timeout", 5)
   ├─ attempts (2) < max_retries (5)
   ├─ next_run_at = NOW() + 2^2 = 4s               next_run_at=+4s
   └─ locked_by = NULL

   ... eventually succeeds on attempt 3 ...

7. mark_completed(id)                               status=completed
```

### Backoff schedule (capped at 2^10)

| Attempt | Delay    | Cumulative |
|---------|----------|------------|
| 1       | 2s       | 2s         |
| 2       | 4s       | 6s         |
| 3       | 8s       | 14s        |
| 4       | 16s      | 30s        |
| 5       | 32s      | ~1min      |
| 6       | 64s      | ~2min      |
| 7       | 128s     | ~4min      |
| 8       | 256s     | ~8min      |
| 9       | 512s     | ~17min     |
| 10+     | 1024s    | capped     |

## Unhappy Path 2: Retries Exhausted (Terminal Failure)

```
JobRunner                                          persistent_jobs row
─────────                                          ───────────────────
(after max_retries=5 attempts, all transient failures)

1. claim_batch → locked_by="runner-1"              attempts=5

2. handler.execute() → Err(Transient)

3. mark_retry_or_failed(id, "still failing", 5)
   ├─ attempts (5) >= max_retries (5)
   ├─ status → 'failed'                            status=FAILED (terminal)
   ├─ last_error = "still failing"
   └─ locked_by = NULL

   Job is now TERMINAL — never claimed again.
   Operator can retry via retry_dead_lettered() (Phase 5).
```

## Unhappy Path 3: Permanent Error (Dead Letter)

```
JobRunner                                          persistent_jobs row
─────────                                          ───────────────────
1. claim_batch → locked_by="runner-1"              status=running, attempts=1

2. handler.execute() → Err(JobError::Permanent("corrupt payload"))

3. mark_dead_lettered(id, "corrupt payload")
   ├─ status → 'dead_lettered'                     status=DEAD_LETTERED (terminal)
   ├─ last_error = "corrupt payload"
   └─ locked_by = NULL

   No retry — immediate terminal state.
   Requires code or data investigation before retry.
```

## Unhappy Path 4: Handler Timeout

```
JobRunner                                          persistent_jobs row
─────────                                          ───────────────────
1. claim_batch → locked_by="runner-1"              status=running, attempts=1

2. tokio::time::timeout(timeout_secs,
     handler.execute()) → Err(Elapsed)
   ├─ handler's Future is cancelled (Drop)
   └─ any open transactions roll back via Drop

3. Job left as 'running' with stale lock            status=running (stale)

   ... stale_lock_check_interval passes ...

4. Stale lock recovery loop:
   release_stale_locks(stale_lock_timeout)
   ├─ locked_at < NOW() - timeout
   ├─ status → 'pending'                           status=pending
   └─ locked_by = NULL                              (retry on next claim)
```

## Unhappy Path 5: Runner Crashes (Stale Lock)

```
Runner-1                         Recovery loop                Runner-2
────────                         ─────────────                ────────
1. claim_batch
   locked_by="runner-1"
   locked_at=10:00:00

2. CRASH (OOM, panic, kill)

                                 3. release_stale_locks(300s)
                                    at 10:05:05:
                                    locked_at (10:00:00) < NOW() - 300s
                                    → status='pending'
                                    → locked_by=NULL
                                    → locked_at=NULL

                                                              4. claim_batch
                                                                 → picks up the job
                                                                 locked_by="runner-2"
                                                              5. handler.execute() → Ok
                                                              6. mark_completed
```

## Recurring Job Lifecycle (Slot Model)

Recurring jobs use a single row that cycles `pending → running → pending` indefinitely.

```
Runner startup                     Claim & execute loop
──────────────                     ────────────────────
1. seed_recurring_jobs()
   INSERT ... ON CONFLICT
   (job_type, dedup_key)
   DO NOTHING
   → Creates slot if not exists

                                   2. claim_batch → status='running'
                                                    attempts=1

                                   3. handler.execute() → Ok

                                   4. mark_completed(id)
                                      → status='completed'

                                   5. Check: job.schedule IS NOT NULL
                                      → compute_next_run_at(schedule)
                                      → reset_recurring(id, next_run_at)
                                         status → 'pending'             ◄── back to pending
                                         attempts → 0
                                         next_run_at → next tick
                                         locked_by → NULL

                                   ... next_run_at arrives ...

                                   6. claim_batch picks it up again
                                      → cycle repeats indefinitely
```

### Failure Policies for Recurring Jobs

```
RecurringFailurePolicy::Die (default)
─────────────────────────────────────
After max_retries exhausted:
  → status='failed' (slot is dead)
  → Operator must manually retry
  → Safer: don't silently continue if fundamentally broken

RecurringFailurePolicy::ResetToNext
───────────────────────────────────
After max_retries exhausted:
  → reset_recurring(id, next_tick)
  → status='pending', next_run_at=next tick
  → Warning logged, scheduling continues
  → Use for: health checks, metrics collection
```

### Missed Tick Handling

```
Scenario: job took 3 hours, schedule is every hour

Last completed at:  10:00
Now:                13:00
Missed ticks:       11:00, 12:00 (2 missed)

Behavior:
  → Warning logged: "missed_ticks=2"
  → next_run_at computed from NOW(), not from old schedule
  → Next run at 14:00 (skip missed, don't catch up)
```

## Concurrent Runner Safety

```
Runner-1                               Runner-2
────────                               ────────
claim_batch(5, "runner-1")             claim_batch(5, "runner-2")
        │                                      │
        └──── both execute CTE simultaneously ─┘
              FOR UPDATE SKIP LOCKED

              Postgres guarantees:
              - One runner wins the row lock
              - Other runner SKIPs that row
              - Zero duplicate claims

Result: each job executed exactly once across all runners.
```

## Shutdown Drain

```
1. shutdown.cancel()
   ├─ All 3 loops exit (biased select! prioritizes shutdown)
   └─ tokio::join! returns

2. Drain phase:
   ├─ while in_flight > 0:
   │    wait on drain_notify (InFlightGuard signals on Drop)
   └─ Safety timeout = stale_lock_timeout
      → If exceeded, warn and proceed (stale lock recovery handles orphans)

3. "Job runner shut down gracefully"
```

## Runner Loops

```
┌──────────────────────────────────────────────────────┐
│ JobRunner background tasks (via JobRunnerConfig)      │
├──────────────────────────────────────────────────────┤
│ 1. Claim & execute (PgListener + poll_interval=1s)   │
│    claim_batch → timeout → execute → mark result     │
│    InFlightGuard tracks concurrent jobs              │
│                                                      │
│ 2. Stale lock recovery (every 30s, timeout=300s)     │
│    release_stale_locks() → free crashed/timed-out    │
│                                                      │
│ 3. Cleanup (every 3600s, max_age=7 days)             │
│    cleanup_completed() → batched DELETE of 1000      │
│    drain loop until 0 rows, shutdown check between   │
└──────────────────────────────────────────────────────┘
```

## State Machine Summary

Enforced at the DB level by the `enforce_job_status_transition` trigger.
Invalid transitions raise a `check_violation` exception.

```
                 enqueue_job
                     │
                     ▼
                ┌─────────┐   release_stale_locks()
         ┌─────│ PENDING  │◄──────────────┐
         │     └────┬─────┘               │
         │          │                     │
   cancel_job   claim_batch          mark_retry_or_failed
   (admin)      (lock acquired)      (attempts < max)
         │          │                     │
         ▼          ▼                     │
   ┌──────────┐ ┌─────────┐              │
   │CANCELLED │ │ RUNNING  │──────────────┘
   └──────────┘ └────┬─────┘
                     │
            ┌────────┼──────────┐
            │        │          │
       handler ok  transient  permanent
            │     exhausted    error
            ▼        │          │
     ┌──────────┐    ▼          ▼
     │COMPLETED │ ┌──────┐ ┌────────────┐
     └────┬─────┘ │FAILED│ │DEAD_LETTERED│
          │       └──────┘ └────────────┘
          │         │  ▲          │
     ┌────┴────┐    │  │          │
     │recurring│    └──┘          │
     │schedule │  retry from      │
     │IS NOT   │  terminal        │
     │NULL     │  (Phase 5)       │
     └────┬────┘                  │
          │                       │
          ▼                       │
    reset_recurring          retry from terminal
    (→ PENDING with          (Phase 5)
     next_run_at)
```
