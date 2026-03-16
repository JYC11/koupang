use cart::AppState;
use rust_decimal::Decimal;
use shared::config::auth_config::AuthConfig;
use shared::test_utils::redis::TestRedis;
use uuid::Uuid;

use cart::cart::dtos::AddToCartReq;

pub async fn test_redis() -> TestRedis {
    TestRedis::start().await
}

pub fn test_auth_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: b"test-access-secret-key-for-testing".to_vec(),
        refresh_token_secret: b"test-refresh-secret-key-for-testing".to_vec(),
        access_token_expiry_secs: 3600,
        refresh_token_expiry_secs: 7200,
    }
}

pub fn test_app_state(conn: redis::aio::ConnectionManager) -> AppState {
    AppState::new_with_jwt(conn, test_auth_config())
}

pub fn sample_add_item_req() -> AddToCartReq {
    AddToCartReq {
        product_id: Uuid::new_v4(),
        sku_id: Uuid::new_v4(),
        quantity: 2,
        unit_price: Decimal::new(1999, 2), // $19.99
        currency: Some("USD".to_string()),
        product_name: "Test Widget".to_string(),
        image_url: Some("https://example.com/widget.jpg".to_string()),
    }
}
