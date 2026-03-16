use crate::orders::entities::{OrderEntity, OrderItemEntity, OrderListEntity};
use crate::orders::value_objects::{
    Currency, IdempotencyKey, OrderStatus, Price, Quantity, ShippingAddress, ShippingAddressReq,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use shared::db::pagination_support::{PaginationDirection, PaginationParams, PaginationQuery};
use shared::dto_helpers::{fmt_datetime, fmt_id};
use shared::errors::AppError;
use uuid::Uuid;

// ── Filter DTOs ─────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OrderFilterQuery {
    pub limit: Option<u32>,
    pub cursor: Option<Uuid>,
    pub direction: Option<PaginationDirection>,
    pub status: Option<OrderStatus>,
}

impl OrderFilterQuery {
    pub fn into_parts(self) -> (PaginationParams, OrderFilter) {
        let pagination = PaginationQuery {
            limit: self.limit,
            cursor: self.cursor,
            direction: self.direction,
        };
        let filter = OrderFilter {
            status: self.status,
        };
        (pagination.into_params(), filter)
    }
}

pub struct OrderFilter {
    pub status: Option<OrderStatus>,
}

// ── Create Order Request ────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderReq {
    pub items: Vec<CreateOrderItemReq>,
    pub currency: Option<String>,
    pub shipping_address: ShippingAddressReq,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderItemReq {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub product_name: String,
    pub sku_code: String,
    pub quantity: i32,
    pub seller_id: Uuid,
    pub unit_price: Decimal,
}

// ── Validated Create Order ──────────────────────────────────

pub struct ValidCreateOrderReq {
    pub idempotency_key: IdempotencyKey,
    pub currency: Currency,
    pub shipping_address: ShippingAddress,
    pub items: Vec<ValidCreateOrderItem>,
    pub total_amount: Decimal,
}

pub struct ValidCreateOrderItem {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub product_name: String,
    pub sku_code: String,
    pub quantity: Quantity,
    pub seller_id: Uuid,
    pub unit_price: Price,
    pub total_price: Decimal,
}

impl ValidCreateOrderReq {
    pub fn new(idempotency_key: &str, req: CreateOrderReq) -> Result<Self, AppError> {
        let key = IdempotencyKey::new(idempotency_key)?;
        let currency = match req.currency {
            Some(c) => Currency::new(&c)?,
            None => Currency::default(),
        };
        let shipping_address = ShippingAddress::new(req.shipping_address)?;

        if req.items.is_empty() {
            return Err(AppError::BadRequest(
                "Order must have at least one item".to_string(),
            ));
        }

        let mut items = Vec::with_capacity(req.items.len());
        let mut total_amount = Decimal::ZERO;

        for item in req.items {
            let quantity = Quantity::new(item.quantity)?;
            let unit_price = Price::new(item.unit_price)?;
            let total_price = unit_price.value() * Decimal::from(quantity.value());

            total_amount += total_price;

            items.push(ValidCreateOrderItem {
                product_id: item.product_id,
                sku_id: item.sku_id,
                product_name: item.product_name,
                sku_code: item.sku_code,
                quantity,
                seller_id: item.seller_id,
                unit_price,
                total_price,
            });
        }

        Ok(Self {
            idempotency_key: key,
            currency,
            shipping_address,
            items,
            total_amount,
        })
    }
}

// ── Response DTOs ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRes {
    pub id: String,
    pub created_at: String,
    pub buyer_id: String,
    pub status: OrderStatus,
    pub total_amount: Decimal,
    pub currency: String,
    pub idempotency_key: String,
    pub shipping_address: serde_json::Value,
    pub cancelled_reason: Option<String>,
}

impl OrderRes {
    pub fn new(entity: OrderEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            buyer_id: fmt_id(&entity.buyer_id),
            status: entity.status,
            total_amount: entity.total_amount,
            currency: entity.currency,
            idempotency_key: entity.idempotency_key,
            shipping_address: entity.shipping_address,
            cancelled_reason: entity.cancelled_reason,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderListRes {
    pub id: String,
    pub created_at: String,
    pub buyer_id: String,
    pub status: OrderStatus,
    pub total_amount: Decimal,
    pub currency: String,
    pub item_count: i64,
}

impl OrderListRes {
    pub fn new(entity: OrderListEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            buyer_id: fmt_id(&entity.buyer_id),
            status: entity.status,
            total_amount: entity.total_amount,
            currency: entity.currency,
            item_count: entity.item_count,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItemRes {
    pub id: String,
    pub product_id: String,
    pub sku_id: String,
    pub product_name: String,
    pub sku_code: String,
    pub quantity: i32,
    pub seller_id: String,
    pub unit_price: Decimal,
    pub total_price: Decimal,
}

impl OrderItemRes {
    pub fn new(entity: OrderItemEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            product_id: fmt_id(&entity.product_id),
            sku_id: fmt_id(&entity.sku_id),
            product_name: entity.product_name,
            sku_code: entity.sku_code,
            quantity: entity.quantity,
            seller_id: fmt_id(&entity.seller_id),
            unit_price: entity.unit_price,
            total_price: entity.total_price,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderDetailRes {
    #[serde(flatten)]
    pub order: OrderRes,
    pub items: Vec<OrderItemRes>,
}
