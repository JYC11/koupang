use crate::AppState;
use crate::gateway::traits::PaymentGateway;
use crate::ledger::repository as ledger_repo;
use crate::ledger::value_objects::PaymentState;
use crate::payments::service;
use shared::db::PgPool;
use shared::events::EventEnvelope;
use shared::outbox::{is_event_processed, mark_event_processed};

const CONSUMER_GROUP: &str = "payment-service";

/// Handle OrderConfirmed: capture the authorized payment.
pub async fn handle_order_confirmed(
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

    service::capture_payment(state, gateway, order_id)
        .await
        .map_err(|e| format!("Failed to capture payment: {e}"))?;

    mark_event_processed(pool, event_id, "OrderConfirmed", "order", CONSUMER_GROUP)
        .await
        .map_err(|e| format!("Failed to mark event processed: {e}"))?;

    Ok(())
}

/// Handle OrderCancelled: void if authorized, refund if captured.
pub async fn handle_order_cancelled(
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

    let transactions = ledger_repo::list_transactions_by_order(pool, order_id)
        .await
        .map_err(|e| format!("Failed to list transactions: {e}"))?;

    let payment_state = ledger_repo::derive_payment_state(&transactions);

    match payment_state {
        PaymentState::Authorized => {
            service::void_payment(state, gateway, order_id)
                .await
                .map_err(|e| format!("Failed to void payment: {e}"))?;
        }
        PaymentState::New | PaymentState::Failed => {
            // No payment to reverse
        }
        _ => {
            tracing::warn!(
                order_id = %order_id,
                payment_state = ?payment_state,
                "OrderCancelled received but payment in unexpected state"
            );
        }
    }

    mark_event_processed(pool, event_id, "OrderCancelled", "order", CONSUMER_GROUP)
        .await
        .map_err(|e| format!("Failed to mark event processed: {e}"))?;

    Ok(())
}
