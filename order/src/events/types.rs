use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Payload for OrderCreated events on orders.events topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCreatedPayload {
    pub order_id: Uuid,
    pub buyer_id: Uuid,
    pub total_amount: String,
    pub currency: String,
    pub items: Vec<OrderCreatedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCreatedItem {
    pub product_id: Uuid,
    pub sku_id: Uuid,
    pub quantity: i32,
    pub unit_price: String,
}

/// Payload for OrderConfirmed events on orders.events topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderConfirmedPayload {
    pub order_id: Uuid,
    pub buyer_id: Uuid,
}

/// Payload for OrderCancelled events on orders.events topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderCancelledPayload {
    pub order_id: Uuid,
    pub buyer_id: Uuid,
    pub reason: String,
}

/// Payload for InventoryReserved from catalog (consumed by order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReservedPayload {
    pub order_id: Uuid,
    pub buyer_id: Uuid,
    pub total_amount: String,
    pub currency: String,
    pub items: Vec<InventoryReservedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReservedItem {
    pub sku_id: Uuid,
    pub quantity: i32,
}

/// Payload for InventoryReservationFailed from catalog (consumed by order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryReservationFailedPayload {
    pub order_id: Uuid,
    pub reason: String,
}

/// Payload for PaymentAuthorized from payment (consumed by order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentAuthorizedPayload {
    pub order_id: Uuid,
    pub payment_id: Uuid,
    pub gateway_reference: String,
}

/// Payload for PaymentFailed from payment (consumed by order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentFailedPayload {
    pub order_id: Uuid,
    pub reason: String,
}

/// Payload for PaymentTimedOut from payment (consumed by order).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentTimedOutPayload {
    pub order_id: Uuid,
    pub reason: String,
}
