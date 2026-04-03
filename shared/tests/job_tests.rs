//! Integration tests for the persistent job system.
//!
//! Phase 1: migration triggers, enqueue/claim/complete lifecycle, NOTIFY wakeup,
//! concurrent claim safety, and handler panic safety.
//! Phase 2: retry backoff, retry exhaustion, permanent error, timeout, runner retry e2e.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use shared::db::PgPool;
use shared::jobs::{
    JobConfig, JobError, JobHandler, JobName, JobRegistry, JobRunner, JobRunnerConfig, claim_batch,
    enqueue_job, mark_completed, mark_dead_lettered, mark_retry_or_failed,
};
use shared::test_utils::db::TestDb;

const MIGRATIONS: &str = "tests/migrations";

fn jn(s: &str) -> JobName {
    JobName::new(s).unwrap()
}

fn fast_runner_config() -> JobRunnerConfig {
    JobRunnerConfig {
        poll_interval: Duration::from_millis(50),
        stale_lock_check_interval: Duration::from_secs(1),
        stale_lock_timeout: Duration::from_secs(5),
        cleanup_interval: Duration::from_secs(3600),
        max_concurrent_jobs: 5,
        default_max_retries: 5,
        default_timeout_seconds: 300,
        ..Default::default()
    }
}

// ── Test handler ───────────────────────────────────────────────────

struct CountingHandler {
    invocations: Arc<AtomicU32>,
}

impl CountingHandler {
    fn new() -> (Self, Arc<AtomicU32>) {
        let count = Arc::new(AtomicU32::new(0));
        (
            Self {
                invocations: Arc::clone(&count),
            },
            count,
        )
    }
}

#[async_trait::async_trait]
impl JobHandler for CountingHandler {
    fn job_type(&self) -> &str {
        "test.count"
    }
    async fn execute(&self, _payload: &Value, _pool: &PgPool) -> Result<(), JobError> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct PanicHandler;

#[async_trait::async_trait]
impl JobHandler for PanicHandler {
    fn job_type(&self) -> &str {
        "test.panic"
    }
    async fn execute(&self, _payload: &Value, _pool: &PgPool) -> Result<(), JobError> {
        panic!("simulated handler panic");
    }
}

// ═══════════════════════════════════════════════════════════════════
// Migration & trigger tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn trigger_function_exists() {
    let db = TestDb::start(MIGRATIONS).await;

    let status_trigger: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM pg_trigger WHERE tgname = 'job_enforce_status_transition')",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(status_trigger.0, "status transition trigger should exist");

    let notify_trigger: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM pg_trigger WHERE tgname = 'persistent_jobs_after_insert')",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert!(notify_trigger.0, "NOTIFY trigger should exist");
}

#[tokio::test]
async fn status_check_constraint() {
    let db = TestDb::start(MIGRATIONS).await;

    let result = sqlx::query(
        "INSERT INTO persistent_jobs (job_type, payload, status) VALUES ('test', '{}'::jsonb, 'bogus')",
    )
    .execute(&db.pool)
    .await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("chk_job_status"),
        "expected check constraint violation, got: {err}"
    );
}

#[tokio::test]
async fn valid_transitions() {
    let db = TestDb::start(MIGRATIONS).await;

    // Helper: insert a job and return its id
    async fn insert_job(pool: &PgPool) -> uuid::Uuid {
        let row: (uuid::Uuid,) = sqlx::query_as(
            "INSERT INTO persistent_jobs (job_type, payload) VALUES ('test', '{}'::jsonb) RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        row.0
    }

    // pending -> running
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("pending -> running should be allowed");

    // running -> completed
    sqlx::query("UPDATE persistent_jobs SET status = 'completed' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("running -> completed should be allowed");

    // running -> failed
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'failed' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("running -> failed should be allowed");

    // running -> dead_lettered
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'dead_lettered' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("running -> dead_lettered should be allowed");

    // running -> pending (retry)
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'pending' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("running -> pending should be allowed (retry)");

    // pending -> cancelled
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'cancelled' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("pending -> cancelled should be allowed");

    // dead_lettered -> pending (admin retry)
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'dead_lettered' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'pending' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("dead_lettered -> pending should be allowed");

    // failed -> pending (admin retry)
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'failed' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'pending' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("failed -> pending should be allowed");

    // Self-transitions
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'pending' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .expect("pending -> pending (self) should be allowed");
}

#[tokio::test]
async fn invalid_transitions() {
    let db = TestDb::start(MIGRATIONS).await;

    async fn insert_job(pool: &PgPool) -> uuid::Uuid {
        let row: (uuid::Uuid,) = sqlx::query_as(
            "INSERT INTO persistent_jobs (job_type, payload) VALUES ('test', '{}'::jsonb) RETURNING id",
        )
        .fetch_one(pool)
        .await
        .unwrap();
        row.0
    }

    // completed -> running (invalid)
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'completed' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    let result = sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await;
    assert!(result.is_err(), "completed -> running should be rejected");

    // failed -> running (invalid)
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'failed' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    let result = sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await;
    assert!(result.is_err(), "failed -> running should be rejected");

    // cancelled -> running (invalid)
    let id = insert_job(&db.pool).await;
    sqlx::query("UPDATE persistent_jobs SET status = 'cancelled' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await
        .unwrap();
    let result = sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id)
        .execute(&db.pool)
        .await;
    assert!(result.is_err(), "cancelled -> running should be rejected");
}

#[tokio::test]
async fn recurring_gate() {
    let db = TestDb::start(MIGRATIONS).await;

    // completed -> pending FAILS when schedule IS NULL (one-shot job)
    let id: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO persistent_jobs (job_type, payload) VALUES ('test', '{}'::jsonb) RETURNING id",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id.0)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'completed' WHERE id = $1")
        .bind(id.0)
        .execute(&db.pool)
        .await
        .unwrap();
    let result = sqlx::query("UPDATE persistent_jobs SET status = 'pending' WHERE id = $1")
        .bind(id.0)
        .execute(&db.pool)
        .await;
    assert!(
        result.is_err(),
        "completed -> pending should fail for one-shot"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("recurring"),
        "error should mention recurring, got: {err}"
    );

    // completed -> pending SUCCEEDS when schedule IS NOT NULL (recurring job)
    let id: (uuid::Uuid,) = sqlx::query_as(
        "INSERT INTO persistent_jobs (job_type, payload, schedule) VALUES ('test.recurring', '{}'::jsonb, '0 * * * * *') RETURNING id",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'running' WHERE id = $1")
        .bind(id.0)
        .execute(&db.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE persistent_jobs SET status = 'completed' WHERE id = $1")
        .bind(id.0)
        .execute(&db.pool)
        .await
        .unwrap();
    let result = sqlx::query("UPDATE persistent_jobs SET status = 'pending' WHERE id = $1")
        .bind(id.0)
        .execute(&db.pool)
        .await;
    assert!(
        result.is_ok(),
        "completed -> pending should succeed for recurring"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Repository tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn enqueue_and_claim_happy_path() {
    let db = TestDb::start(MIGRATIONS).await;
    let payload = json!({"order_id": "abc123"});

    let job_id = enqueue_job(&db.pool, &jn("test.job"), &payload, None)
        .await
        .unwrap();

    let jobs = claim_batch(&db.pool, 1, "runner-1").await.unwrap();
    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert_eq!(job.id, job_id);
    assert_eq!(job.job_type, "test.job");
    assert_eq!(job.status.to_string(), "running");
    assert_eq!(job.locked_by.as_deref(), Some("runner-1"));
    assert_eq!(job.attempts, 1);
    assert_eq!(job.payload, payload);
}

#[tokio::test]
async fn claim_skip_locked() {
    let db = TestDb::start(MIGRATIONS).await;

    let id1 = enqueue_job(&db.pool, &jn("test.job"), &json!({"n": 1}), None)
        .await
        .unwrap();
    let id2 = enqueue_job(&db.pool, &jn("test.job"), &json!({"n": 2}), None)
        .await
        .unwrap();

    // First claim gets one job
    let batch1 = claim_batch(&db.pool, 1, "runner-1").await.unwrap();
    assert_eq!(batch1.len(), 1);

    // Second claim gets the other job (SKIP LOCKED skips the first)
    let batch2 = claim_batch(&db.pool, 1, "runner-2").await.unwrap();
    assert_eq!(batch2.len(), 1);

    // No overlap
    assert_ne!(batch1[0].id, batch2[0].id);
    let claimed_ids: std::collections::HashSet<_> = [batch1[0].id, batch2[0].id].into();
    assert!(claimed_ids.contains(&id1));
    assert!(claimed_ids.contains(&id2));
}

#[tokio::test]
async fn claim_respects_next_run_at() {
    let db = TestDb::start(MIGRATIONS).await;

    // Insert a job with next_run_at in the future
    sqlx::query(
        "INSERT INTO persistent_jobs (job_type, payload, next_run_at)
         VALUES ('test.future', '{}'::jsonb, NOW() + INTERVAL '1 hour')",
    )
    .execute(&db.pool)
    .await
    .unwrap();

    let jobs = claim_batch(&db.pool, 10, "runner-1").await.unwrap();
    assert!(jobs.is_empty(), "future job should not be claimable");
}

#[tokio::test]
async fn mark_completed_clears_lock() {
    let db = TestDb::start(MIGRATIONS).await;

    let job_id = enqueue_job(&db.pool, &jn("test.job"), &json!({}), None)
        .await
        .unwrap();
    claim_batch(&db.pool, 1, "runner-1").await.unwrap();

    mark_completed(&db.pool, job_id).await.unwrap();

    let row: (String, Option<String>) =
        sqlx::query_as("SELECT status, locked_by FROM persistent_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();

    assert_eq!(row.0, "completed");
    assert!(row.1.is_none(), "locked_by should be cleared");
}

// ═══════════════════════════════════════════════════════════════════
// Runner integration tests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn runner_end_to_end() {
    let db = TestDb::start(MIGRATIONS).await;
    let (handler, count) = CountingHandler::new();

    let mut registry = JobRegistry::new();
    registry.register(Arc::new(handler));

    let runner = Arc::new(JobRunner::new(
        db.pool.clone(),
        registry,
        fast_runner_config(),
    ));
    let shutdown = CancellationToken::new();

    let runner_handle = {
        let r = Arc::clone(&runner);
        let s = shutdown.clone();
        tokio::spawn(async move { r.run(s).await })
    };

    // Enqueue a job
    enqueue_job(&db.pool, &jn("test.count"), &json!({"x": 1}), None)
        .await
        .unwrap();

    // Wait for handler to be called
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if count.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("handler should be called within 5s");

    assert_eq!(count.load(Ordering::SeqCst), 1);

    shutdown.cancel();
    tokio::time::timeout(Duration::from_secs(5), runner_handle)
        .await
        .expect("runner should shut down within 5s")
        .unwrap();
}

#[tokio::test]
async fn notify_wakeup() {
    let db = TestDb::start(MIGRATIONS).await;
    let (handler, count) = CountingHandler::new();

    let mut registry = JobRegistry::new();
    registry.register(Arc::new(handler));

    // Use a very long poll interval — if handler fires quickly it's because of NOTIFY
    let config = JobRunnerConfig {
        poll_interval: Duration::from_secs(60),
        stale_lock_check_interval: Duration::from_secs(3600),
        stale_lock_timeout: Duration::from_secs(7200),
        ..Default::default()
    };

    let runner = Arc::new(JobRunner::new(db.pool.clone(), registry, config));
    let shutdown = CancellationToken::new();

    let runner_handle = {
        let r = Arc::clone(&runner);
        let s = shutdown.clone();
        tokio::spawn(async move { r.run(s).await })
    };

    // Give runner time to start and connect PgListener
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Enqueue — NOTIFY should wake the runner immediately
    enqueue_job(&db.pool, &jn("test.count"), &json!({}), None)
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if count.load(Ordering::SeqCst) >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("NOTIFY should wake runner within 2s (not waiting for 60s poll)");

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), runner_handle).await;
}

#[tokio::test]
async fn concurrent_claim() {
    let db = TestDb::start(MIGRATIONS).await;
    let (handler1, count1) = CountingHandler::new();
    let (handler2, count2) = CountingHandler::new();

    let mut registry1 = JobRegistry::new();
    registry1.register(Arc::new(handler1));
    let mut registry2 = JobRegistry::new();
    registry2.register(Arc::new(handler2));

    let runner1 = Arc::new(JobRunner::new(
        db.pool.clone(),
        registry1,
        fast_runner_config(),
    ));
    let runner2 = Arc::new(JobRunner::new(
        db.pool.clone(),
        registry2,
        fast_runner_config(),
    ));
    let shutdown = CancellationToken::new();

    let h1 = {
        let r = Arc::clone(&runner1);
        let s = shutdown.clone();
        tokio::spawn(async move { r.run(s).await })
    };
    let h2 = {
        let r = Arc::clone(&runner2);
        let s = shutdown.clone();
        tokio::spawn(async move { r.run(s).await })
    };

    // Enqueue exactly 1 job
    enqueue_job(&db.pool, &jn("test.count"), &json!({}), None)
        .await
        .unwrap();

    // Wait for it to be processed
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let total = count1.load(Ordering::SeqCst) + count2.load(Ordering::SeqCst);
            if total >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("job should be processed within 5s");

    // Exactly 1 handler should have processed it (SKIP LOCKED)
    let total = count1.load(Ordering::SeqCst) + count2.load(Ordering::SeqCst);
    assert_eq!(total, 1, "exactly one runner should claim the job");

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        let _ = h1.await;
        let _ = h2.await;
    })
    .await;
}

#[tokio::test]
async fn handler_panic_safety() {
    let db = TestDb::start(MIGRATIONS).await;

    let mut registry = JobRegistry::new();
    registry.register(Arc::new(PanicHandler));

    let runner = Arc::new(JobRunner::new(
        db.pool.clone(),
        registry,
        fast_runner_config(),
    ));
    let shutdown = CancellationToken::new();

    let runner_handle = {
        let r = Arc::clone(&runner);
        let s = shutdown.clone();
        tokio::spawn(async move { r.run(s).await })
    };

    enqueue_job(&db.pool, &jn("test.panic"), &json!({}), None)
        .await
        .unwrap();

    // Wait for the panic to occur and be caught
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Runner should still be alive (panic was caught by tokio::spawn)
    assert!(
        !runner_handle.is_finished(),
        "runner should survive handler panic"
    );

    shutdown.cancel();
    let result = tokio::time::timeout(Duration::from_secs(5), runner_handle).await;
    assert!(
        result.is_ok(),
        "runner should shut down cleanly after panic"
    );
}

// ═══════════════════════════════════════════════════════════════════
// Phase 2: Error handling tests
// ═══════════════════════════════════════════════════════════════════

// ── Test handlers for Phase 2 ──────────────────────────────────────

struct TransientHandler {
    fail_count: AtomicU32,
    invocations: Arc<AtomicU32>,
}

impl TransientHandler {
    fn new(fail_count: u32) -> (Self, Arc<AtomicU32>) {
        let invocations = Arc::new(AtomicU32::new(0));
        (
            Self {
                fail_count: AtomicU32::new(fail_count),
                invocations: Arc::clone(&invocations),
            },
            invocations,
        )
    }
}

#[async_trait::async_trait]
impl JobHandler for TransientHandler {
    fn job_type(&self) -> &str {
        "test.transient"
    }
    async fn execute(&self, _payload: &Value, _pool: &PgPool) -> Result<(), JobError> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        let remaining = self.fail_count.load(Ordering::SeqCst);
        if remaining > 0 {
            self.fail_count.fetch_sub(1, Ordering::SeqCst);
            return Err(JobError::Transient("temporary failure".into()));
        }
        Ok(())
    }
}

struct PermanentHandler;

#[async_trait::async_trait]
impl JobHandler for PermanentHandler {
    fn job_type(&self) -> &str {
        "test.permanent"
    }
    async fn execute(&self, _payload: &Value, _pool: &PgPool) -> Result<(), JobError> {
        Err(JobError::Permanent("bad data, cannot recover".into()))
    }
}

struct SlowHandler {
    delay: Duration,
    invocations: Arc<AtomicU32>,
}

#[async_trait::async_trait]
impl JobHandler for SlowHandler {
    fn job_type(&self) -> &str {
        "test.slow"
    }
    async fn execute(&self, _payload: &Value, _pool: &PgPool) -> Result<(), JobError> {
        self.invocations.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(self.delay).await;
        Ok(())
    }
}

// ── Repository-level tests ─────────────────────────────────────────

#[tokio::test]
async fn transient_retry_backoff() {
    let db = TestDb::start(MIGRATIONS).await;

    let job_id = enqueue_job(&db.pool, &jn("test.job"), &json!({}), None)
        .await
        .unwrap();
    // Claim to get attempts=1
    claim_batch(&db.pool, 1, "runner-1").await.unwrap();

    // First transient failure (attempts=1, max_retries=5)
    mark_retry_or_failed(&db.pool, job_id, "err1", 5)
        .await
        .unwrap();

    let row: (String, i32, Option<String>) =
        sqlx::query_as("SELECT status, attempts, last_error FROM persistent_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row.0, "pending", "should retry (not exhausted)");
    assert_eq!(row.1, 1, "attempts unchanged by mark_retry_or_failed");
    assert_eq!(row.2.as_deref(), Some("err1"));

    // Verify next_run_at is in the future (backoff applied)
    let has_backoff: (bool,) =
        sqlx::query_as("SELECT next_run_at > NOW() FROM persistent_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert!(
        has_backoff.0,
        "next_run_at should be in the future (backoff)"
    );
}

#[tokio::test]
async fn transient_exhausted_becomes_failed() {
    let db = TestDb::start(MIGRATIONS).await;

    let config = JobConfig {
        max_retries: Some(2),
        ..Default::default()
    };
    let job_id = enqueue_job(&db.pool, &jn("test.job"), &json!({}), Some(&config))
        .await
        .unwrap();

    // Claim (attempts becomes 1)
    claim_batch(&db.pool, 1, "runner-1").await.unwrap();
    // Retry back to pending (attempts=1 < max_retries=2)
    mark_retry_or_failed(&db.pool, job_id, "err1", 2)
        .await
        .unwrap();

    // Reset next_run_at so we can claim again immediately (backoff set it to future)
    sqlx::query("UPDATE persistent_jobs SET next_run_at = NOW() WHERE id = $1")
        .bind(job_id)
        .execute(&db.pool)
        .await
        .unwrap();

    // Claim again (attempts becomes 2)
    claim_batch(&db.pool, 1, "runner-1").await.unwrap();
    // Now attempts=2 >= max_retries=2 → should become failed
    mark_retry_or_failed(&db.pool, job_id, "err2", 2)
        .await
        .unwrap();

    let row: (String, Option<String>) =
        sqlx::query_as("SELECT status, last_error FROM persistent_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(
        row.0, "failed",
        "should be terminal after retries exhausted"
    );
    assert_eq!(row.1.as_deref(), Some("err2"));
}

#[tokio::test]
async fn permanent_dead_lettered() {
    let db = TestDb::start(MIGRATIONS).await;

    let job_id = enqueue_job(&db.pool, &jn("test.job"), &json!({}), None)
        .await
        .unwrap();
    claim_batch(&db.pool, 1, "runner-1").await.unwrap();

    mark_dead_lettered(&db.pool, job_id, "corrupt payload")
        .await
        .unwrap();

    let row: (String, i32, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT status, attempts, last_error, locked_by FROM persistent_jobs WHERE id = $1",
    )
    .bind(job_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, "dead_lettered");
    assert_eq!(row.1, 1, "attempts should be 1 (claimed once)");
    assert_eq!(row.2.as_deref(), Some("corrupt payload"));
    assert!(row.3.is_none(), "lock should be cleared");
}

#[tokio::test]
async fn timeout_leaves_running() {
    let db = TestDb::start(MIGRATIONS).await;

    let count = Arc::new(AtomicU32::new(0));
    let handler = SlowHandler {
        delay: Duration::from_secs(10), // much longer than timeout
        invocations: Arc::clone(&count),
    };

    let mut registry = JobRegistry::new();
    registry.register(Arc::new(handler));

    // Very short timeout to trigger quickly
    let config = JobRunnerConfig {
        poll_interval: Duration::from_millis(50),
        stale_lock_check_interval: Duration::from_secs(3600),
        stale_lock_timeout: Duration::from_secs(7200),
        default_timeout_seconds: 1, // 1 second timeout
        ..Default::default()
    };

    let runner = Arc::new(JobRunner::new(db.pool.clone(), registry, config));
    let shutdown = CancellationToken::new();

    let runner_handle = {
        let r = Arc::clone(&runner);
        let s = shutdown.clone();
        tokio::spawn(async move { r.run(s).await })
    };

    // Enqueue with 1-second timeout override
    let job_config = JobConfig {
        timeout_seconds: Some(1),
        ..Default::default()
    };
    let job_id = enqueue_job(&db.pool, &jn("test.slow"), &json!({}), Some(&job_config))
        .await
        .unwrap();

    // Wait for timeout to fire
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Job should still be 'running' (timeout leaves it for stale lock recovery)
    let row: (String, Option<String>) =
        sqlx::query_as("SELECT status, locked_by FROM persistent_jobs WHERE id = $1")
            .bind(job_id)
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row.0, "running", "timed-out job should stay running");
    assert!(row.1.is_some(), "lock should still be held");

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), runner_handle).await;
}

// ── Runner-level retry e2e ─────────────────────────────────────────

#[tokio::test]
async fn runner_retry_end_to_end() {
    let db = TestDb::start(MIGRATIONS).await;

    // Handler fails transiently once, then succeeds
    let (handler, invocations) = TransientHandler::new(1);

    let mut registry = JobRegistry::new();
    registry.register(Arc::new(handler));

    let config = JobRunnerConfig {
        poll_interval: Duration::from_millis(50),
        stale_lock_check_interval: Duration::from_secs(3600),
        stale_lock_timeout: Duration::from_secs(7200),
        ..Default::default()
    };

    let runner = Arc::new(JobRunner::new(db.pool.clone(), registry, config));
    let shutdown = CancellationToken::new();

    let runner_handle = {
        let r = Arc::clone(&runner);
        let s = shutdown.clone();
        tokio::spawn(async move { r.run(s).await })
    };

    // Enqueue job
    let job_id = enqueue_job(&db.pool, &jn("test.transient"), &json!({}), None)
        .await
        .unwrap();

    // Wait for first attempt (transient failure) + backoff + second attempt (success)
    // Backoff for attempts=1 is 2^1 = 2 seconds
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let row: (String,) = sqlx::query_as("SELECT status FROM persistent_jobs WHERE id = $1")
                .bind(job_id)
                .fetch_one(&db.pool)
                .await
                .unwrap();
            if row.0 == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("job should eventually complete after retry");

    assert!(
        invocations.load(Ordering::SeqCst) >= 2,
        "handler should be called at least twice (fail + succeed)"
    );

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), runner_handle).await;
}
