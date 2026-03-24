use rust_decimal::Decimal;
use uuid::Uuid;

use crate::AppState;
use crate::orders::dtos::{CreateOrderItemReq, CreateOrderReq, ValidCreateOrderReq};
use crate::orders::repository;
use crate::orders::service;
use crate::orders::value_objects::{OrderId, OrderStatus, ShippingAddressReq};
use shared::auth::Role;
use shared::auth::jwt::CurrentUser;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;

pub fn sample_shipping_address() -> ShippingAddressReq {
    ShippingAddressReq {
        street: "123 Test Street".to_string(),
        city: "Seoul".to_string(),
        state: "".to_string(),
        postal_code: "06000".to_string(),
        country: "KR".to_string(),
    }
}

pub fn sample_order_item(seller_id: Uuid) -> CreateOrderItemReq {
    CreateOrderItemReq {
        product_id: Uuid::new_v4(),
        sku_id: Uuid::new_v4(),
        product_name: "Test Widget".to_string(),
        sku_code: "WIDGET-BLUE-XL".to_string(),
        quantity: 2,
        seller_id,
        unit_price: Decimal::new(1999, 2), // $19.99
    }
}

pub fn sample_order_item_with_sku(
    seller_id: Uuid,
    sku_id: Uuid,
    quantity: i32,
) -> CreateOrderItemReq {
    CreateOrderItemReq {
        product_id: Uuid::new_v4(),
        sku_id,
        product_name: "Test Widget".to_string(),
        sku_code: "WIDGET-BLUE-XL".to_string(),
        quantity,
        seller_id,
        unit_price: Decimal::new(2499, 2), // $24.99
    }
}

pub fn sample_create_order_req(seller_id: Uuid) -> CreateOrderReq {
    CreateOrderReq {
        items: vec![sample_order_item(seller_id)],
        currency: Some("USD".to_string()),
        shipping_address: sample_shipping_address(),
    }
}

/// Create an order in the database and return (order_id, buyer_id).
pub async fn create_test_order(pool: &PgPool) -> (Uuid, Uuid) {
    let seller_id = Uuid::new_v4();
    let buyer_id = Uuid::new_v4();
    let req = sample_create_order_req(seller_id);
    let validated =
        ValidCreateOrderReq::new(&format!("test-{}", Uuid::new_v4()), req).expect("valid req");

    let mut conn = pool.acquire().await.unwrap();
    let order_id = repository::create_order(&mut *conn, buyer_id, &validated)
        .await
        .unwrap();

    (order_id.value(), buyer_id)
}

/// Create an order with specific items and return (order_id, buyer_id).
pub async fn create_test_order_with_items(
    pool: &PgPool,
    items: Vec<CreateOrderItemReq>,
) -> (Uuid, Uuid) {
    let buyer_id = Uuid::new_v4();
    let req = CreateOrderReq {
        items,
        currency: Some("USD".to_string()),
        shipping_address: sample_shipping_address(),
    };
    let validated =
        ValidCreateOrderReq::new(&format!("test-{}", Uuid::new_v4()), req).expect("valid req");

    let mut conn = pool.acquire().await.unwrap();
    let order_id = repository::create_order(&mut *conn, buyer_id, &validated)
        .await
        .unwrap();

    (order_id.value(), buyer_id)
}

/// Create an order via the service layer (writes OrderCreated to outbox).
/// Returns (order_id, buyer_id).
pub async fn create_order_via_service(
    pool: &PgPool,
    items: Vec<CreateOrderItemReq>,
) -> (Uuid, Uuid) {
    let buyer_id = Uuid::new_v4();
    // Use deterministic test config (doesn't read env vars).
    let auth_config = AuthConfig {
        access_token_secret: b"test-access-secret-key-for-testing".to_vec(),
        refresh_token_secret: b"test-refresh-secret-key-for-testing".to_vec(),
        access_token_expiry_secs: 3600,
        refresh_token_expiry_secs: 7200,
    };
    let app_state = AppState::new_with_jwt(pool.clone(), auth_config);
    let current_user = CurrentUser {
        id: buyer_id,
        role: Role::Buyer,
    };
    let req = CreateOrderReq {
        items,
        currency: Some("USD".to_string()),
        shipping_address: sample_shipping_address(),
    };
    let idempotency_key = format!("test-{}", Uuid::new_v4());
    let res = service::create_order(&app_state, &current_user, &idempotency_key, req)
        .await
        .expect("create_order_via_service failed");

    let order_id: Uuid = res.id.parse().expect("order id should be a UUID");
    (order_id, buyer_id)
}

/// Transition an order to a specific status by walking through the state machine.
pub async fn advance_order_to(pool: &PgPool, order_id: OrderId, target: &OrderStatus) {
    let path = match target {
        OrderStatus::InventoryReserved => vec![OrderStatus::InventoryReserved],
        OrderStatus::PaymentAuthorized => {
            vec![
                OrderStatus::InventoryReserved,
                OrderStatus::PaymentAuthorized,
            ]
        }
        OrderStatus::Confirmed => vec![
            OrderStatus::InventoryReserved,
            OrderStatus::PaymentAuthorized,
            OrderStatus::Confirmed,
        ],
        _ => vec![],
    };

    let mut conn = pool.acquire().await.unwrap();
    for status in path {
        repository::update_order_status(&mut *conn, order_id, &status, None)
            .await
            .unwrap();
    }
}
