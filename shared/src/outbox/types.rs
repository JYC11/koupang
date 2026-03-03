use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::errors::AppError;
use crate::events::EventEnvelope;

// ── Outbox event (DB row) ────────────────────────────────────────────

/// Status of an outbox event in its lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "VARCHAR", rename_all = "lowercase")]
pub enum OutboxStatus {
    Pending,
    Published,
    Failed,
}

impl std::fmt::Display for OutboxStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "pending"),
            Self::Published => write!(f, "published"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// A row from the `outbox_events` table.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OutboxEvent {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub event_id: Uuid,
    pub topic: String,
    pub partition_key: String,
    pub payload: serde_json::Value,
    pub metadata: Option<serde_json::Value>,
    pub status: OutboxStatus,
    pub published_at: Option<DateTime<Utc>>,
    pub locked_by: Option<String>,
    pub locked_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub max_retries: i32,
    pub last_error: Option<String>,
    pub next_retry_at: DateTime<Utc>,
}

// ── Insert DTO ───────────────────────────────────────────────────────

/// Data required to insert a new outbox event.
/// Use `OutboxInsert::from_envelope()` for the common case.
pub struct OutboxInsert {
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub event_id: Uuid,
    pub topic: String,
    pub partition_key: String,
    pub payload: serde_json::Value,
    pub metadata: Option<serde_json::Value>,
}

impl OutboxInsert {
    /// Build from an `EventEnvelope` + topic name.
    ///
    /// Maps envelope fields to outbox columns:
    /// - `aggregate_type`, `aggregate_id`, `event_type`, `event_id` from metadata
    /// - `partition_key` = `aggregate_id` as string
    /// - `payload` = serialized envelope (metadata + payload together)
    pub fn from_envelope(topic: &str, envelope: &EventEnvelope) -> Self {
        Self {
            aggregate_type: envelope.metadata.aggregate_type.to_string(),
            aggregate_id: envelope.metadata.aggregate_id,
            event_type: envelope.metadata.event_type.to_string(),
            event_id: envelope.metadata.event_id,
            topic: topic.to_string(),
            partition_key: envelope.partition_key(),
            payload: serde_json::to_value(envelope).expect("EventEnvelope is always serializable"),
            metadata: None,
        }
    }

    /// Attach trace context metadata (call `capture_trace_context()` to produce the value).
    pub fn with_metadata(mut self, metadata: Option<serde_json::Value>) -> Self {
        self.metadata = metadata;
        self
    }
}

// ── Failure escalation ───────────────────────────────────────────────

/// Called when an outbox event exhausts all retries and transitions to `failed`.
///
/// Implementations can log, alert, push to a DLQ topic, or trigger manual review.
/// The relay ships with `LogFailureEscalation` as the default.
#[async_trait::async_trait]
pub trait FailureEscalation: Send + Sync {
    async fn on_permanent_failure(&self, event: &OutboxEvent) -> Result<(), AppError>;
}

/// Default escalation: emits a structured error log.
pub struct LogFailureEscalation;

#[async_trait::async_trait]
impl FailureEscalation for LogFailureEscalation {
    async fn on_permanent_failure(&self, event: &OutboxEvent) -> Result<(), AppError> {
        tracing::error!(
            event_id = %event.event_id,
            event_type = %event.event_type,
            aggregate_type = %event.aggregate_type,
            aggregate_id = %event.aggregate_id,
            topic = %event.topic,
            retry_count = event.retry_count,
            last_error = event.last_error.as_deref().unwrap_or("unknown"),
            "Outbox event permanently failed after exhausting all retries"
        );
        Ok(())
    }
}

// ── Trace context propagation ────────────────────────────────────────

/// Captures the current tracing span context into a JSON map
/// suitable for the outbox `metadata` column.
///
/// Returns `None` if there is no active span (avoids storing empty metadata).
/// The relay later injects this as Kafka headers so downstream consumers
/// can continue the same distributed trace.
///
/// Currently captures tracing-level context (span ID, name, target, module).
/// Will be upgraded to full W3C traceparent/tracestate propagation once
/// the OpenTelemetry SDK is integrated (bd-8fc).
pub fn capture_trace_context() -> Option<serde_json::Value> {
    use tracing::Span;

    let span = Span::current();
    let span_meta = span.metadata()?;
    let span_id = span.id().map(|id| id.into_u64());

    Some(serde_json::json!({
        "span_name": span_meta.name(),
        "span_target": span_meta.target(),
        "span_id": span_id,
        "span_module_path": span_meta.module_path(),
    }))
}

// ── Outbox metrics snapshot ──────────────────────────────────────────

/// Point-in-time metrics for the outbox table.
#[derive(Debug, Clone, Default)]
pub struct OutboxMetrics {
    pub pending_count: i64,
    pub failed_count: i64,
    pub published_count: i64,
    pub oldest_pending_age_secs: Option<f64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AggregateType, EventMetadata, EventType, SourceService};
    use serde_json::json;

    fn test_envelope() -> EventEnvelope {
        let metadata = EventMetadata::new(
            EventType::OrderCreated,
            AggregateType::Order,
            Uuid::now_v7(),
            SourceService::Order,
        );
        EventEnvelope::new(metadata, json!({"order_total": "99.99"}))
    }

    #[test]
    fn outbox_insert_from_envelope_maps_fields() {
        let envelope = test_envelope();
        let insert = OutboxInsert::from_envelope("orders.events", &envelope);

        assert_eq!(insert.topic, "orders.events");
        assert_eq!(insert.aggregate_type, "Order");
        assert_eq!(insert.aggregate_id, envelope.metadata.aggregate_id);
        assert_eq!(insert.event_type, "OrderCreated");
        assert_eq!(insert.event_id, envelope.metadata.event_id);
        assert_eq!(
            insert.partition_key,
            envelope.metadata.aggregate_id.to_string()
        );
        assert!(insert.metadata.is_none());

        // payload is the full serialized envelope
        let payload_envelope: EventEnvelope =
            serde_json::from_value(insert.payload).expect("payload should deserialize back");
        assert_eq!(
            payload_envelope.metadata.event_type,
            EventType::OrderCreated
        );
    }

    #[test]
    fn outbox_insert_with_metadata() {
        let envelope = test_envelope();
        let trace = json!({"trace_id": "abc123", "span_id": "def456"});
        let insert = OutboxInsert::from_envelope("orders.events", &envelope)
            .with_metadata(Some(trace.clone()));

        assert_eq!(insert.metadata, Some(trace));
    }

    #[test]
    fn outbox_status_display() {
        assert_eq!(OutboxStatus::Pending.to_string(), "pending");
        assert_eq!(OutboxStatus::Published.to_string(), "published");
        assert_eq!(OutboxStatus::Failed.to_string(), "failed");
    }

    #[test]
    fn log_failure_escalation_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<LogFailureEscalation>();
    }

    #[tokio::test]
    async fn log_failure_escalation_returns_ok() {
        let escalation = LogFailureEscalation;
        let event = OutboxEvent {
            id: Uuid::now_v7(),
            created_at: Utc::now(),
            aggregate_type: "Order".to_string(),
            aggregate_id: Uuid::now_v7(),
            event_type: "OrderCreated".to_string(),
            event_id: Uuid::now_v7(),
            topic: "orders.events".to_string(),
            partition_key: Uuid::now_v7().to_string(),
            payload: json!({}),
            metadata: None,
            status: OutboxStatus::Failed,
            published_at: None,
            locked_by: None,
            locked_at: None,
            retry_count: 10,
            max_retries: 10,
            last_error: Some("Kafka unavailable".to_string()),
            next_retry_at: Utc::now(),
        };

        let result = escalation.on_permanent_failure(&event).await;
        assert!(result.is_ok());
    }

    #[test]
    fn capture_trace_context_returns_some_in_span() {
        // A tracing subscriber must be active for Span::metadata() to return Some
        let subscriber = tracing_subscriber::fmt().with_test_writer().finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let span = tracing::info_span!("test_span");
        let _enter = span.enter();
        let ctx = capture_trace_context();
        assert!(ctx.is_some());
        let map = ctx.unwrap();
        assert_eq!(map.get("span_name").unwrap(), "test_span");
        assert!(map.get("span_id").is_some());
        assert!(map.get("span_module_path").is_some());
    }

    #[test]
    fn capture_trace_context_returns_none_without_span() {
        // No subscriber, no active span → None
        let ctx = capture_trace_context();
        assert!(ctx.is_none());
    }

    #[test]
    fn outbox_metrics_default() {
        let metrics = OutboxMetrics::default();
        assert_eq!(metrics.pending_count, 0);
        assert_eq!(metrics.failed_count, 0);
        assert_eq!(metrics.published_count, 0);
        assert!(metrics.oldest_pending_age_secs.is_none());
    }
}
