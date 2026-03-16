use crate::gateway::traits::PaymentGateway;
use crate::ledger::repository as ledger_repo;
use crate::ledger::value_objects::PaymentState;
use crate::payments::service;
use shared::db::PgPool;
use shared::errors::AppError;
use shared::events::EventEnvelope;
use sqlx::PgConnection;

/// Handle OrderConfirmed: capture the authorized payment.
/// Reads on pool (released before gateway call), writes on tx.
pub async fn handle_order_confirmed(
    tx: &mut PgConnection,
    pool: &PgPool,
    gateway: &dyn PaymentGateway,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = envelope.payload_uuid("order_id")?;
    service::capture_payment_on_tx(pool, tx, gateway, order_id).await
}

/// Handle OrderCancelled: void if authorized.
/// Reads on pool (released before gateway call), writes on tx.
pub async fn handle_order_cancelled(
    tx: &mut PgConnection,
    pool: &PgPool,
    gateway: &dyn PaymentGateway,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = envelope.payload_uuid("order_id")?;

    // Read payment state on pool (doesn't hold consumer's tx during read)
    let transactions = ledger_repo::list_transactions_by_order(pool, order_id).await?;
    let payment_state = ledger_repo::derive_payment_state(&transactions);

    match payment_state {
        PaymentState::Authorized => {
            service::void_payment_on_tx(pool, tx, gateway, order_id).await?;
        }
        PaymentState::New | PaymentState::Failed => {}
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
