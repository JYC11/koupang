use crate::orders::dtos::{OrderFilter, ValidCreateOrderReq};
use crate::orders::entities::{OrderEntity, OrderItemEntity, OrderListEntity};
use crate::orders::value_objects::{OrderId, OrderStatus};
use shared::db::PgExec;
use shared::db::pagination_support::{PaginationParams, keyset_paginate};
use shared::errors::AppError;
use sqlx::{PgConnection, Postgres, QueryBuilder};
use uuid::Uuid;

// ── Order queries ───────────────────────────────────────────

pub async fn get_order_by_id<'e>(
    executor: impl PgExec<'e>,
    id: OrderId,
) -> Result<OrderEntity, AppError> {
    sqlx::query_as::<_, OrderEntity>("SELECT * FROM orders WHERE id = $1")
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("Order not found: {}", e)))
}

pub async fn get_order_by_idempotency_key<'e>(
    executor: impl PgExec<'e>,
    key: &str,
) -> Result<Option<OrderEntity>, AppError> {
    sqlx::query_as::<_, OrderEntity>("SELECT * FROM orders WHERE idempotency_key = $1")
        .bind(key)
        .fetch_optional(executor)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to check idempotency key: {}", e))
        })
}

const ORDER_LIST_SELECT: &str = "\
    SELECT o.id, o.created_at, o.buyer_id, o.status, o.total_amount, o.currency, \
           COUNT(oi.id) AS item_count \
    FROM orders o \
    LEFT JOIN order_items oi ON oi.order_id = o.id \
    WHERE 1=1";

fn apply_order_filters(qb: &mut QueryBuilder<Postgres>, filter: &OrderFilter) {
    if let Some(ref status) = filter.status {
        qb.push(" AND o.status = ");
        qb.push_bind(status.as_str().to_string());
    }
}

pub async fn list_orders_by_buyer<'e>(
    executor: impl PgExec<'e>,
    buyer_id: Uuid,
    params: &PaginationParams,
    filter: &OrderFilter,
) -> Result<Vec<OrderListEntity>, AppError> {
    let mut qb = QueryBuilder::new(ORDER_LIST_SELECT);
    qb.push(" AND o.buyer_id = ");
    qb.push_bind(buyer_id);
    apply_order_filters(&mut qb, filter);
    qb.push(" GROUP BY o.id");
    keyset_paginate(params, Some("o"), &mut qb);
    qb.build_query_as::<OrderListEntity>()
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list orders: {}", e)))
}

/// List orders containing items from a specific seller.
pub async fn list_orders_by_seller<'e>(
    executor: impl PgExec<'e>,
    seller_id: Uuid,
    params: &PaginationParams,
    filter: &OrderFilter,
) -> Result<Vec<OrderListEntity>, AppError> {
    let mut qb = QueryBuilder::new(
        "SELECT o.id, o.created_at, o.buyer_id, o.status, o.total_amount, o.currency, \
                COUNT(oi.id) AS item_count \
         FROM orders o \
         INNER JOIN order_items oi ON oi.order_id = o.id \
         WHERE oi.seller_id = ",
    );
    qb.push_bind(seller_id);
    apply_order_filters(&mut qb, filter);
    qb.push(" GROUP BY o.id");
    keyset_paginate(params, Some("o"), &mut qb);
    qb.build_query_as::<OrderListEntity>()
        .fetch_all(executor)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to list seller orders: {}", e)))
}

// ── Order items ─────────────────────────────────────────────

pub async fn list_order_items<'e>(
    executor: impl PgExec<'e>,
    order_id: OrderId,
) -> Result<Vec<OrderItemEntity>, AppError> {
    sqlx::query_as::<_, OrderItemEntity>(
        "SELECT * FROM order_items WHERE order_id = $1 ORDER BY created_at ASC LIMIT 100",
    )
    .bind(order_id.value())
    .fetch_all(executor)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to list order items: {}", e)))
}

// ── Order mutations ─────────────────────────────────────────

pub async fn create_order(
    tx: &mut PgConnection,
    buyer_id: Uuid,
    req: &ValidCreateOrderReq,
) -> Result<OrderId, AppError> {
    let shipping_json = serde_json::to_value(&req.shipping_address)
        .map_err(|e| AppError::InternalServerError(format!("Failed to serialize address: {e}")))?;

    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO orders (buyer_id, total_amount, currency, idempotency_key, shipping_address) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id",
    )
    .bind(buyer_id)
    .bind(req.total_amount)
    .bind(req.currency.as_str())
    .bind(req.idempotency_key.as_str())
    .bind(&shipping_json)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create order: {}", e)))?;

    let order_id = OrderId::new(row.0);

    for item in &req.items {
        sqlx::query(
            "INSERT INTO order_items (order_id, product_id, sku_id, product_name, sku_code, quantity, seller_id, unit_price, total_price) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(order_id.value())
        .bind(item.product_id)
        .bind(item.sku_id)
        .bind(&item.product_name)
        .bind(&item.sku_code)
        .bind(item.quantity.value())
        .bind(item.seller_id)
        .bind(item.unit_price.value())
        .bind(item.total_price)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create order item: {}", e)))?;
    }

    Ok(order_id)
}

pub async fn update_order_status(
    tx: &mut PgConnection,
    id: OrderId,
    status: &OrderStatus,
    cancelled_reason: Option<&str>,
) -> Result<(), AppError> {
    let result = if let Some(reason) = cancelled_reason {
        sqlx::query(
            "UPDATE orders SET status = $1, cancelled_reason = $2, updated_at = NOW() WHERE id = $3",
        )
        .bind(status.as_str())
        .bind(reason)
        .bind(id.value())
        .execute(&mut *tx)
        .await
    } else {
        sqlx::query("UPDATE orders SET status = $1, updated_at = NOW() WHERE id = $2")
            .bind(status.as_str())
            .bind(id.value())
            .execute(&mut *tx)
            .await
    };

    let result = result.map_err(|e| {
        AppError::InternalServerError(format!("Failed to update order status: {}", e))
    })?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Order not found".to_string()));
    }
    assert_eq!(
        result.rows_affected(),
        1,
        "UPDATE must affect exactly 1 row"
    );

    Ok(())
}
