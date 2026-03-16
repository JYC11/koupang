use crate::inventory::service;
use shared::db::PgPool;
use shared::errors::AppError;
use shared::events::EventEnvelope;
use sqlx::PgConnection;
use uuid::Uuid;

/// Handle OrderCreated: reserve inventory for each SKU in the order.
/// Called within the consumer's transaction.
pub async fn handle_order_created(
    tx: &mut PgConnection,
    pool: &PgPool,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = extract_uuid(&envelope.payload, "order_id")?;
    let buyer_id = extract_uuid(&envelope.payload, "buyer_id")?;
    let total_amount = envelope.payload["total_amount"]
        .as_str()
        .unwrap_or("0")
        .to_string();
    let currency = envelope.payload["currency"]
        .as_str()
        .unwrap_or("USD")
        .to_string();

    let items: Vec<(Uuid, i32)> = envelope.payload["items"]
        .as_array()
        .ok_or_else(|| {
            AppError::BadRequest("Missing items array in OrderCreated payload".to_string())
        })?
        .iter()
        .map(|item| {
            let sku_id = item["sku_id"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| AppError::BadRequest("Invalid sku_id in item".to_string()))?;
            let quantity = item["quantity"]
                .as_i64()
                .ok_or_else(|| AppError::BadRequest("Missing quantity in item".to_string()))?
                as i32;
            Ok((sku_id, quantity))
        })
        .collect::<Result<Vec<_>, AppError>>()?;

    // reserve_for_order_on_tx writes InventoryReserved or InventoryReservationFailed
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
            // Reservation failed — failure event already written on a separate tx.
            // Log but don't propagate error (consumer should commit to mark processed).
            tracing::warn!(order_id = %order_id, error = %e, "Inventory reservation failed");
            Ok(())
        }
    }
}

/// Handle OrderCancelled: release all inventory reservations for the order.
pub async fn handle_order_cancelled(
    tx: &mut PgConnection,
    envelope: &EventEnvelope,
) -> Result<(), AppError> {
    let order_id = extract_uuid(&envelope.payload, "order_id")?;

    // Release may find no reservations (order cancelled before inventory reserved) — that's OK
    if let Err(e) = service::release_for_order_on_tx(tx, order_id).await {
        tracing::warn!(order_id = %order_id, error = %e, "Failed to release inventory (may not have been reserved)");
    }

    Ok(())
}

fn extract_uuid(payload: &serde_json::Value, field: &str) -> Result<Uuid, AppError> {
    payload[field]
        .as_str()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| AppError::BadRequest(format!("Missing or invalid {field} in payload")))
}
