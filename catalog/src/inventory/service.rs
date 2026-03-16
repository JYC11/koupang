use crate::AppState;
use crate::inventory::repository;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use shared::outbox::{OutboxInsert, insert_outbox_event};
use sqlx::PgConnection;
use uuid::Uuid;

/// Item to reserve: (sku_id, quantity)
pub type ReservationItem = (Uuid, i32);

/// Reserve inventory within an existing transaction. Writes InventoryReserved outbox event on success.
/// On failure, writes InventoryReservationFailed outbox event (using a separate transaction for the
/// failure event since the main tx will be rolled back by the consumer).
pub async fn reserve_for_order_on_tx(
    tx: &mut PgConnection,
    pool: &shared::db::PgPool,
    order_id: Uuid,
    buyer_id: Uuid,
    total_amount: &str,
    currency: &str,
    items: &[ReservationItem],
) -> Result<(), AppError> {
    // Try reserving — if any SKU fails, write failure event on a separate tx
    let reserve_result =
        do_reserve_and_write_event(tx, order_id, buyer_id, total_amount, currency, items).await;

    match reserve_result {
        Ok(()) => Ok(()),
        Err(e) => {
            // Write InventoryReservationFailed on a separate transaction
            // (consumer will rollback the main tx on error)
            let reason = e.to_string();
            write_reservation_failed_on_pool(pool, order_id, &reason).await?;
            Err(e)
        }
    }
}

/// Reserve inventory for an order using AppState (creates its own transaction).
pub async fn reserve_for_order(
    state: &AppState,
    order_id: Uuid,
    buyer_id: Uuid,
    total_amount: &str,
    currency: &str,
    items: &[ReservationItem],
) -> Result<(), AppError> {
    let total_amount = total_amount.to_string();
    let currency = currency.to_string();
    let items = items.to_vec();

    let result = with_transaction(&state.pool, |tx| {
        let total_amount = total_amount.clone();
        let currency = currency.clone();
        let items = items.clone();
        Box::pin(async move {
            do_reserve_and_write_event(
                tx.as_executor(),
                order_id,
                buyer_id,
                &total_amount,
                &currency,
                &items,
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await;

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            let reason = e.to_string();
            write_reservation_failed_on_pool(&state.pool, order_id, &reason).await?;
            Err(AppError::BadRequest(reason))
        }
    }
}

/// Release all inventory reservations within an existing transaction.
pub async fn release_for_order_on_tx(
    tx: &mut PgConnection,
    order_id: Uuid,
) -> Result<(), AppError> {
    repository::release_all_reservations(tx, order_id).await
}

/// Release all inventory reservations for a cancelled order (creates own transaction).
pub async fn release_for_order(state: &AppState, order_id: Uuid) -> Result<(), AppError> {
    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            repository::release_all_reservations(tx.as_executor(), order_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to release inventory: {e}")))
}

/// Confirm all reservations for an order (creates own transaction).
pub async fn confirm_for_order(
    state: &AppState,
    order_id: Uuid,
    items: &[ReservationItem],
) -> Result<(), AppError> {
    let items = items.to_vec();
    with_transaction(&state.pool, |tx| {
        let items = items.clone();
        Box::pin(async move {
            for &(sku_id, _) in &items {
                repository::confirm_reservation(tx.as_executor(), order_id, sku_id)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))?;
            }
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to confirm inventory: {e}")))
}

// ── Internal helpers ────────────────────────────────────────

async fn do_reserve_and_write_event(
    tx: &mut PgConnection,
    order_id: Uuid,
    buyer_id: Uuid,
    total_amount: &str,
    currency: &str,
    items: &[ReservationItem],
) -> Result<(), AppError> {
    for &(sku_id, quantity) in items {
        repository::reserve_inventory(&mut *tx, order_id, sku_id, quantity).await?;
    }

    let payload = serde_json::json!({
        "order_id": order_id.to_string(),
        "buyer_id": buyer_id.to_string(),
        "total_amount": total_amount,
        "currency": currency,
        "items": items.iter().map(|(sku_id, qty)| serde_json::json!({
            "sku_id": sku_id.to_string(),
            "quantity": qty,
        })).collect::<Vec<_>>(),
    });
    let metadata = EventMetadata::new(
        EventType::InventoryReserved,
        AggregateType::Inventory,
        order_id,
        SourceService::Catalog,
    );
    let envelope = EventEnvelope::new(metadata, payload);
    let insert = OutboxInsert::from_envelope("catalog.events", &envelope);
    insert_outbox_event(&mut *tx, &insert).await.map(|_| ())
}

async fn write_reservation_failed_on_pool(
    pool: &shared::db::PgPool,
    order_id: Uuid,
    reason: &str,
) -> Result<(), AppError> {
    let reason = reason.to_string();
    with_transaction(pool, |tx| {
        let reason = reason.clone();
        Box::pin(async move {
            let payload = serde_json::json!({
                "order_id": order_id.to_string(),
                "reason": reason,
            });
            let metadata = EventMetadata::new(
                EventType::InventoryReservationFailed,
                AggregateType::Inventory,
                order_id,
                SourceService::Catalog,
            );
            let envelope = EventEnvelope::new(metadata, payload);
            let insert = OutboxInsert::from_envelope("catalog.events", &envelope);
            insert_outbox_event(tx.as_executor(), &insert)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to write event: {e}")))
}
