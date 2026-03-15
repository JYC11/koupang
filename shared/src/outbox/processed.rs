use crate::errors::AppError;
use uuid::Uuid;

/// Check whether a consumed event has already been processed by this consumer group.
pub async fn is_event_processed(
    executor: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
    consumer_group: &str,
) -> Result<bool, AppError> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM processed_events WHERE event_id = $1 AND consumer_group = $2)",
    )
    .bind(event_id)
    .bind(consumer_group)
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(row.0)
}

/// Record that an event has been successfully processed by this consumer group.
///
/// Uses `ON CONFLICT DO NOTHING` so calling this twice with the same
/// `(event_id, consumer_group)` is a safe no-op (idempotent).
pub async fn mark_event_processed(
    executor: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
    event_type: &str,
    source_service: &str,
    consumer_group: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO processed_events (event_id, event_type, source_service, consumer_group)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (event_id, consumer_group) DO NOTHING",
    )
    .bind(event_id)
    .bind(event_type)
    .bind(source_service)
    .bind(consumer_group)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(())
}

/// Delete up to 1000 processed-event records older than `max_age_secs` seconds.
///
/// Returns the number of rows deleted. Call in a loop until it returns 0
/// to drain the full backlog in small batches.
pub async fn cleanup_processed_events(
    executor: impl sqlx::PgExecutor<'_>,
    max_age_secs: i64,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        DELETE FROM processed_events
        WHERE (event_id, consumer_group) IN (
            SELECT event_id, consumer_group FROM processed_events
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
