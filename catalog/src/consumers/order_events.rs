use crate::inventory::service;
use shared::db::PgPool;
use shared::errors::AppError;
use shared::events::EventEnvelope;
use shared::events::consumer::HandlerError;
use sqlx::PgConnection;
use uuid::Uuid;

/// Handle OrderCreated: reserve inventory for each SKU in the order.
/// Called within the consumer's transaction.
pub async fn handle_order_created(
    tx: &mut PgConnection,
    pool: &PgPool,
    envelope: &EventEnvelope,
) -> Result<(), HandlerError> {
    let order_id = envelope
        .payload_uuid("order_id")
        .map_err(|e| HandlerError::permanent(e.to_string()))?;
    let buyer_id = envelope
        .payload_uuid("buyer_id")
        .map_err(|e| HandlerError::permanent(e.to_string()))?;
    let total_amount = envelope.payload["total_amount"]
        .as_str()
        .unwrap_or("0")
        .to_string();
    let currency = envelope.payload["currency"]
        .as_str()
        .unwrap_or("USD")
        .to_string();

    let items = parse_items(&envelope.payload)?;

    // Try reserving on the consumer's tx.
    // On failure: write InventoryReservationFailed on separate pool tx (survives rollback),
    // then return Err so the consumer rolls back partial reservation writes.
    match service::reserve_for_order_on_tx(
        tx,
        pool,
        order_id,
        buyer_id,
        &total_amount,
        &currency,
        &items,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!(order_id = %order_id, error = %e, "Inventory reservation failed");
            // Failure event already written on pool. Return Permanent so consumer
            // rolls back the tx (no partial reserves committed) and sends to DLQ.
            Err(HandlerError::permanent(format!(
                "Inventory reservation failed: {e}"
            )))
        }
    }
}

/// Handle OrderCancelled: release all inventory reservations for the order.
pub async fn handle_order_cancelled(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = envelope.payload_uuid("order_id")?;

    // Release may find no reservations (order cancelled before inventory reserved) — that's OK
    if let Err(e) = service::release_for_order_on_tx(tx, order_id).await {
        tracing::warn!(order_id = %order_id, error = %e, "Failed to release inventory (may not have been reserved)");
    }

    Ok(())
}

fn parse_items(payload: &serde_json::Value) -> Result<Vec<(Uuid, i32)>, HandlerError> {
    payload["items"]
        .as_array()
        .ok_or_else(|| HandlerError::permanent("Missing items array in OrderCreated payload"))?
        .iter()
        .map(|item| {
            let sku_id = item["sku_id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| HandlerError::permanent("Invalid sku_id in item"))?;
            let quantity = item["quantity"]
                .as_i64()
                .ok_or_else(|| HandlerError::permanent("Missing quantity in item"))?
                as i32;
            Ok((sku_id, quantity))
        })
        .collect()
}
