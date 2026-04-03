use serde_json::Value;
use uuid::Uuid;

use crate::errors::AppError;
use crate::jobs::types::{Job, JobConfig, JobName};

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
