# Persistent Jobs: Research Findings

Research completed 2026-04-02 to resolve the 5 open items from `design.md`.

---

## R1: Transaction Scope in Other Libraries

**Question:** Do libraries wrap handlers in a framework-managed transaction or let handlers manage their own?

### Findings by Library

| Library | Claim | Handler Tx | Mark Complete | Single Tx? | Crash Recovery |
|---------|-------|-----------|---------------|------------|----------------|
| **Oban** (Elixir) | `FOR UPDATE SKIP LOCKED` in short tx | None — handler gets Job struct | Standalone UPDATE with retry | No — 3 phases | Rescue plugin detects stale `executing` rows |
| **GoodJob** (Ruby) | `pg_try_advisory_lock` (session-level) | None — handler gets ActiveJob context | `transaction { save! }` | No — advisory lock spans phases | Lock auto-released on connection drop |
| **Que** (Ruby) | Session-level advisory lock | Handler encouraged to wrap own work + `destroy` in same tx | Handler calls `destroy` in its own tx | **Yes, if handler follows pattern** | Advisory lock released on crash |
| **JobRunr** (Java) | Optimistic concurrency (version check) | None — handler uses own framework tx | `storageProvider.save(job)` | No — 3 phases | Orphaned job detection via heartbeat |
| **Hangfire** (.NET) | Queue-based fetch (DELETE/UPDATE) | None — handler gets PerformContext | `TryChangeState` with retry + TransactionalAcknowledge | Partial (state change + queue removal atomic) | Visibility timeout re-queues |
| **Sidekiq Pro** (Ruby/Redis) | `RPOPLPUSH` to working list | None | `LREM` from working list | No — independent Redis commands | Working-list cleanup on process death |

### Key Takeaways

1. **Industry consensus: separate transactions.** Every major library except Que uses separate transactions for claim, execute, and mark. Que's wrapping-transaction model is the minority approach.

2. **Que's trade-off is explicit:** wrapping business work + job destruction in one tx gives the strongest atomicity, but holds a transaction open for the entire job duration (blocks VACUUM, holds row locks, ties up connection pool slots). This is incompatible with our `with_transaction()` / `TxContext` pattern.

3. **Hangfire's TransactionalAcknowledge** narrows the failure window by making state change + queue removal atomic, but still doesn't wrap the handler.

4. **Every library documents idempotency as a requirement** due to the crash window between handler success and completion marking (except Que where the wrapping tx makes it optional).

### Decision

**D4 validated — no changes.** Our two-phase model (claim in short tx, handler gets `&PgPool`, mark in short tx) matches the approach of Oban, GoodJob, JobRunr, and Hangfire.

---

## R2: Missed Cron Tick Handling

**Question:** How do libraries handle overrun scheduling and missed ticks?

### Findings by Library

| Library | Overrun Behavior | Multiple Missed Ticks | Coalescing | Next Schedule From | Configuration |
|---------|-----------------|----------------------|------------|-------------------|---------------|
| **Quartz** (Java) | `@DisallowConcurrentExecution` annotation | Configurable via misfire instructions | `FIRE_AND_PROCEED` (default) coalesces to one | Cron expression (wall clock) | `misfireThreshold`, per-trigger instruction |
| **Oban** (Elixir) | Overlap allowed by default; use unique jobs | Lost — no catch-up | N/A | Current minute vs cron expression | Unique job constraints |
| **GoodJob** (Ruby) | Overlap allowed; use concurrency controls | Recovered within `cron_graceful_restart_period` | No — each tick enqueued separately | Cron expression or lambda with `last_ran` | `cron_graceful_restart_period` |
| **APScheduler** (Python) | `max_instances=1` (default) treats overrun as misfire | Each checked against `misfire_grace_time` | Explicit `coalesce=True` option | Cron expression (wall clock) | `misfire_grace_time`, `coalesce`, `max_instances` |
| **Celery Beat** (Python) | Overlap allowed; manual locking | Lost — no catch-up | N/A | Cron expression (wall clock) | None |
| **cron** (Unix) | Overlap allowed; use `flock` | Lost — no catch-up | N/A | Current minute vs crontab | None |
| **anacron** (Unix) | N/A (daily resolution) | Coalesced into one per period | Implicit | Last-run date check | Period in days, delay in minutes |

### Key Takeaways

1. **Most common default: skip missed ticks.** Oban, Celery Beat, and cron all simply lose missed ticks — no catch-up, no coalescing. This is the simplest approach.

2. **Quartz's default (FIRE_AND_PROCEED)** coalesces all missed into one immediate fire, then resumes schedule. Most sophisticated built-in handling.

3. **APScheduler is the most configurable** with `misfire_grace_time`, `coalesce`, and `max_instances` options.

4. **Our slot model naturally prevents overlap** — the single row cycles `pending -> running -> pending`, so a new execution cannot start while the previous is running. This is stronger than most defaults.

5. **anacron's model is closest to ours** — one record per job, updated in place, natural coalescing.

6. **Schedule computation from "now" is standard** — every library computes next fire time from the cron expression against wall clock time, not from last scheduled time.

### Decision

**D7 validated — no changes.** Our approach (skip missed ticks, compute next from `now()`, log warning with missed tick count) matches the simplest and most common pattern (Oban, Celery, cron). The slot model provides natural overlap prevention and coalescing without additional configuration.

---

## R3: Completed Job Retention

**Question:** How do libraries handle completed job history — delete immediately, retain with pruner, archive table, or object storage?

### Findings by Library

| Library | Storage | Default Retention | Prune Model | By Age | By Count | Batched | Archive Hook |
|---------|---------|-------------------|-------------|--------|----------|---------|-------------|
| **Oban** | Postgres | 60s (with plugin) | Background loop (leader-only) | Yes | Yes (Pro) | Yes (10k) | Yes (Pro `before_delete`) |
| **GoodJob** | Postgres | 14 days | Inline with scheduler | Yes | No | Yes | No |
| **Quartz** | DB/RAM | Immediate delete | N/A (no history) | N/A | N/A | N/A | Listeners |
| **Hangfire** | SQL Server | 24 hours | Background ExpirationManager | Yes | No | Yes | No |
| **Sidekiq** | Redis | Immediate delete | N/A | N/A | Dead: 10k | N/A | No |
| **Celery** | Various | 24 hours (result) | Backend-specific (TTL/beat) | Yes | No | DB: batched | Backend is archive |
| **BullMQ** | Redis | Keep forever | Lazy (on completion) | Yes | Yes | Atomic w/ completion | No |

### Key Takeaways

1. **No library provides a built-in archive table.** The pattern for long-term history is always "hook before delete, export to external storage."

2. **Three philosophies:** delete immediately (Sidekiq, Quartz), time-based expiration (Hangfire 24h, Celery 24h, GoodJob 14d, Oban 60s), keep forever (BullMQ).

3. **Failed jobs always get longer retention** than completed jobs. Sidekiq: 0 vs 6 months. Hangfire: 24h vs forever.

4. **Batched deletion is universal** for DB-backed systems.

5. **Only Oban Pro has a `before_delete` callback** for cold storage export.

### Decision

**D2 validated — no changes.** Our approach (delete completed via cleanup loop with `cleanup_max_age` default 7 days, retain `failed`/`dead_lettered` indefinitely, no archive table) aligns with industry practice. 7-day default is in the middle of the range (Oban 60s to GoodJob 14 days).

---

## R4: Cleanup Batching

**Question:** Should the cleanup loop batch deletes to avoid long-running DELETE transactions?

### Findings

The outbox cleanup already implements batched deletion:

```sql
DELETE FROM outbox_events
WHERE id IN (
    SELECT id FROM outbox_events
    WHERE status = 'published'
      AND published_at < NOW() - make_interval(secs => $1::float8)
    LIMIT 1000
)
```

Called in a drain loop (`relay.rs:317-349`): repeat until 0 rows deleted, with shutdown checks between batches. Comment: "Drain in batches of 1000 to avoid long transactions with 100K+ rows."

Same pattern in `processed_events` cleanup (`processed.rs:53-73`).

Industry comparison: Oban batches at 10k per cycle. All DB-backed libraries batch.

### Decision

**Replicate outbox pattern exactly.** The job cleanup loop will use `LIMIT 1000` batched deletes in a drain loop with shutdown checks between batches. Same SQL subquery pattern, same loop structure.

---

## R5: Recurring vs One-Shot DB Constraints

**Question:** How to design DB trigger constraints that enforce different valid state transitions based on `schedule IS NOT NULL`?

### Findings

The outbox trigger pattern (`enforce_outbox_status_transition`) is well-established:

```sql
CREATE OR REPLACE FUNCTION enforce_outbox_status_transition() RETURNS trigger AS $$
BEGIN
    IF OLD.status = NEW.status THEN RETURN NEW; END IF;
    IF OLD.status = 'pending' AND NEW.status IN ('published', 'failed') THEN RETURN NEW; END IF;
    RAISE EXCEPTION 'invalid outbox status transition: % → %', OLD.status, NEW.status
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;
```

Used identically across order, payment, catalog services. Tested in `shared/tests/outbox_migration_test.rs` with both valid and invalid transitions.

### Decision: Extended Trigger for Job Status Transitions

The job trigger adds a `schedule` column check for transitions that are only valid for recurring jobs:

```sql
CREATE OR REPLACE FUNCTION enforce_job_status_transition() RETURNS trigger AS $$
BEGIN
    -- Self-transitions always allowed
    IF OLD.status = NEW.status THEN
        RETURN NEW;
    END IF;

    -- Universal transitions (both one-shot and recurring)
    IF OLD.status = 'pending'  AND NEW.status = 'running'       THEN RETURN NEW; END IF;
    IF OLD.status = 'running'  AND NEW.status = 'completed'     THEN RETURN NEW; END IF;
    IF OLD.status = 'running'  AND NEW.status = 'failed'        THEN RETURN NEW; END IF;
    IF OLD.status = 'running'  AND NEW.status = 'dead_lettered' THEN RETURN NEW; END IF;
    IF OLD.status = 'running'  AND NEW.status = 'pending'       THEN RETURN NEW; END IF;  -- retry
    IF OLD.status = 'pending'  AND NEW.status = 'cancelled'     THEN RETURN NEW; END IF;

    -- Admin retry from terminal states
    IF OLD.status = 'dead_lettered' AND NEW.status = 'pending'  THEN RETURN NEW; END IF;
    IF OLD.status = 'failed'        AND NEW.status = 'pending'  THEN RETURN NEW; END IF;

    -- Recurring-only: completed -> pending (reset-in-place)
    IF OLD.status = 'completed' AND NEW.status = 'pending' THEN
        IF NEW.schedule IS NOT NULL THEN
            RETURN NEW;
        END IF;
        RAISE EXCEPTION 'completed → pending is only valid for recurring jobs (schedule IS NOT NULL)'
            USING ERRCODE = 'check_violation';
    END IF;

    RAISE EXCEPTION 'invalid job status transition: % → %', OLD.status, NEW.status
        USING ERRCODE = 'check_violation';
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER job_enforce_status_transition
    BEFORE UPDATE OF status ON persistent_jobs
    FOR EACH ROW EXECUTE FUNCTION enforce_job_status_transition();
```

Key design points:
- `completed -> pending` gated by `schedule IS NOT NULL` (recurring only)
- All other transitions are universal (same for both types)
- Self-transitions always allowed (idempotent updates)
- Same `check_violation` ERRCODE as outbox for consistent error handling
- Tests should cover both recurring and one-shot scenarios for the gated transition
