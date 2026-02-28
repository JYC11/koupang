use crate::errors::AppError;
use crate::outbox::OutboxMetrics;

/// Collect all outbox metrics in a single database round-trip.
pub async fn collect_outbox_metrics(
    executor: impl sqlx::PgExecutor<'_>,
) -> Result<OutboxMetrics, AppError> {
    let row: (i64, i64, i64, Option<f64>) = sqlx::query_as(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE status = 'pending')   AS pending_count,
            COUNT(*) FILTER (WHERE status = 'failed')    AS failed_count,
            COUNT(*) FILTER (WHERE status = 'published') AS published_count,
            EXTRACT(EPOCH FROM (NOW() - MIN(created_at) FILTER (WHERE status = 'pending')))::float8
                AS oldest_pending_age_secs
        FROM outbox_events
        "#,
    )
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;

    Ok(OutboxMetrics {
        pending_count: row.0,
        failed_count: row.1,
        published_count: row.2,
        oldest_pending_age_secs: row.3,
    })
}
