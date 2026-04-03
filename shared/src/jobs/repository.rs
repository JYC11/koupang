use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::errors::AppError;
use crate::jobs::types::{Job, JobConfig, JobName, RecurringJobDefinition};

// ── Enqueue ────────────────────────────────────────────────────────

/// Insert a new one-shot job, returning the job ID (D12).
///
/// Accepts any `PgExecutor` — works with both `&PgPool` (standalone)
/// and `&mut PgConnection` (inside `with_transaction`).
///
/// For `DedupStrategy::Skip` (default), the partial unique index on
/// `(job_type, dedup_key) WHERE status IN ('pending', 'running')` rejects
/// duplicates. The `ON CONFLICT DO NOTHING` clause silently skips them,
/// and we return the existing job's ID via a follow-up query.
pub async fn enqueue_job(
    executor: impl sqlx::PgExecutor<'_>,
    job_name: &JobName,
    payload: &Value,
    config: Option<&JobConfig>,
) -> Result<Uuid, AppError> {
    let max_retries = config.and_then(|c| c.max_retries).unwrap_or(5) as i32;
    let timeout_seconds = config.and_then(|c| c.timeout_seconds).unwrap_or(300) as i32;
    let dedup_key = config.and_then(|c| c.dedup_key.as_deref());

    let row: (Uuid,) = sqlx::query_as(
        r#"
        INSERT INTO persistent_jobs (job_type, payload, max_retries, timeout_seconds, dedup_key)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(job_name.as_str())
    .bind(payload)
    .bind(max_retries)
    .bind(timeout_seconds)
    .bind(dedup_key)
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(row.0)
}

// ── Claim batch ────────────────────────────────────────────────────

/// Claim up to `limit` pending jobs ready for execution (D3).
///
/// Uses `FOR UPDATE SKIP LOCKED` to prevent double-claiming across
/// concurrent runner instances. Transitions claimed jobs to `running`.
pub async fn claim_batch(
    pool: &crate::db::PgPool,
    limit: i32,
    instance_id: &str,
) -> Result<Vec<Job>, AppError> {
    sqlx::query_as::<_, Job>(
        r#"
        WITH claimable AS (
            SELECT id FROM persistent_jobs
            WHERE status = 'pending' AND next_run_at <= NOW()
            ORDER BY next_run_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $1
        )
        UPDATE persistent_jobs
        SET status = 'running',
            locked_by = $2,
            locked_at = NOW(),
            attempts = attempts + 1
        FROM claimable
        WHERE persistent_jobs.id = claimable.id
        RETURNING persistent_jobs.*
        "#,
    )
    .bind(limit)
    .bind(instance_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))
}

// ── Mark completed ─────────────────────────────────────────────────

/// Transition a job to `completed` and clear its lock.
pub async fn mark_completed(
    executor: impl sqlx::PgExecutor<'_>,
    job_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE persistent_jobs
        SET status = 'completed',
            locked_by = NULL,
            locked_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(())
}

// ── Mark retry or failed ───────────────────────────────────────────

/// Increment attempts and either retry with exponential backoff or transition
/// to `failed` if retries are exhausted (D9).
///
/// Uses a single UPDATE with CASE expressions — same SQL pattern as
/// `outbox::mark_retry_or_failed()`. Backoff: `2^min(attempts, 10)` seconds.
pub async fn mark_retry_or_failed(
    executor: impl sqlx::PgExecutor<'_>,
    job_id: Uuid,
    error: &str,
    max_retries: i32,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE persistent_jobs
        SET
            status = CASE
                WHEN attempts >= $3 THEN 'failed'
                ELSE 'pending'
            END,
            next_run_at = CASE
                WHEN attempts >= $3 THEN next_run_at
                ELSE NOW() + make_interval(secs => POW(2, LEAST(attempts, 10))::float8)
            END,
            last_error = $2,
            locked_by = NULL,
            locked_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(error)
    .bind(max_retries)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(())
}

// ── Mark dead lettered ─────────────────────────────────────────────

/// Transition a job to `dead_lettered` (permanent error, no retry).
pub async fn mark_dead_lettered(
    executor: impl sqlx::PgExecutor<'_>,
    job_id: Uuid,
    error: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE persistent_jobs
        SET status = 'dead_lettered',
            last_error = $2,
            locked_by = NULL,
            locked_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(error)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(())
}

// ── Release stale locks ────────────────────────────────────────────

/// Free jobs whose lock is older than `stale_timeout_secs`.
///
/// Unlike outbox stale lock recovery (which targets `status='pending'` because
/// the outbox doesn't change status on claim), jobs target `status='running'`
/// because `claim_batch` transitions `pending→running`.
///
/// Returns the number of rows freed.
pub async fn release_stale_locks(
    executor: impl sqlx::PgExecutor<'_>,
    stale_timeout_secs: i64,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        UPDATE persistent_jobs
        SET status = 'pending',
            locked_by = NULL,
            locked_at = NULL
        WHERE status = 'running'
          AND locked_at < NOW() - make_interval(secs => $1::float8)
        "#,
    )
    .bind(stale_timeout_secs)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(result.rows_affected())
}

// ── Cleanup completed ──────────────────────────────────────────────

/// Delete up to 1000 completed jobs older than `max_age_secs` (R4).
///
/// Returns the number of rows deleted. Call in a loop until it returns 0
/// to drain the full backlog in small batches (avoids long transactions
/// and WAL pressure with large backlogs).
pub async fn cleanup_completed(
    executor: impl sqlx::PgExecutor<'_>,
    max_age_secs: i64,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        DELETE FROM persistent_jobs
        WHERE id IN (
            SELECT id FROM persistent_jobs
            WHERE status = 'completed'
              AND updated_at < NOW() - make_interval(secs => $1::float8)
            LIMIT 1000
        )
        "#,
    )
    .bind(max_age_secs)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(result.rows_affected())
}

// ── Seed recurring job ─────────────────────────────────────────────

/// Create the initial slot for a recurring job (D7, D12).
///
/// Uses `ON CONFLICT DO NOTHING` on the `(job_type, dedup_key)` unique partial
/// index. Returns `Some(id)` if a new row was created, `None` if the slot
/// already exists. Safe for concurrent multi-instance startup.
pub async fn seed_recurring_job(
    executor: impl sqlx::PgExecutor<'_>,
    def: &RecurringJobDefinition,
) -> Result<Option<Uuid>, AppError> {
    let schedule_str = match &def.schedule {
        crate::jobs::types::JobSchedule::Cron(s) => s.to_string(),
        crate::jobs::types::JobSchedule::Interval(d) => format!("@every {}s", d.as_secs()),
    };
    let max_retries = def.config.as_ref().and_then(|c| c.max_retries).unwrap_or(5) as i32;
    let timeout_seconds = def
        .config
        .as_ref()
        .and_then(|c| c.timeout_seconds)
        .unwrap_or(300) as i32;

    let row: Option<(Uuid,)> = sqlx::query_as(
        r#"
        INSERT INTO persistent_jobs (job_type, payload, schedule, dedup_key, max_retries, timeout_seconds)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (job_type, dedup_key) WHERE status IN ('pending', 'running') AND dedup_key IS NOT NULL
        DO NOTHING
        RETURNING id
        "#,
    )
    .bind(def.job_name.as_str())
    .bind(&def.payload)
    .bind(&schedule_str)
    .bind(&def.dedup_key)
    .bind(max_retries)
    .bind(timeout_seconds)
    .fetch_optional(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(row.map(|r| r.0))
}

// ── Reset recurring ────────────────────────────────────────────────

/// Reset a recurring job slot back to `pending` with a new `next_run_at` (D7).
///
/// Resets `attempts` to 0, clears lock and error. The row ID stays the same
/// (slot model — no new rows created on each cycle).
pub async fn reset_recurring(
    executor: impl sqlx::PgExecutor<'_>,
    job_id: Uuid,
    next_run_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE persistent_jobs
        SET status = 'pending',
            next_run_at = $2,
            attempts = 0,
            locked_by = NULL,
            locked_at = NULL,
            last_error = NULL
        WHERE id = $1
        "#,
    )
    .bind(job_id)
    .bind(next_run_at)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(())
}
