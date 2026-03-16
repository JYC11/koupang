use crate::orders::repository;
use crate::orders::value_objects::{OrderId, OrderStatus};
use shared::errors::AppError;
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use shared::outbox::{OutboxInsert, insert_outbox_event};
use sqlx::PgConnection;

/// Handle PaymentAuthorized: transition to PaymentAuthorized, then auto-confirm.
/// Writes OrderConfirmed to outbox.
pub async fn handle_payment_authorized(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = extract_order_id(envelope, "PaymentAuthorized")?;

    let order = repository::get_order_by_id(&mut *tx, order_id).await?;
    order
        .status
        .transition_to(&OrderStatus::PaymentAuthorized)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    repository::update_order_status(&mut *tx, order_id, &OrderStatus::PaymentAuthorized, None)
        .await?;

    // Auto-transition: PaymentAuthorized → Confirmed.
    repository::update_order_status(&mut *tx, order_id, &OrderStatus::Confirmed, None).await?;

    write_order_outbox(
        &mut *tx,
        order_id,
        EventType::OrderConfirmed,
        serde_json::json!({
            "order_id": order_id.value(),
            "buyer_id": order.buyer_id,
        }),
        Some(envelope.metadata.event_id),
    )
    .await
}

/// Handle PaymentFailed: cancel the order.
pub async fn handle_payment_failed(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    cancel_order_on_payment_failure(tx, envelope, "Payment failed").await
}

/// Handle PaymentTimedOut: cancel the order.
pub async fn handle_payment_timed_out(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    cancel_order_on_payment_failure(tx, envelope, "Payment timed out").await
}

// ── Helpers ─────────────────────────────────────────────────

async fn cancel_order_on_payment_failure(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
    default_reason: &str,
) -> Result<(), AppError> {
    let order_id = extract_order_id(envelope, "PaymentFailure")?;
    let reason = envelope.payload["reason"]
        .as_str()
        .unwrap_or(default_reason);

    repository::update_order_status(&mut *tx, order_id, &OrderStatus::Cancelled, Some(reason))
        .await?;

    let order = repository::get_order_by_id(&mut *tx, order_id).await?;

    write_order_outbox(
        &mut *tx,
        order_id,
        EventType::OrderCancelled,
        serde_json::json!({
            "order_id": order_id.value(),
            "buyer_id": order.buyer_id,
            "reason": reason,
        }),
        Some(envelope.metadata.event_id),
    )
    .await
}

fn extract_order_id(envelope: &EventEnvelope, event_name: &str) -> Result<OrderId, AppError> {
    let uuid: uuid::Uuid = envelope.payload["order_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Missing or invalid order_id in {event_name} payload"
            ))
        })?;
    Ok(OrderId::new(uuid))
}

async fn write_order_outbox(
    tx: &mut PgConnection,
    order_id: OrderId,
    event_type: EventType,
    payload: serde_json::Value,
    causation_id: Option<uuid::Uuid>,
) -> Result<(), AppError> {
    let mut metadata = EventMetadata::new(
        event_type,
        AggregateType::Order,
        order_id.value(),
        SourceService::Order,
    );
    if let Some(cause) = causation_id {
        metadata = metadata.with_causation_id(cause);
    }
    let envelope = EventEnvelope::new(metadata, payload);
    let insert = OutboxInsert::from_envelope("orders.events", &envelope);
    insert_outbox_event(tx, &insert).await.map(|_| ())
}
