use order::AppState;
use rust_decimal::Decimal;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use shared::test_utils::db::TestDb;
use uuid::Uuid;

use order::orders::dtos::{CreateOrderItemReq, CreateOrderReq};
use order::orders::value_objects::ShippingAddressReq;

pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}

pub fn test_auth_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: b"test-access-secret-key-for-testing".to_vec(),
        refresh_token_secret: b"test-refresh-secret-key-for-testing".to_vec(),
        access_token_expiry_secs: 3600,
        refresh_token_expiry_secs: 7200,
    }
}

pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}

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

pub fn sample_create_order_req(seller_id: Uuid) -> CreateOrderReq {
    CreateOrderReq {
        items: vec![sample_order_item(seller_id)],
        currency: Some("USD".to_string()),
        shipping_address: sample_shipping_address(),
    }
}
