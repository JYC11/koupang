# External Research

## JobRunr (https://github.com/jobrunr/jobrunr)

**Target:** Table schema, claiming strategy, retry/backoff, recurring job scheduling, state machine transitions

### Table Schema

Source: [v001__create_job_table.sql](https://github.com/jobrunr/jobrunr/blob/master/core/src/main/resources/org/jobrunr/storage/sql/common/migrations/v001__create_job_table.sql)

```sql
CREATE TABLE jobrunr_jobs
(
    id           NCHAR(36) PRIMARY KEY,
    version      int NOT NULL,
    jobAsJson    text NOT NULL,
    jobSignature VARCHAR(512) NOT NULL,
    state        VARCHAR(36) NOT NULL,
    createdAt    TIMESTAMP NOT NULL,
    updatedAt    TIMESTAMP NOT NULL,
    scheduledAt  TIMESTAMP
);

CREATE INDEX jobrunr_state_idx ON jobrunr_jobs (state);
CREATE INDEX jobrunr_job_signature_idx ON jobrunr_jobs (jobSignature);
CREATE INDEX jobrunr_job_created_at_idx ON jobrunr_jobs (createdAt);
CREATE INDEX jobrunr_job_updated_at_idx ON jobrunr_jobs (updatedAt);
CREATE INDEX jobrunr_job_scheduled_at_idx ON jobrunr_jobs (scheduledAt);
```

Key observations:
- The job table uses a 36-char `NCHAR` UUID as primary key
- A `version` column is present for optimistic locking (see claiming strategy)
- Job details are stored as a JSON blob (`jobAsJson` as TEXT), not as individual columns
- `jobSignature` is a VARCHAR(512) used to identify the method to execute
- `state` is stored as a VARCHAR(36) string, not an enum
- Indexes on `state`, `jobSignature`, `createdAt`, `updatedAt`, `scheduledAt`

Source: [v002__create_recurring_job_table.sql](https://github.com/jobrunr/jobrunr/blob/master/core/src/main/resources/org/jobrunr/storage/sql/common/migrations/v002__create_recurring_job_table.sql)

```sql
CREATE TABLE jobrunr_recurring_jobs
(
    id        NCHAR(128) PRIMARY KEY,
    version   int  NOT NULL,
    jobAsJson text NOT NULL
);
```

Recurring jobs are stored separately with a 128-char ID and the entire schedule/job definition in a JSON blob.

Source: [v003__create_background_job_server_table.sql](https://github.com/jobrunr/jobrunr/blob/master/core/src/main/resources/org/jobrunr/storage/sql/common/migrations/v003__create_background_job_server_table.sql)

```sql
CREATE TABLE jobrunr_backgroundjobservers
(
    id                     NCHAR(36) PRIMARY KEY,
    workerPoolSize         int           NOT NULL,
    pollIntervalInSeconds  int           NOT NULL,
    firstHeartbeat         TIMESTAMP(6)  NOT NULL,
    lastHeartbeat          TIMESTAMP(6)  NOT NULL,
    running                int           NOT NULL,
    systemTotalMemory      BIGINT        NOT NULL,
    systemFreeMemory       BIGINT        NOT NULL,
    systemCpuLoad          NUMERIC(3, 2) NOT NULL,
    processMaxMemory       BIGINT        NOT NULL,
    processFreeMemory      BIGINT        NOT NULL,
    processAllocatedMemory BIGINT        NOT NULL,
    processCpuLoad         NUMERIC(3, 2) NOT NULL
);
CREATE INDEX jobrunr_bgjobsrvrs_fsthb_idx ON jobrunr_backgroundjobservers (firstHeartbeat);
CREATE INDEX jobrunr_bgjobsrvrs_lsthb_idx ON jobrunr_backgroundjobservers (lastHeartbeat);
```

Later migration v006 added a `recurringJobId` column to the jobs table to link recurring job instances back to the recurring job definition.

Later migration v013 added a `createdAt` column to the recurring_jobs table.

### State Machine

Source: [StateName.java](https://github.com/jobrunr/jobrunr/blob/master/core/src/main/java/org/jobrunr/jobs/states/StateName.java)

```java
public enum StateName {
    AWAITING,
    SCHEDULED,
    ENQUEUED,
    PROCESSING,
    FAILED,
    SUCCEEDED,
    DELETED;
}
```

Seven states: AWAITING, SCHEDULED, ENQUEUED, PROCESSING, FAILED, SUCCEEDED, DELETED.

Source: [AllowedJobStateStateChanges.java](https://github.com/jobrunr/jobrunr/blob/master/core/src/main/java/org/jobrunr/jobs/states/AllowedJobStateStateChanges.java)

Allowed transitions:
- **AWAITING** -> AWAITING, SCHEDULED, ENQUEUED, DELETED
- **SCHEDULED** -> anything except PROCESSING (so: AWAITING, SCHEDULED, ENQUEUED, FAILED, SUCCEEDED, DELETED)
- **ENQUEUED** -> anything except ENQUEUED (so: AWAITING, SCHEDULED, PROCESSING, FAILED, SUCCEEDED, DELETED)
- **PROCESSING** -> SUCCEEDED, FAILED, DELETED
- **FAILED** -> SCHEDULED, ENQUEUED, DELETED
- **SUCCEEDED** -> SCHEDULED, ENQUEUED, DELETED
- **DELETED** -> SCHEDULED, ENQUEUED

Notable: FAILED and SUCCEEDED can transition back to SCHEDULED (for retry) or ENQUEUED. DELETED can be re-scheduled.

### Claiming Strategy (Optimistic Locking via Version Column)

Source: [DefaultSqlStorageProvider.java](https://github.com/jobrunr/jobrunr/blob/master/core/src/main/java/org/jobrunr/storage/sql/common/DefaultSqlStorageProvider.java)

The `getJobsToProcess` method in `DefaultSqlStorageProvider`:
1. Selects jobs that are in `ENQUEUED` state (via `selectJobsToProcess`)
2. Calls `job.startProcessingOn(backgroundJobServer)` which changes state to `PROCESSING`
3. Saves the updated jobs back (via `save(jobs)`)
4. The save uses the `version` column for optimistic concurrency control -- if another server already claimed the job and incremented the version, a `ConcurrentJobModificationException` is thrown
5. On `ConcurrentJobModificationException`, the successfully saved jobs are still committed (partial success)
6. Only jobs that ended up in PROCESSING state are returned

This is an **optimistic locking** approach: no `SELECT FOR UPDATE` or row-level locks. Instead, the `version` column is checked on UPDATE. If the version has changed between read and write, the update fails for that job.

### Retry/Backoff

Source: [RetryFilter.java](https://github.com/jobrunr/jobrunr/blob/master/core/src/main/java/org/jobrunr/jobs/filters/RetryFilter.java)

```java
public static final int DEFAULT_BACKOFF_POLICY_TIME_SEED = 3;
public static final int DEFAULT_NBR_OF_RETRIES = 10;

protected long getSecondsToAdd(Job job) {
    return getExponentialBackoffPolicy(job, backOffPolicyTimeSeed);
}

protected long getExponentialBackoffPolicy(Job job, int seed) {
    return (long) Math.pow(seed, getFailureCount(job));
}
```

- Default retry count: 10
- Default backoff seed: 3
- Formula: `seed ^ failureCount` seconds
  - Retry 1: 3^1 = 3 seconds
  - Retry 2: 3^2 = 9 seconds
  - Retry 3: 3^3 = 27 seconds
  - Retry 4: 3^4 = 81 seconds (~1.3 min)
  - Retry 5: 3^5 = 243 seconds (~4 min)
  - Retry 10: 3^10 = 59,049 seconds (~16.4 hours)
- Configurable: `new RetryFilter(20, 4)` for 20 retries with seed=4
- On retry, the job is moved from FAILED -> SCHEDULED with `scheduledAt = now + delay`
- Skips retry if: job not in failed state, exception is `JobNotFoundException`, exception is marked `mustNotRetry`, or max retries exceeded

### Recurring Job Scheduling

Source: [JobRunr Documentation](https://www.jobrunr.io/en/documentation/)

- A `RecurringJob` stores a CRON schedule or fixed interval, plus the job definition as JSON
- A component called `ProcessRecurringJobsTask` checks recurring jobs and enqueues them as fire-and-forget jobs when due
- The recurring job table is separate from the jobs table
- When a recurring job fires, a new `Job` row is created in the jobs table with a `recurringJobId` linking back
- Deduplication: v006 migration added `recurringJobId` to the jobs table so the system can check if a recurring job is already scheduled/enqueued (previously used `jobSignature` which was less precise)

---

## Quartz Scheduler (https://github.com/quartz-scheduler/quartz)

**Target:** JDBC job store schema, clustering/lock strategy, cron trigger implementation, misfire handling

### JDBC Job Store Schema

Source: [tables_postgres.sql](https://github.com/quartz-scheduler/quartz/blob/main/quartz/src/main/resources/org/quartz/impl/jdbcjobstore/tables_postgres.sql)

Quartz uses 11 tables. The key tables:

**QRTZ_JOB_DETAILS** -- Job definitions (what to run)
```sql
CREATE TABLE QRTZ_JOB_DETAILS (
    SCHED_NAME        VARCHAR(120) NOT NULL,
    JOB_NAME          VARCHAR(200) NOT NULL,
    JOB_GROUP         VARCHAR(200) NOT NULL,
    DESCRIPTION       VARCHAR(250) NULL,
    JOB_CLASS_NAME    VARCHAR(250) NOT NULL,
    IS_DURABLE        BOOL         NOT NULL,
    IS_NONCONCURRENT  BOOL         NOT NULL,
    IS_UPDATE_DATA    BOOL         NOT NULL,
    REQUESTS_RECOVERY BOOL         NOT NULL,
    JOB_DATA          BYTEA        NULL,
    PRIMARY KEY (SCHED_NAME, JOB_NAME, JOB_GROUP)
);
```

**QRTZ_TRIGGERS** -- When to run (trigger definitions)
```sql
CREATE TABLE QRTZ_TRIGGERS (
    SCHED_NAME     VARCHAR(120) NOT NULL,
    TRIGGER_NAME   VARCHAR(200) NOT NULL,
    TRIGGER_GROUP  VARCHAR(200) NOT NULL,
    JOB_NAME       VARCHAR(200) NOT NULL,
    JOB_GROUP      VARCHAR(200) NOT NULL,
    DESCRIPTION    VARCHAR(250) NULL,
    NEXT_FIRE_TIME BIGINT       NULL,
    PREV_FIRE_TIME BIGINT       NULL,
    PRIORITY       INTEGER      NULL,
    TRIGGER_STATE  VARCHAR(16)  NOT NULL,
    TRIGGER_TYPE   VARCHAR(8)   NOT NULL,
    START_TIME     BIGINT       NOT NULL,
    END_TIME       BIGINT       NULL,
    CALENDAR_NAME  VARCHAR(200) NULL,
    MISFIRE_INSTR  SMALLINT     NULL,
    JOB_DATA       BYTEA        NULL,
    PRIMARY KEY (SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP),
    FOREIGN KEY (SCHED_NAME, JOB_NAME, JOB_GROUP)
        REFERENCES QRTZ_JOB_DETAILS (SCHED_NAME, JOB_NAME, JOB_GROUP)
);
```

**QRTZ_CRON_TRIGGERS** -- Cron-specific trigger data
```sql
CREATE TABLE QRTZ_CRON_TRIGGERS (
    SCHED_NAME      VARCHAR(120) NOT NULL,
    TRIGGER_NAME    VARCHAR(200) NOT NULL,
    TRIGGER_GROUP   VARCHAR(200) NOT NULL,
    CRON_EXPRESSION VARCHAR(120) NOT NULL,
    TIME_ZONE_ID    VARCHAR(80),
    PRIMARY KEY (SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP),
    FOREIGN KEY (SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP)
        REFERENCES QRTZ_TRIGGERS (SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP)
);
```

**QRTZ_SIMPLE_TRIGGERS** -- Interval-based triggers
```sql
CREATE TABLE QRTZ_SIMPLE_TRIGGERS (
    SCHED_NAME      VARCHAR(120) NOT NULL,
    TRIGGER_NAME    VARCHAR(200) NOT NULL,
    TRIGGER_GROUP   VARCHAR(200) NOT NULL,
    REPEAT_COUNT    BIGINT       NOT NULL,
    REPEAT_INTERVAL BIGINT       NOT NULL,
    TIMES_TRIGGERED BIGINT       NOT NULL,
    PRIMARY KEY (SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP),
    FOREIGN KEY (SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP)
        REFERENCES QRTZ_TRIGGERS (SCHED_NAME, TRIGGER_NAME, TRIGGER_GROUP)
);
```

**QRTZ_FIRED_TRIGGERS** -- Currently executing triggers
```sql
CREATE TABLE QRTZ_FIRED_TRIGGERS (
    SCHED_NAME        VARCHAR(120) NOT NULL,
    ENTRY_ID          VARCHAR(95)  NOT NULL,
    TRIGGER_NAME      VARCHAR(200) NOT NULL,
    TRIGGER_GROUP     VARCHAR(200) NOT NULL,
    INSTANCE_NAME     VARCHAR(200) NOT NULL,
    FIRED_TIME        BIGINT       NOT NULL,
    SCHED_TIME        BIGINT       NOT NULL,
    PRIORITY          INTEGER      NOT NULL,
    STATE             VARCHAR(16)  NOT NULL,
    JOB_NAME          VARCHAR(200) NULL,
    JOB_GROUP         VARCHAR(200) NULL,
    IS_NONCONCURRENT  BOOL         NULL,
    REQUESTS_RECOVERY BOOL         NULL,
    PRIMARY KEY (SCHED_NAME, ENTRY_ID)
);
```

**QRTZ_LOCKS** -- Database-level lock table
```sql
CREATE TABLE QRTZ_LOCKS (
    SCHED_NAME VARCHAR(120) NOT NULL,
    LOCK_NAME  VARCHAR(40)  NOT NULL,
    PRIMARY KEY (SCHED_NAME, LOCK_NAME)
);
```

**QRTZ_SCHEDULER_STATE** -- Cluster node tracking
```sql
CREATE TABLE QRTZ_SCHEDULER_STATE (
    SCHED_NAME        VARCHAR(120) NOT NULL,
    INSTANCE_NAME     VARCHAR(200) NOT NULL,
    LAST_CHECKIN_TIME BIGINT       NOT NULL,
    CHECKIN_INTERVAL  BIGINT       NOT NULL,
    PRIMARY KEY (SCHED_NAME, INSTANCE_NAME)
);
```

Key design observations:
- Jobs and triggers are **separate concepts** -- a job can have multiple triggers
- All tables are namespaced by `SCHED_NAME` to support multi-scheduler in one DB
- Jobs use `(JOB_NAME, JOB_GROUP)` composite keys with a name/group naming convention
- Times stored as `BIGINT` (epoch millis), not TIMESTAMP
- Trigger types use table-per-type inheritance: base `QRTZ_TRIGGERS` + child tables for cron, simple, blob, simprop
- Heavy indexing on triggers table: 13 indexes including composite indexes on `(state, next_fire_time)`, `(misfire_instr, next_fire_time)`, `(misfire_instr, next_fire_time, state)`
- `FIRED_TRIGGERS` table tracks currently executing instances, enabling recovery on node failure

### Trigger States

Source: [Constants.java](https://github.com/quartz-scheduler/quartz/blob/main/quartz/src/main/java/org/quartz/impl/jdbcjobstore/Constants.java)

```java
String STATE_WAITING = "WAITING";
String STATE_ACQUIRED = "ACQUIRED";
String STATE_EXECUTING = "EXECUTING";
String STATE_COMPLETE = "COMPLETE";
String STATE_BLOCKED = "BLOCKED";
String STATE_ERROR = "ERROR";
String STATE_PAUSED = "PAUSED";
String STATE_PAUSED_BLOCKED = "PAUSED_BLOCKED";
String STATE_DELETED = "DELETED";
```

Nine trigger states. BLOCKED/PAUSED_BLOCKED are for non-concurrent job triggers that are waiting for another instance to finish.

### Clustering/Lock Strategy

Source: [StdRowLockSemaphore.java](https://github.com/quartz-scheduler/quartz/blob/main/quartz/src/main/java/org/quartz/impl/jdbcjobstore/StdRowLockSemaphore.java)

Quartz uses **`SELECT ... FOR UPDATE`** on the `QRTZ_LOCKS` table for cluster coordination:

```java
public static final String SELECT_FOR_LOCK = "SELECT * FROM "
    + TABLE_PREFIX_SUBST + TABLE_LOCKS
    + " WHERE " + COL_SCHEDULER_NAME + " = " + SCHED_NAME_SUBST
    + " AND " + COL_LOCK_NAME + " = ? FOR UPDATE";
```

The process:
1. Execute `SELECT * FROM QRTZ_LOCKS WHERE SCHED_NAME = ? AND LOCK_NAME = ? FOR UPDATE`
2. If no row exists, INSERT a new lock row
3. The `FOR UPDATE` causes other transactions to block until the lock is released (on commit/rollback)
4. Configurable retry: `maxRetry` (default 3) with `retryPeriod` (default 1000ms)
5. On failure to acquire, rollback and retry with sleep

The lock names used are typically `TRIGGER_ACCESS` and `STATE_ACCESS`.

### Misfire Handling

Quartz stores a `MISFIRE_INSTR` (misfire instruction) smallint column on each trigger. Misfires are detected dynamically by checking if `NEXT_FIRE_TIME` is older than the misfire threshold. The indexes `IDX_QRTZ_T_NFT_MISFIRE`, `IDX_QRTZ_T_NFT_ST_MISFIRE`, and `IDX_QRTZ_T_NFT_ST_MISFIRE_GRP` all include `MISFIRE_INSTR` and `NEXT_FIRE_TIME` for efficient misfire detection queries. The misfire instruction determines what action to take: fire now, do nothing, fire once now and reschedule, etc.

---

## Oban (https://github.com/oban-bg/oban) -- Elixir/Postgres

**Target:** Schema design, claiming strategy, state machine, retry, deduplication, cleanup

### Table Schema

Source: [v01.ex migration](https://github.com/oban-bg/oban/blob/main/lib/oban/migrations/postgres/v01.ex)

```sql
-- Postgres enum type for job state
CREATE TYPE oban_job_state AS ENUM (
    'available',
    'suspended',
    'scheduled',
    'executing',
    'retryable',
    'completed',
    'discarded',
    'cancelled'
);

-- Main jobs table
CREATE TABLE oban_jobs (
    id            BIGSERIAL PRIMARY KEY,
    state         oban_job_state NOT NULL DEFAULT 'available',
    queue         TEXT NOT NULL DEFAULT 'default',
    worker        TEXT NOT NULL,
    args          JSONB NOT NULL,
    errors        JSONB[] NOT NULL DEFAULT '{}',  -- array of error maps
    attempt       INTEGER NOT NULL DEFAULT 0,
    max_attempts  INTEGER NOT NULL DEFAULT 20,
    inserted_at   TIMESTAMPTZ NOT NULL DEFAULT timezone('UTC', now()),
    scheduled_at  TIMESTAMPTZ NOT NULL DEFAULT timezone('UTC', now()),
    attempted_at  TIMESTAMPTZ,
    completed_at  TIMESTAMPTZ
);

CREATE INDEX ON oban_jobs (queue);
CREATE INDEX ON oban_jobs (state);
CREATE INDEX ON oban_jobs (scheduled_at);
```

Key observations:
- Uses a Postgres **ENUM type** for states (not a string column)
- `BIGSERIAL` auto-increment PK (not UUID)
- `args` stored as `JSONB` (queryable JSON), not TEXT
- `errors` is a **JSONB array** -- each element is a map with error details, preserving full error history
- `attempt` / `max_attempts` tracked directly on the row (default max: 20)
- `worker` column stores the Elixir module name that handles the job
- `queue` column allows per-queue isolation and concurrency limits
- Timestamps use `TIMESTAMPTZ` (timezone-aware)

### State Machine

Eight states: `available`, `suspended`, `scheduled`, `executing`, `retryable`, `completed`, `discarded`, `cancelled`

- `available` -- ready to be picked up by a queue
- `suspended` -- manually paused, not eligible for execution (added in v2.21)
- `scheduled` -- has a future `scheduled_at`, will become `available` when due
- `executing` -- currently being processed
- `retryable` -- failed but will be retried (becomes `available` at `scheduled_at`)
- `completed` -- finished successfully
- `discarded` -- exhausted all retries or returned a discard signal
- `cancelled` -- explicitly cancelled by user

### Claiming Strategy (Postgres CTE + FOR UPDATE SKIP LOCKED)

Source: [Oban documentation](https://hexdocs.pm/oban/Oban.html) and engine architecture

Oban uses `FOR UPDATE SKIP LOCKED` for job claiming:
- A CTE selects available jobs ordered by priority and scheduled_at
- `FOR UPDATE SKIP LOCKED` ensures that locked rows are skipped (not blocked on)
- The outer query updates the selected rows to `executing` state
- This is a non-blocking approach: workers never wait for each other

The Postgres engine performs this in a single atomic query. The `FOR UPDATE SKIP LOCKED` pattern is the core of Oban's claiming mechanism, providing efficient concurrent job fetching without table-level locks.

### Notification via Postgres Triggers

Source: [v01.ex migration](https://github.com/oban-bg/oban/blob/main/lib/oban/migrations/postgres/v01.ex)

```sql
CREATE OR REPLACE FUNCTION oban_jobs_notify() RETURNS trigger AS $$
DECLARE
    channel text;
    notice json;
BEGIN
    IF (TG_OP = 'INSERT') THEN
        channel = 'oban_insert';
        notice = json_build_object('queue', NEW.queue, 'state', NEW.state);
        IF NEW.scheduled_at IS NOT NULL AND NEW.scheduled_at > now() AT TIME ZONE 'utc' THEN
            RETURN null;
        END IF;
    ELSE
        channel = 'oban_update';
        notice = json_build_object('queue', NEW.queue, 'new_state', NEW.state, 'old_state', OLD.state);
    END IF;
    PERFORM pg_notify(channel, notice::text);
    RETURN NULL;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER oban_notify
    AFTER INSERT OR UPDATE OF state ON oban_jobs
    FOR EACH ROW EXECUTE PROCEDURE oban_jobs_notify();
```

- Uses Postgres `pg_notify` (LISTEN/NOTIFY) for real-time job dispatch
- Insert trigger notifies on new available jobs (skips future-scheduled)
- Update trigger notifies on state changes
- This eliminates polling latency: workers are notified immediately when jobs are available

### Deduplication (Unique Jobs)

Source: [Oban unique jobs docs](https://hexdocs.pm/oban/unique_jobs.html)

Oban supports unique job constraints:
- Uniqueness can be scoped by: `worker`, `queue`, `args`, `states`, and a time `period`
- Configured via worker module options: `use Oban.Worker, unique: [period: 60, fields: [:worker, :queue]]`
- On conflict, behavior options include: `:raise`, `:replace` (update the existing job), or `:discard` (do nothing)
- Uniqueness checks happen at insert time using database constraints
- Sub-argument keys can be specified for partial args matching

### Retry

Source: [Oban error handling docs](https://hexdocs.pm/oban/error_handling.html)

- Default `max_attempts` is 20
- On failure, job moves to `retryable` state with a computed `scheduled_at`
- Backoff is configurable per-worker via `backoff/1` callback
- Default backoff: exponential, formula `attempt^4` seconds (4th power)
- Workers can return `{:snooze, seconds}` to reschedule without counting as an attempt
- Workers can return `:discard` or `{:discard, reason}` to move straight to `discarded`
- Error details are appended to the `errors` JSONB array (preserved history)

### Cleanup

- Completed/discarded/cancelled jobs are **not deleted** by default -- retained for metrics
- A `Pruner` plugin handles cleanup: configurable max age or max count
- Default config: `{Oban.Plugins.Pruner, max_age: 60}` (prune after 60 seconds)
- Pruning runs periodically and deletes jobs past the configured threshold

### Peer Election

Source: [v11.ex migration](https://github.com/oban-bg/oban/blob/main/lib/oban/migrations/postgres/v11.ex)

```sql
CREATE TABLE oban_peers (
    name       TEXT PRIMARY KEY,
    node       TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
```

- Used for leader election among nodes
- Only the leader runs periodic/cron job scheduling and pruning
- Lease-based: `expires_at` provides automatic leader failover

---

## GoodJob (https://github.com/bensheldon/good_job) -- Ruby/Postgres

**Target:** Schema design, claiming strategy, deduplication

### Table Schema

Source: [create_good_jobs.rb.erb](https://github.com/bensheldon/good_job/blob/main/lib/generators/good_job/templates/install/migrations/create_good_jobs.rb.erb)

```ruby
create_table :good_jobs, id: :uuid do |t|
    t.text :queue_name
    t.integer :priority
    t.jsonb :serialized_params
    t.datetime :scheduled_at
    t.datetime :performed_at
    t.datetime :finished_at
    t.text :error
    t.timestamps                     # created_at, updated_at
    t.uuid :active_job_id
    t.text :concurrency_key
    t.text :cron_key
    t.uuid :retried_good_job_id
    t.datetime :cron_at
    t.uuid :batch_id
    t.uuid :batch_callback_id
    t.boolean :is_discrete
    t.integer :executions_count
    t.text :job_class
    t.integer :error_event, limit: 2  # smallint
    t.text :labels, array: true
    t.uuid :locked_by_id
    t.datetime :locked_at
end
```

Key observations:
- UUID primary key (via Postgres `gen_random_uuid()`)
- State is implicit: derived from `finished_at`, `error`, `performed_at` combinations (no explicit state column)
- `locked_by_id` / `locked_at` for process-level locking
- `concurrency_key` for concurrency control (limiting concurrent executions of similar jobs)
- `cron_key` + `cron_at` for recurring job deduplication
- `serialized_params` as JSONB for job parameters
- `executions_count` tracks retry attempts
- `error_event` is a smallint enum (retry, discard, etc.)
- `labels` is a Postgres text array with GIN index

### Indexes (notable)

```ruby
# Candidate job lookup: unfinished, unlocked, ordered by priority/scheduled_at
add_index :good_jobs, [:priority, :scheduled_at],
    order: { priority: "ASC NULLS LAST", scheduled_at: :asc },
    where: "finished_at IS NULL AND locked_by_id IS NULL",
    name: :index_good_jobs_on_priority_scheduled_at_unfinished_unlocked

# Cron deduplication: unique on (cron_key, cron_at)
add_index :good_jobs, [:cron_key, :cron_at],
    where: "(cron_key IS NOT NULL)", unique: true,
    name: :index_good_jobs_on_cron_key_and_cron_at_cond

# Partial index on finished jobs
add_index :good_jobs, [:finished_at],
    where: "finished_at IS NOT NULL",
    name: :index_good_jobs_jobs_on_finished_at_only
```

- Heavy use of **partial indexes** (`WHERE` clauses) to keep indexes small
- Candidate lookup index filters for `finished_at IS NULL AND locked_by_id IS NULL`
- Cron deduplication enforced at the database level via unique index on `(cron_key, cron_at)`

### Claiming Strategy

GoodJob uses **advisory locks** (Postgres `pg_try_advisory_lock`) combined with `locked_by_id`:
- Each process has a UUID identifier
- To claim a job: SELECT candidates with `locked_by_id IS NULL`, then UPDATE to set `locked_by_id` and `locked_at`
- Advisory locks provide an additional layer: `pg_try_advisory_lock(job_id)` returns true only for the first caller
- This is non-blocking (try-lock, not wait-lock)

### Separate Executions Table

```ruby
create_table :good_job_executions, id: :uuid do |t|
    t.timestamps
    t.uuid :active_job_id, null: false
    t.text :job_class
    t.text :queue_name
    t.jsonb :serialized_params
    t.datetime :scheduled_at
    t.datetime :finished_at
    t.text :error
    t.integer :error_event, limit: 2
    t.text :error_backtrace, array: true
    t.uuid :process_id
    t.interval :duration
end
```

Each execution attempt creates a row in `good_job_executions`, providing a complete audit trail.

### Processes Table

```ruby
create_table :good_job_processes, id: :uuid do |t|
    t.timestamps
    t.jsonb :state
    t.integer :lock_type, limit: 2
end
```

Tracks active worker processes with their state as JSON.

---

## Hangfire (https://github.com/HangfireIO/Hangfire) -- .NET/SQL Server

**Target:** Schema design, claiming strategy, state machine

### Table Schema

Source: [Install.sql](https://github.com/HangfireIO/Hangfire/blob/main/src/Hangfire.SqlServer/Install.sql)

**Job table:**
```sql
CREATE TABLE [HangFire].[Job] (
    [Id]             BIGINT IDENTITY(1,1) NOT NULL,
    [StateId]        BIGINT NULL,
    [StateName]      NVARCHAR(20) NULL,
    [InvocationData] NVARCHAR(MAX) NOT NULL,
    [Arguments]      NVARCHAR(MAX) NOT NULL,
    [CreatedAt]      DATETIME NOT NULL,
    [ExpireAt]       DATETIME NULL,
    PRIMARY KEY CLUSTERED ([Id] ASC)
);
CREATE NONCLUSTERED INDEX [IX_HangFire_Job_StateName]
    ON [HangFire].[Job] ([StateName]) WHERE [StateName] IS NOT NULL;
CREATE NONCLUSTERED INDEX [IX_HangFire_Job_ExpireAt]
    ON [HangFire].[Job] ([ExpireAt]) INCLUDE ([StateName]) WHERE [ExpireAt] IS NOT NULL;
```

**State history table (separate):**
```sql
CREATE TABLE [HangFire].[State] (
    [Id]        BIGINT IDENTITY(1,1) NOT NULL,
    [JobId]     BIGINT NOT NULL,
    [Name]      NVARCHAR(20) NOT NULL,
    [Reason]    NVARCHAR(100) NULL,
    [CreatedAt] DATETIME NOT NULL,
    [Data]      NVARCHAR(MAX) NULL,
    PRIMARY KEY CLUSTERED ([JobId] ASC, [Id])
);
```

**Job queue table:**
```sql
CREATE TABLE [HangFire].[JobQueue] (
    [Id]        BIGINT IDENTITY(1,1) NOT NULL,
    [JobId]     BIGINT NOT NULL,
    [Queue]     NVARCHAR(50) NOT NULL,
    [FetchedAt] DATETIME NULL,
    PRIMARY KEY CLUSTERED ([Queue] ASC, [Id] ASC)
);
```

Key observations:
- BIGINT identity PK (auto-increment)
- `StateName` is denormalized on Job for fast queries, `StateId` points to the latest State record
- Full state history in a separate `State` table with `Reason` and `Data` (NVARCHAR(MAX) JSON)
- `ExpireAt` column for automatic cleanup -- filtered index for efficient expiration queries
- `JobQueue` is a separate table from Job -- jobs are enqueued by inserting into JobQueue
- `FetchedAt` in JobQueue: NULL means available, non-NULL means fetched (used for visibility timeout claiming)
- `InvocationData` and `Arguments` stored as NVARCHAR(MAX) JSON

### Claiming Strategy (Visibility Timeout)

Hangfire uses a **visibility timeout** pattern via the `JobQueue` table:
1. Fetch: `UPDATE TOP(1) SET FetchedAt = GETUTCDATE() WHERE Queue = @queue AND FetchedAt IS NULL` (or `FetchedAt < threshold` for stale fetches)
2. The `FetchedAt` column acts as a lock: non-NULL means claimed
3. If a worker dies, `FetchedAt` becomes stale and the job becomes available again after the timeout
4. Uses `sp_getapplock` (SQL Server application lock) for schema migrations

### Cleanup

- `ExpireAt` column on multiple tables (Job, Hash, List, Set, AggregatedCounter)
- Filtered indexes: `WHERE [ExpireAt] IS NOT NULL` for efficient cleanup queries
- Background process periodically deletes expired rows
- Completed jobs get an `ExpireAt` set (configurable retention)

### Additional Tables

- `Server` -- worker heartbeat tracking (`Id`, `Data` as JSON, `LastHeartbeat`)
- `Hash` -- key-value storage (clustered on `Key, Field`)
- `Set` -- scored set storage (clustered on `Key, Value`)
- `List` -- list storage (clustered on `Key, Id`)
- `Counter` / `AggregatedCounter` -- distributed counters for metrics

---

## `cron` crate (https://crates.io/crates/cron)

**Target:** How to parse and evaluate cron expressions in Rust, 7-field syntax, timezone handling

### Overview

Source: [docs.rs/cron](https://docs.rs/cron/latest/cron/)

- Version: 0.16.0 (latest as of March 2026)
- License: MIT OR Apache-2.0
- Dependencies: `chrono`, `once_cell`, `phf`, `winnow` (parser combinator)
- Optional: `serde` feature for serialization support

### 7-Field Format (with optional year)

Source: [parsing.rs](https://docs.rs/crate/cron/latest/source/src/parsing.rs)

The crate uses a **7-field format**: `sec min hour day_of_month month day_of_week [year]`

```rust
fn longhand(i: &mut &str) -> winnow::Result<ScheduleFields> {
    let seconds = field.try_map(Seconds::from_field);
    let minutes = field.try_map(Minutes::from_field);
    let hours = field.try_map(Hours::from_field);
    let days_of_month = field_with_any.try_map(DaysOfMonth::from_field);
    let months = field.try_map(Months::from_field);
    let days_of_week = field_with_any.try_map(DaysOfWeek::from_field);
    let years = opt(field.try_map(Years::from_field));  // OPTIONAL 7th field

    terminated(fields, eof)
        .map(|(seconds, minutes, hours, days_of_month, months, days_of_week, years)| {
            let years = years.unwrap_or_else(Years::all);
            ScheduleFields::new(seconds, minutes, hours, days_of_month, months, days_of_week, years)
        })
        .parse_next(i)
}
```

Key facts:
- Minimum 6 fields required: `sec min hour dom month dow`
- 7th field (year) is optional; defaults to all years if omitted
- Standard 5-field cron (`min hour dom month dow`) is **NOT supported** -- 6 fields minimum
- `?` (any) is only valid for `day_of_month` and `day_of_week` fields
- Named values supported: `MON`, `WED`, `January`, `May-Aug`, etc.
- Step syntax: `*/5`, `10-20/2`, `Mon-Fri/2`
- Named ranges: `Mon-Thurs/2`, `February-November/2`

### Shorthand Expressions

```rust
fn shorthand(i: &mut &str) -> winnow::Result<ScheduleFields> {
    alt((
        shorthand_yearly,   // @yearly  -> 0 0 0 1 1 * *
        shorthand_monthly,  // @monthly -> 0 0 0 1 * * *
        shorthand_weekly,   // @weekly  -> 0 0 0 * * 1 *
        shorthand_daily,    // @daily   -> 0 0 0 * * * *
        shorthand_hourly,   // @hourly  -> 0 0 * * * * *
    ))
}
```

Shorthands: `@yearly`, `@monthly`, `@weekly`, `@daily`, `@hourly`. All set seconds and minutes to 0.

### API Surface

```rust
use cron::Schedule;
use chrono::Utc;
use std::str::FromStr;

// Parse a cron expression
let schedule = Schedule::from_str("0 30 9,12,15 1,15 May-Aug Mon,Wed,Fri 2018/2").unwrap();

// Get upcoming fire times (returns iterator of DateTime<Tz>)
for datetime in schedule.upcoming(Utc).take(10) {
    println!("-> {}", datetime);
}

// Get fire times after a specific datetime
let after = chrono::Utc::now();
for datetime in schedule.after(&after).take(5) {
    println!("-> {}", datetime);
}

// Check if a specific datetime matches the schedule
let matches: bool = schedule.includes(some_datetime);

// Get the original expression string
let source: &str = schedule.source();

// Owned iterator (no lifetime ties to Schedule)
let iter = schedule.upcoming_owned(Utc);
let iter = schedule.after_owned(some_datetime);

// Inspect individual time unit ordinals
let seconds: &impl TimeUnitSpec = schedule.seconds();
let minutes: &impl TimeUnitSpec = schedule.minutes();
let hours: &impl TimeUnitSpec = schedule.hours();
let days_of_month: &impl TimeUnitSpec = schedule.days_of_month();
let days_of_week: &impl TimeUnitSpec = schedule.days_of_week();
let months: &impl TimeUnitSpec = schedule.months();
let years: &impl TimeUnitSpec = schedule.years();
```

### Timezone Handling

- `upcoming(tz)` and `after(datetime)` accept any `chrono::TimeZone` implementation
- Works with `Utc`, `Local`, and `chrono-tz` timezones
- The Schedule struct itself is timezone-agnostic; timezone is provided at iteration time
- The crate is `Send + Sync`, safe for concurrent use

### Traits

- `FromStr` -- parse from string
- `TryFrom<&str>`, `TryFrom<String>`, `TryFrom<Cow<'_, str>>` -- multiple conversion paths
- `Clone`, `Debug`, `Display`, `Eq`, `PartialEq`
- Optional `serde::Serialize` / `serde::Deserialize` with `serde` feature

### Gotchas and Limitations

1. **6-field minimum**: Standard 5-field cron expressions (`* * * * *`) will fail to parse. You must provide at least seconds: `0 * * * * *`
2. **No timezone in expression**: The cron expression itself contains no timezone info. Timezone is supplied when iterating. This means the cron string stored in a database needs an accompanying timezone field.
3. **Year field validation**: Intervals in the year field are validated -- e.g., `2020-2040/2200` fails but `2020-2040/10` succeeds
4. **`?` only for day fields**: Using `?` in seconds, minutes, hours, or months will fail to parse
5. **Trailing characters rejected**: `"* * * * * *foo *"` fails to parse (strict parsing)

---

## Key Patterns Across Sources

### Schema Design

- **All systems use a single jobs table** as the core data structure. Oban, JobRunr, and GoodJob put everything in one table. Quartz and Hangfire split into separate tables (job definitions vs triggers/queue).
- **JSON blob storage** is universal: JobRunr stores entire job as `jobAsJson` TEXT, Oban uses `args` JSONB, GoodJob uses `serialized_params` JSONB, Hangfire uses `InvocationData` NVARCHAR(MAX).
- **Recurring/cron jobs** are either stored in a separate table (JobRunr `jobrunr_recurring_jobs`, Quartz `QRTZ_CRON_TRIGGERS`) or handled in the same table with additional columns (GoodJob `cron_key`/`cron_at`, Oban periodic jobs via application config).
- **Worker/server tracking** tables appear in all systems: JobRunr `jobrunr_backgroundjobservers`, Quartz `QRTZ_SCHEDULER_STATE`, GoodJob `good_job_processes`, Hangfire `[Server]`, Oban `oban_peers`.

### Primary Key Strategies

- JobRunr: 36-char UUID string (`NCHAR(36)`)
- Oban: `BIGSERIAL` (auto-increment integer)
- GoodJob: UUID (Postgres native)
- Hangfire: `BIGINT IDENTITY` (auto-increment)
- Quartz: Composite natural keys (`SCHED_NAME, JOB_NAME, JOB_GROUP`)

### Claiming Strategies

Four distinct approaches observed:

1. **Optimistic locking via version column** (JobRunr): Read job, increment version, UPDATE with WHERE version = old_version. If another worker already claimed it, the version check fails. No row-level locks held.

2. **SELECT FOR UPDATE on lock table** (Quartz): A dedicated `LOCKS` table with named locks. `SELECT ... FOR UPDATE` blocks other transactions. Used for coordination of trigger acquisition, not individual job claiming.

3. **FOR UPDATE SKIP LOCKED** (Oban): SELECT with `FOR UPDATE SKIP LOCKED` on the jobs table itself. Locked rows are skipped (not waited on). Atomic claim in a single query. Most efficient for high-throughput Postgres workloads.

4. **Visibility timeout / FetchedAt** (Hangfire): Set `FetchedAt` timestamp on claim. If worker dies, `FetchedAt` becomes stale and job becomes re-claimable after timeout. Simple but requires separate timeout reaper.

5. **Advisory locks + locked_by_id** (GoodJob): Postgres `pg_try_advisory_lock` combined with a `locked_by_id` column. Non-blocking try-lock pattern.

### State Machines

| System   | States                                                                                         |
|----------|-----------------------------------------------------------------------------------------------|
| JobRunr  | AWAITING, SCHEDULED, ENQUEUED, PROCESSING, FAILED, SUCCEEDED, DELETED (7)                     |
| Quartz   | WAITING, ACQUIRED, EXECUTING, COMPLETE, BLOCKED, ERROR, PAUSED, PAUSED_BLOCKED, DELETED (9)   |
| Oban     | available, suspended, scheduled, executing, retryable, completed, discarded, cancelled (8)     |
| GoodJob  | Implicit (derived from finished_at, error, locked_by_id columns)                               |
| Hangfire | State stored in separate State table; StateName denormalized on Job for queries                 |

### Retry/Backoff Formulas

| System   | Default Retries | Backoff Formula            | Notes                                      |
|----------|----------------|----------------------------|--------------------------------------------|
| JobRunr  | 10             | `seed^attempt` (seed=3)    | 3, 9, 27, 81, 243s...                     |
| Oban     | 20             | `attempt^4`                | 1, 16, 81, 256, 625s...                   |
| GoodJob  | Configurable   | Via ActiveJob adapter      | Delegates to Rails retry mechanism         |
| Quartz   | N/A            | Not built-in (misfire)     | Misfire instructions, not retry per se     |
| Hangfire | 10             | Configurable filters       | Default automatic retry with backoff       |

### Deduplication

- **Oban**: Unique constraints on `(worker, queue, args, states)` with configurable time period. Replace or discard on conflict.
- **GoodJob**: `cron_key` + `cron_at` with a unique database index. Ensures only one instance per cron schedule tick.
- **JobRunr**: `recurringJobId` column links fired jobs to their recurring job definition. Checks if a job for the same recurring ID is already scheduled/enqueued.
- **Quartz**: Jobs and triggers use composite natural keys (`name, group`). Inserting a duplicate key fails.
- **Hangfire**: Uses Hash and Set tables with unique indexes for recurring job tracking.

### Cleanup

- **Oban**: Pruner plugin deletes completed/discarded/cancelled jobs past a configurable max age or max count. Jobs are retained by default for metrics.
- **GoodJob**: `finished_at` column with partial indexes. Cleanup via configurable `cleanup_preserved_jobs_before_seconds_ago`.
- **JobRunr**: `deleteJobsPermanently(state, updatedBefore)` method. Background server removes old succeeded/deleted jobs.
- **Hangfire**: `ExpireAt` column on jobs and other tables. Filtered indexes `WHERE ExpireAt IS NOT NULL`. Background process reaps expired rows.
- **Quartz**: Completed triggers/jobs removed immediately (no retention by default). `FIRED_TRIGGERS` table cleaned after execution.

### Notification Mechanisms

- **Oban**: Postgres LISTEN/NOTIFY via triggers on the jobs table. Real-time dispatch without polling.
- **JobRunr**: Polling-based. `BackgroundJobServer` polls the storage provider at configurable intervals.
- **GoodJob**: Postgres LISTEN/NOTIFY (similar to Oban).
- **Quartz**: Polling with configurable interval. `SCHEDULER_STATE` table tracks node heartbeats.
- **Hangfire**: Polling-based with configurable interval.
