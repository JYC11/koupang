use crate::AppState;
use crate::orders::dtos::{
    CreateOrderReq, OrderDetailRes, OrderFilter, OrderItemRes, OrderListRes, OrderRes,
    ValidCreateOrderReq,
};
use crate::orders::repository;
use crate::orders::value_objects::{OrderId, OrderStatus};
use shared::auth::guards::require_access;
use shared::auth::jwt::CurrentUser;
use shared::db::pagination_support::{PaginationParams, PaginationRes, get_cursors};
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use shared::outbox::{OutboxInsert, insert_outbox_event};

// ── Create order ────────────────────────────────────────────

pub async fn create_order(
    state: &AppState,
    current_user: &CurrentUser,
    idempotency_key: &str,
    req: CreateOrderReq,
) -> Result<OrderRes, AppError> {
    let validated = ValidCreateOrderReq::new(idempotency_key, req)?;

    // Idempotency: return existing order if key already used
    if let Some(existing) =
        repository::get_order_by_idempotency_key(&state.pool, validated.idempotency_key.as_str())
            .await?
    {
        return Ok(OrderRes::new(existing));
    }

    let buyer_id = current_user.id;

    let order_id = with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            let order_id = repository::create_order(tx.as_executor(), buyer_id, &validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            // Write OrderCreated event to outbox
            let payload = serde_json::json!({
                "order_id": order_id.value(),
                "buyer_id": buyer_id,
                "total_amount": validated.total_amount.to_string(),
                "currency": validated.currency.as_str(),
                "items": validated.items.iter().map(|i| serde_json::json!({
                    "product_id": i.product_id,
                    "sku_id": i.sku_id,
                    "quantity": i.quantity.value(),
                    "unit_price": i.unit_price.value().to_string(),
                })).collect::<Vec<_>>(),
            });
            let metadata = EventMetadata::new(
                EventType::OrderCreated,
                AggregateType::Order,
                order_id.value(),
                SourceService::Order,
            );
            let envelope = EventEnvelope::new(metadata, payload);
            let insert = OutboxInsert::from_envelope("orders.events", &envelope);
            insert_outbox_event(tx.as_executor(), &insert)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(order_id)
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create order: {}", e)))?;

    let order = repository::get_order_by_id(&state.pool, order_id).await?;
    Ok(OrderRes::new(order))
}

// ── Get order detail ────────────────────────────────────────

pub async fn get_order_detail(
    state: &AppState,
    current_user: &CurrentUser,
    order_id: OrderId,
) -> Result<OrderDetailRes, AppError> {
    let order = repository::get_order_by_id(&state.pool, order_id).await?;
    require_access(current_user, &order.buyer_id)?;

    let items = repository::list_order_items(&state.pool, order_id).await?;

    Ok(OrderDetailRes {
        order: OrderRes::new(order),
        items: items.into_iter().map(OrderItemRes::new).collect(),
    })
}

// ── List my orders ──────────────────────────────────────────

pub async fn list_my_orders(
    state: &AppState,
    buyer_id: uuid::Uuid,
    params: PaginationParams,
    filter: OrderFilter,
) -> Result<PaginationRes<OrderListRes>, AppError> {
    let mut orders =
        repository::list_orders_by_buyer(&state.pool, buyer_id, &params, &filter).await?;
    let cursors = get_cursors(&params, &mut orders);
    let items = orders.into_iter().map(OrderListRes::new).collect();
    Ok(PaginationRes::new(items, cursors))
}

// ── List seller orders ──────────────────────────────────────

pub async fn list_seller_orders(
    state: &AppState,
    seller_id: uuid::Uuid,
    params: PaginationParams,
    filter: OrderFilter,
) -> Result<PaginationRes<OrderListRes>, AppError> {
    let mut orders =
        repository::list_orders_by_seller(&state.pool, seller_id, &params, &filter).await?;
    let cursors = get_cursors(&params, &mut orders);
    let items = orders.into_iter().map(OrderListRes::new).collect();
    Ok(PaginationRes::new(items, cursors))
}

// ── Cancel order ────────────────────────────────────────────

pub async fn cancel_order(
    state: &AppState,
    current_user: &CurrentUser,
    order_id: OrderId,
    reason: Option<String>,
) -> Result<(), AppError> {
    let order = repository::get_order_by_id(&state.pool, order_id).await?;
    require_access(current_user, &order.buyer_id)?;

    // Validate transition
    order.status.transition_to(&OrderStatus::Cancelled)?;

    let cancel_reason = reason.as_deref().unwrap_or("Cancelled by buyer");

    with_transaction(&state.pool, |tx| {
        let cancel_reason = cancel_reason.to_string();
        Box::pin(async move {
            repository::update_order_status(
                tx.as_executor(),
                order_id,
                &OrderStatus::Cancelled,
                Some(&cancel_reason),
            )
            .await
            .map_err(|e| TxError::Other(e.to_string()))?;

            // Write OrderCancelled event to outbox
            let payload = serde_json::json!({
                "order_id": order_id.value(),
                "buyer_id": order.buyer_id,
                "reason": cancel_reason,
            });
            let metadata = EventMetadata::new(
                EventType::OrderCancelled,
                AggregateType::Order,
                order_id.value(),
                SourceService::Order,
            );
            let envelope = EventEnvelope::new(metadata, payload);
            let insert = OutboxInsert::from_envelope("orders.events", &envelope);
            insert_outbox_event(tx.as_executor(), &insert)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to cancel order: {}", e)))?;

    Ok(())
}
