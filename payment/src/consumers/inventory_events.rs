use crate::gateway::traits::PaymentGateway;
use crate::payments::service;
use rust_decimal::Decimal;
use shared::errors::AppError;
use shared::events::EventEnvelope;
use sqlx::PgConnection;

/// Handle InventoryReserved: authorize payment via gateway.
/// Called within the consumer's transaction.
pub async fn handle_inventory_reserved(
    tx: &mut PgConnection,
    gateway: &dyn PaymentGateway,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id: uuid::Uuid = envelope.payload["order_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::BadRequest("Missing or invalid order_id".to_string()))?;

    let total_amount: Decimal = envelope.payload["total_amount"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::BadRequest("Missing or invalid total_amount".to_string()))?;

    let currency = envelope.payload["currency"].as_str().unwrap_or("USD");

    service::authorize_payment_on_tx(tx, gateway, order_id, total_amount, currency).await
}
