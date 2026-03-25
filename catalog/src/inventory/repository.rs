use crate::inventory::entities::{InventoryReservationEntity, SkuAvailabilityRow};
use shared::errors::AppError;
use sqlx::PgConnection;
use uuid::Uuid;

/// Reserve inventory for an order. Atomically:
/// 1. Check available quantity (stock - reserved >= requested)
/// 2. Increment reserved_quantity on the SKU
/// 3. Insert a reservation record
pub async fn reserve_inventory(
    tx: &mut PgConnection,
    order_id: Uuid,
    sku_id: Uuid,
    quantity: i32,
) -> Result<(), AppError> {
    // Atomic check-and-reserve: only succeeds if enough unreserved stock
    let result = sqlx::query(
        "UPDATE skus SET reserved_quantity = reserved_quantity + $1, updated_at = NOW() \
         WHERE id = $2 AND deleted_at IS NULL \
         AND (stock_quantity - reserved_quantity) >= $1",
    )
    .bind(quantity)
    .bind(sku_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to reserve inventory: {e}")))?;

    if result.rows_affected() == 0 {
        return Err(AppError::BadRequest(format!(
            "Insufficient stock for SKU {sku_id}"
        )));
    }

    sqlx::query(
        "INSERT INTO inventory_reservations (order_id, sku_id, quantity) VALUES ($1, $2, $3)",
    )
    .bind(order_id)
    .bind(sku_id)
    .bind(quantity)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to insert reservation: {e}")))?;

    Ok(())
}

/// Release a reservation (order cancelled). Decrements reserved_quantity and marks released.
pub async fn release_reservation(
    tx: &mut PgConnection,
    order_id: Uuid,
    sku_id: Uuid,
) -> Result<(), AppError> {
    let reservation: InventoryReservationEntity = sqlx::query_as(
        "SELECT * FROM inventory_reservations \
         WHERE order_id = $1 AND sku_id = $2 AND status = 'reserved'",
    )
    .bind(order_id)
    .bind(sku_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::NotFound(format!("Reservation not found: {e}")))?;

    sqlx::query(
        "UPDATE skus SET reserved_quantity = reserved_quantity - $1, updated_at = NOW() \
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(reservation.quantity)
    .bind(sku_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to release reserved stock: {e}")))?;

    sqlx::query(
        "UPDATE inventory_reservations SET status = 'released', released_at = NOW() \
         WHERE id = $1",
    )
    .bind(reservation.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to release reservation: {e}")))?;

    Ok(())
}

/// Confirm a reservation (order confirmed/shipped). Decrements both stock and reserved.
pub async fn confirm_reservation(
    tx: &mut PgConnection,
    order_id: Uuid,
    sku_id: Uuid,
) -> Result<(), AppError> {
    let reservation: InventoryReservationEntity = sqlx::query_as(
        "SELECT * FROM inventory_reservations \
         WHERE order_id = $1 AND sku_id = $2 AND status = 'reserved'",
    )
    .bind(order_id)
    .bind(sku_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::NotFound(format!("Reservation not found: {e}")))?;

    // Deduct from both stock and reserved. If admin reduced stock below reserved
    // quantity while order was in flight, the CHECK (stock_quantity >= 0) constraint
    // fires — catch it and return a meaningful error instead of a generic 500.
    sqlx::query(
        "UPDATE skus SET stock_quantity = stock_quantity - $1, \
         reserved_quantity = reserved_quantity - $1, updated_at = NOW() \
         WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(reservation.quantity)
    .bind(sku_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        if let Some(db_err) = e.as_database_error() {
            if db_err.constraint() == Some("chk_skus_stock") {
                tracing::error!(
                    order_id = %order_id,
                    sku_id = %sku_id,
                    quantity = reservation.quantity,
                    "Stock reduced below reservation — admin likely edited stock while order was in flight"
                );
                return AppError::BadRequest(format!(
                    "Cannot confirm reservation for SKU {sku_id}: stock was reduced below reserved quantity"
                ));
            }
        }
        AppError::InternalServerError(format!("Failed to confirm stock: {e}"))
    })?;

    sqlx::query(
        "UPDATE inventory_reservations SET status = 'confirmed', confirmed_at = NOW() \
         WHERE id = $1",
    )
    .bind(reservation.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to confirm reservation: {e}")))?;

    Ok(())
}

/// Release all reservations for an order (multi-SKU cancellation).
pub async fn release_all_reservations(
    tx: &mut PgConnection,
    order_id: Uuid,
) -> Result<(), AppError> {
    let reservations: Vec<InventoryReservationEntity> = sqlx::query_as(
        "SELECT * FROM inventory_reservations \
         WHERE order_id = $1 AND status = 'reserved'",
    )
    .bind(order_id)
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to find reservations: {e}")))?;

    for reservation in &reservations {
        release_reservation(&mut *tx, order_id, reservation.sku_id).await?;
    }

    Ok(())
}

/// Get availability for a specific SKU.
pub async fn get_sku_availability(
    tx: &mut PgConnection,
    sku_id: Uuid,
) -> Result<SkuAvailabilityRow, AppError> {
    sqlx::query_as::<_, SkuAvailabilityRow>("SELECT * FROM sku_availability WHERE sku_id = $1")
        .bind(sku_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| AppError::NotFound(format!("SKU availability not found: {e}")))
}

/// Get reservation for an order + SKU.
pub async fn get_reservation(
    tx: &mut PgConnection,
    order_id: Uuid,
    sku_id: Uuid,
) -> Result<Option<InventoryReservationEntity>, AppError> {
    sqlx::query_as("SELECT * FROM inventory_reservations WHERE order_id = $1 AND sku_id = $2")
        .bind(order_id)
        .bind(sku_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to get reservation: {e}")))
}
