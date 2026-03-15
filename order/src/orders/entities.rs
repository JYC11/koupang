use rust_decimal::Decimal;
use shared::db::pagination_support::HasId;
use sqlx::FromRow;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, Utc};

use super::value_objects::OrderStatus;

#[derive(Debug, Clone, FromRow)]
pub struct OrderEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub buyer_id: Uuid,
    pub status: OrderStatus,
    pub total_amount: Decimal,
    pub currency: String,
    pub idempotency_key: String,
    pub shipping_address: serde_json::Value,
    pub cancelled_reason: Option<String>,
}

impl HasId for OrderEntity {
    fn id(&self) -> Uuid {
        self.id
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct OrderListEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub buyer_id: Uuid,
    pub status: OrderStatus,
    pub total_amount: Decimal,
    pub currency: String,
    pub item_count: i64,
}

impl HasId for OrderListEntity {
    fn id(&self) -> Uuid {
        self.id
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct OrderItemEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub order_id: Uuid,
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub product_name: String,
    pub sku_code: String,
    pub quantity: i32,
    pub seller_id: Uuid,
    pub unit_price: Decimal,
    pub total_price: Decimal,
}
