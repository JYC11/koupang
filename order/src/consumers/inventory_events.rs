use crate::orders::repository;
use crate::orders::value_objects::{OrderId, OrderStatus};
use shared::db::PgPool;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::events::EventEnvelope;
use shared::outbox::{is_event_processed, mark_event_processed};

const CONSUMER_GROUP: &str = "order-service";

/// Handle InventoryReserved: transition order to InventoryReserved.
pub async fn handle_inventory_reserved(
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

    let order_id: uuid::Uuid = envelope.payload["order_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("Missing or invalid order_id in InventoryReserved payload")?;

    let order_id = OrderId::new(order_id);

    with_transaction(pool, |tx| {
        Box::pin(async move {
            let order = repository::get_order_by_id(tx.as_executor(), order_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            order
                .status
                .transition_to(&OrderStatus::InventoryReserved)
                .map_err(|e| TxError::Other(e.to_string()))?;

            repository::update_order_status(
                tx.as_executor(),
                order_id,
                &OrderStatus::InventoryReserved,
                None,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            mark_event_processed(
                tx.as_executor(),
                event_id,
                "InventoryReserved",
                "catalog",
                CONSUMER_GROUP,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| format!("Failed to handle InventoryReserved: {e}"))
}

/// Handle InventoryReservationFailed: cancel the order.
pub async fn handle_inventory_reservation_failed(
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

    let order_id: uuid::Uuid = envelope.payload["order_id"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or("Missing or invalid order_id in InventoryReservationFailed payload")?;

    let reason = envelope.payload["reason"]
        .as_str()
        .unwrap_or("Inventory reservation failed");

    let order_id = OrderId::new(order_id);

    with_transaction(pool, |tx| {
        let reason = reason.to_string();
        Box::pin(async move {
            repository::update_order_status(
                tx.as_executor(),
                order_id,
                &OrderStatus::Cancelled,
                Some(&reason),
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            mark_event_processed(
                tx.as_executor(),
                event_id,
                "InventoryReservationFailed",
                "catalog",
                CONSUMER_GROUP,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| format!("Failed to handle InventoryReservationFailed: {e}"))
}
