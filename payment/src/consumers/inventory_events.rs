use crate::AppState;
use crate::gateway::traits::PaymentGateway;
use crate::payments::service;
use rust_decimal::Decimal;
use shared::db::PgPool;
use shared::events::EventEnvelope;
use shared::outbox::{is_event_processed, mark_event_processed};

const CONSUMER_GROUP: &str = "payment-service";

/// Handle InventoryReserved: authorize payment via gateway.
pub async fn handle_inventory_reserved(
    pool: &PgPool,
    state: &AppState,
    gateway: &dyn PaymentGateway,
    envelope: &EventEnvelope,
) -> Result<(), String> {
    let event_id = envelope.metadata.event_id;

    if is_event_processed(pool, event_id, CONSUMER_GROUP)
        .await
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }

    let order_id: uuid::Uuid = envelope.payload["order_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("Missing or invalid order_id")?;

    let total_amount: Decimal = envelope.payload["total_amount"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("Missing or invalid total_amount")?;

    let currency = envelope.payload["currency"].as_str().unwrap_or("USD");

    service::authorize_payment(state, gateway, order_id, total_amount, currency)
        .await
        .map_err(|e| format!("Failed to authorize payment: {e}"))?;

    mark_event_processed(
        pool,
        event_id,
        "InventoryReserved",
        "catalog",
        CONSUMER_GROUP,
    )
    .await
    .map_err(|e| format!("Failed to mark event processed: {e}"))?;

    Ok(())
}
