use crate::orders::repository;
use crate::orders::value_objects::{OrderId, OrderStatus};
use shared::db::PgPool;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use shared::outbox::{OutboxInsert, insert_outbox_event, is_event_processed, mark_event_processed};

const CONSUMER_GROUP: &str = "order-service";

/// Handle PaymentAuthorized: transition to PaymentAuthorized, then auto-confirm.
/// Writes OrderConfirmed to outbox.
pub async fn handle_payment_authorized(
    pool: &PgPool,
    envelope: &EventEnvelope,
) -> Result<(), String> {
    let event_id = envelope.metadata.event_id;

    if is_event_processed(pool, event_id, CONSUMER_GROUP)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }

    let order_id = extract_order_id(envelope, "PaymentAuthorized")?;

    with_transaction(pool, |tx| {
        Box::pin(async move {
            let order = repository::get_order_by_id(tx.as_executor(), order_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            order
                .status
                .transition_to(&OrderStatus::PaymentAuthorized)
                .map_err(|e| TxError::Other(e.to_string()))?;

            repository::update_order_status(
                tx.as_executor(),
                order_id,
                &OrderStatus::PaymentAuthorized,
                None,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            // Auto-transition: PaymentAuthorized → Confirmed.
            repository::update_order_status(
                tx.as_executor(),
                order_id,
                &OrderStatus::Confirmed,
                None,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            write_order_outbox(
                tx.as_executor(),
                order_id,
                EventType::OrderConfirmed,
                serde_json::json!({
                    "order_id": order_id.value(),
                    "buyer_id": order.buyer_id,
                }),
                Some(event_id),
            )
            .await?;

            mark_event_processed(
                tx.as_executor(),
                event_id,
                "PaymentAuthorized",
                "payment",
                CONSUMER_GROUP,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| format!("Failed to handle PaymentAuthorized: {e}"))
}

/// Handle PaymentFailed: cancel the order.
pub async fn handle_payment_failed(pool: &PgPool, envelope: &EventEnvelope) -> Result<(), String> {
    cancel_order_on_payment_failure(pool, envelope, "PaymentFailed", "Payment failed").await
}

/// Handle PaymentTimedOut: cancel the order.
pub async fn handle_payment_timed_out(
    pool: &PgPool,
    envelope: &EventEnvelope,
) -> Result<(), String> {
    cancel_order_on_payment_failure(pool, envelope, "PaymentTimedOut", "Payment timed out").await
}

// ── Helpers ─────────────────────────────────────────────────

/// Shared logic for PaymentFailed and PaymentTimedOut — both cancel the order.
async fn cancel_order_on_payment_failure(
    pool: &PgPool,
    envelope: &EventEnvelope,
    event_type_name: &str,
    default_reason: &str,
) -> Result<(), String> {
    let event_id = envelope.metadata.event_id;

    if is_event_processed(pool, event_id, CONSUMER_GROUP)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }

    let order_id = extract_order_id(envelope, event_type_name)?;

    let reason = envelope.payload["reason"]
        .as_str()
        .unwrap_or(default_reason);

    with_transaction(pool, |tx| {
        let reason = reason.to_string();
        let event_type_name = event_type_name.to_string();
        Box::pin(async move {
            repository::update_order_status(
                tx.as_executor(),
                order_id,
                &OrderStatus::Cancelled,
                Some(&reason),
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            let order = repository::get_order_by_id(tx.as_executor(), order_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            write_order_outbox(
                tx.as_executor(),
                order_id,
                EventType::OrderCancelled,
                serde_json::json!({
                    "order_id": order_id.value(),
                    "buyer_id": order.buyer_id,
                    "reason": reason,
                }),
                Some(event_id),
            )
            .await?;

            mark_event_processed(
                tx.as_executor(),
                event_id,
                &event_type_name,
                "payment",
                CONSUMER_GROUP,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| format!("Failed to handle {event_type_name}: {e}"))
}

fn extract_order_id(envelope: &EventEnvelope, event_name: &str) -> Result<OrderId, String> {
    let uuid: uuid::Uuid = envelope.payload["order_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or(format!(
            "Missing or invalid order_id in {event_name} payload"
        ))?;
    Ok(OrderId::new(uuid))
}

async fn write_order_outbox(
    tx: &mut sqlx::PgConnection,
    order_id: OrderId,
    event_type: EventType,
    payload: serde_json::Value,
    causation_id: Option<uuid::Uuid>,
) -> Result<(), TxError> {
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
    insert_outbox_event(tx, &insert)
        .await
        .map(|_| ())
        .map_err(|e| TxError::Other(e.to_string()))
}
