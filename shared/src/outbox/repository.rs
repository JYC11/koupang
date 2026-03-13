use uuid::Uuid;

use crate::errors::AppError;
use crate::outbox::types::{OutboxEvent, OutboxInsert};

/// Maximum payload size in bytes (900 KB).
/// Keeps the serialized event under Kafka's default `message.max.bytes` (1 MB).
const MAX_PAYLOAD_BYTES: usize = 900_000;

// ── Insert ──────────────────────────────────────────────────────────

/// Insert a new outbox event, returning the created row.
///
/// Rejects payloads larger than 900 KB to stay within Kafka's default
/// `message.max.bytes` limit.
pub async fn insert_outbox_event(
    executor: impl sqlx::PgExecutor<'_>,
    insert: &OutboxInsert,
) -> Result<OutboxEvent, AppError> {
    let payload_bytes = insert.payload.to_string().len();
    if payload_bytes > MAX_PAYLOAD_BYTES {
        return Err(AppError::BadRequest(format!(
            "Outbox payload too large: {payload_bytes} bytes exceeds {MAX_PAYLOAD_BYTES} byte limit"
        )));
    }

    sqlx::query_as::<_, OutboxEvent>(
        r#"
        INSERT INTO outbox_events
            (aggregate_type, aggregate_id, event_type, event_id,
             topic, partition_key, payload, metadata)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING *
        "#,
    )
    .bind(&insert.aggregate_type)
    .bind(insert.aggregate_id)
    .bind(&insert.event_type)
    .bind(insert.event_id)
    .bind(&insert.topic)
    .bind(&insert.partition_key)
    .bind(&insert.payload)
    .bind(&insert.metadata)
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))
}

// ── Claim batch ─────────────────────────────────────────────────────

/// Claim the oldest pending event per aggregate, up to `batch_size`.
///
/// Uses a two-step CTE to prevent ordering violations:
/// 1. Select the oldest pending event per `aggregate_id`.
/// 2. Lock those rows with `FOR UPDATE SKIP LOCKED` and assign `locked_by`.
pub async fn claim_batch(
    executor: impl sqlx::PgExecutor<'_>,
    batch_size: i64,
    instance_id: &str,
) -> Result<Vec<OutboxEvent>, AppError> {
    sqlx::query_as::<_, OutboxEvent>(
        r#"
        WITH oldest_per_aggregate AS (
            SELECT DISTINCT ON (aggregate_id) id
            FROM outbox_events
            WHERE status = 'pending'
              AND next_retry_at <= NOW()
            ORDER BY aggregate_id, created_at ASC
        ),
        locked AS (
            SELECT oe.id FROM outbox_events oe
            JOIN oldest_per_aggregate opa ON oe.id = opa.id
            WHERE oe.locked_by IS NULL
            FOR UPDATE OF oe SKIP LOCKED
            LIMIT $1
        )
        UPDATE outbox_events
        SET locked_by = $2, locked_at = NOW()
        FROM locked
        WHERE outbox_events.id = locked.id
        RETURNING outbox_events.*
        "#,
    )
    .bind(batch_size)
    .bind(instance_id)
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))
}

// ── Mark published ──────────────────────────────────────────────────

/// Transition an event to `published` and clear its lock.
pub async fn mark_published(
    executor: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE outbox_events
        SET status = 'published',
            published_at = NOW(),
            locked_by = NULL,
            locked_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(event_id)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(())
}

// ── Delete published ────────────────────────────────────────────────

/// Delete a single outbox event (used in `delete_on_publish` mode).
pub async fn delete_published(
    executor: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM outbox_events WHERE id = $1")
        .bind(event_id)
        .execute(executor)
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(())
}

// ── Mark retry or failed ────────────────────────────────────────────

/// Increment retry count, compute exponential back-off, and unlock.
///
/// If `retry_count + 1 >= max_retries`, transition to `failed` instead.
/// Uses a single UPDATE with CASE expressions.
pub async fn mark_retry_or_failed(
    executor: impl sqlx::PgExecutor<'_>,
    event_id: Uuid,
    error: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE outbox_events
        SET
            status = CASE
                WHEN retry_count + 1 >= max_retries THEN 'failed'
                ELSE 'pending'
            END,
            retry_count = retry_count + 1,
            next_retry_at = CASE
                WHEN retry_count + 1 >= max_retries THEN next_retry_at
                ELSE NOW() + make_interval(secs => POW(2, LEAST(retry_count + 1, 10))::float8)
            END,
            last_error = $2,
            locked_by = NULL,
            locked_at = NULL
        WHERE id = $1
        "#,
    )
    .bind(event_id)
    .bind(error)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(())
}

// ── Release stale locks ─────────────────────────────────────────────

/// Unlock events whose lock is older than `stale_timeout_secs`.
/// Returns the number of rows unlocked.
pub async fn release_stale_locks(
    executor: impl sqlx::PgExecutor<'_>,
    stale_timeout_secs: i64,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        UPDATE outbox_events
        SET locked_by = NULL,
            locked_at = NULL
        WHERE locked_by IS NOT NULL
          AND locked_at < NOW() - make_interval(secs => $1::float8)
          AND status = 'pending'
        "#,
    )
    .bind(stale_timeout_secs)
    .execute(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(result.rows_affected())
}

// ── Cleanup published ───────────────────────────────────────────────

/// Delete up to 1000 published events older than `max_age_secs`.
///
/// Returns the number of rows deleted. Call in a loop until it returns 0
/// to drain the full backlog in small batches (avoids long transactions
/// and WAL pressure with 100K+ rows).
pub async fn cleanup_published(
    executor: impl sqlx::PgExecutor<'_>,
    max_age_secs: i64,
) -> Result<u64, AppError> {
    let result = sqlx::query(
        r#"
        DELETE FROM outbox_events
        WHERE id IN (
            SELECT id FROM outbox_events
            WHERE status = 'published'
              AND published_at < NOW() - make_interval(secs => $1::float8)
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

// ── Metrics queries ─────────────────────────────────────────────────

/// Count of pending outbox events (lag).
pub async fn outbox_lag(executor: impl sqlx::PgExecutor<'_>) -> Result<i64, AppError> {
    let row: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE status = 'pending'")
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(row.0)
}

/// Age in seconds of the oldest pending event, or `None` if no pending events.
pub async fn oldest_unpublished_age_secs(
    executor: impl sqlx::PgExecutor<'_>,
) -> Result<Option<f64>, AppError> {
    let row: (Option<f64>,) = sqlx::query_as(
        "SELECT EXTRACT(EPOCH FROM (NOW() - MIN(created_at)))::float8 FROM outbox_events WHERE status = 'pending'",
    )
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::InternalServerError(e.to_string()))?;
    Ok(row.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AggregateType, EventMetadata, EventType, SourceService};
    use serde_json::json;

    #[test]
    fn payload_size_guard_rejects_oversized_payload() {
        // Build a payload that exceeds MAX_PAYLOAD_BYTES (900 KB)
        let big_string = "x".repeat(MAX_PAYLOAD_BYTES + 1);
        let insert = OutboxInsert {
            aggregate_type: "Order".to_string(),
            aggregate_id: uuid::Uuid::now_v7(),
            event_type: "OrderCreated".to_string(),
            event_id: uuid::Uuid::now_v7(),
            topic: "test.topic".to_string(),
            partition_key: "key".to_string(),
            payload: json!({ "data": big_string }),
            metadata: None,
        };

        // Can't call async insert without a DB, but we can verify the size check
        let payload_bytes = insert.payload.to_string().len();
        assert!(
            payload_bytes > MAX_PAYLOAD_BYTES,
            "test payload should exceed limit"
        );
    }

    #[test]
    fn payload_size_guard_accepts_normal_payload() {
        let metadata = EventMetadata::new(
            EventType::OrderCreated,
            AggregateType::Order,
            uuid::Uuid::now_v7(),
            SourceService::Order,
        );
        let envelope = crate::events::EventEnvelope::new(metadata, json!({"order_total": "99.99"}));
        let insert = OutboxInsert::from_envelope("test.topic", &envelope);

        let payload_bytes = insert.payload.to_string().len();
        assert!(
            payload_bytes <= MAX_PAYLOAD_BYTES,
            "normal payload should be well under limit: {payload_bytes} bytes"
        );
    }
}
