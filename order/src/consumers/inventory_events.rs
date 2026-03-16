use crate::orders::repository;
use crate::orders::value_objects::{OrderId, OrderStatus};
use shared::errors::AppError;
use shared::events::EventEnvelope;
use sqlx::PgConnection;

/// Handle InventoryReserved: transition order to InventoryReserved.
/// Called within the consumer's transaction — no own tx or idempotency check needed.
pub async fn handle_inventory_reserved(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = OrderId::new(envelope.payload_uuid("order_id")?);

    let order = repository::get_order_by_id(&mut *tx, order_id).await?;
    order
        .status
        .transition_to(&OrderStatus::InventoryReserved)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    repository::update_order_status(&mut *tx, order_id, &OrderStatus::InventoryReserved, None).await
}

/// Handle InventoryReservationFailed: cancel the order.
pub async fn handle_inventory_reservation_failed(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = OrderId::new(envelope.payload_uuid("order_id")?);

    let order = repository::get_order_by_id(&mut *tx, order_id).await?;
    order
        .status
        .transition_to(&OrderStatus::Cancelled)
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let reason = envelope.payload["reason"]
        .as_str()
        .unwrap_or("Inventory reservation failed");

    repository::update_order_status(&mut *tx, order_id, &OrderStatus::Cancelled, Some(reason)).await
}
