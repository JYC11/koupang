use crate::errors::AppError;
use uuid::Uuid;

/// Check whether a consumed event has already been processed (consumer-side idempotency).
pub async fn is_event_processed(
    executor: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
) -> Result<bool, AppError> {
    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM processed_events WHERE event_id = $1)")
            .bind(event_id)
            .fetch_one(executor)
            .await
            .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(row.0)
}

/// Record that an event has been successfully processed.
///
/// Uses `ON CONFLICT DO NOTHING` so calling this twice with the same
/// `event_id` is a safe no-op (idempotent).
pub async fn mark_event_processed(
    executor: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
    event_type: &str,
    source_service: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO processed_events (event_id, event_type, source_service)
         VALUES ($1, $2, $3)
         ON CONFLICT (event_id) DO NOTHING",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(source_service)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(())
}

/// Delete up to 1000 processed-event records older than `max_age_secs` seconds.
///
/// Returns the number of rows deleted. Call in a loop until it returns 0
/// to drain the full backlog in small batches (avoids long transactions
/// and WAL pressure with 100K+ rows).
pub async fn cleanup_processed_events(
    executor: impl sqlx::PgExecutor<'_>,
    max_age_secs: i64,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        DELETE FROM processed_events
        WHERE event_id IN (
            SELECT event_id FROM processed_events
            WHERE processed_at < NOW() - make_interval(secs => $1::float8)
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
