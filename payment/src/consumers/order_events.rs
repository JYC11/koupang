use crate::gateway::traits::PaymentGateway;
use crate::ledger::repository as ledger_repo;
use crate::ledger::value_objects::PaymentState;
use crate::payments::service;
use shared::errors::AppError;
use shared::events::EventEnvelope;
use sqlx::PgConnection;

/// Handle OrderConfirmed: capture the authorized payment.
pub async fn handle_order_confirmed(
    tx: &mut PgConnection,
    gateway: &dyn PaymentGateway,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = extract_order_id(envelope)?;
    service::capture_payment_on_tx(tx, gateway, order_id).await
}

/// Handle OrderCancelled: void if authorized.
pub async fn handle_order_cancelled(
    tx: &mut PgConnection,
    gateway: &dyn PaymentGateway,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = extract_order_id(envelope)?;

    let transactions = ledger_repo::list_transactions_by_order(&mut *tx, order_id).await?;
    let payment_state = ledger_repo::derive_payment_state(&transactions);

    match payment_state {
        PaymentState::Authorized => {
            service::void_payment_on_tx(tx, gateway, order_id).await?;
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

    Ok(())
}

fn extract_order_id(envelope: &EventEnvelope) -> Result<uuid::Uuid, AppError> {
    envelope.payload["order_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::BadRequest("Missing or invalid order_id".to_string()))
}
