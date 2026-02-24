use catalog::AppState;
use catalog::products::dtos::{AddProductImageReq, CreateProductReq, CreateSkuReq};
use catalog::products::service::CatalogService;
use rust_decimal::Decimal;
use shared::auth::Role;
use shared::auth::jwt::CurrentUser;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use shared::test_utils::db::TestDb;
use std::sync::Arc;

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

pub fn test_catalog_service(pool: PgPool) -> CatalogService {
    CatalogService::new(pool)
}

pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}

pub fn seller_user() -> CurrentUser {
    CurrentUser {
        id: uuid::Uuid::new_v4(),
        role: Role::Seller,
    }
}

pub fn buyer_user() -> CurrentUser {
    CurrentUser {
        id: uuid::Uuid::new_v4(),
        role: Role::Buyer,
    }
}

pub fn admin_user() -> CurrentUser {
    CurrentUser {
        id: uuid::Uuid::new_v4(),
        role: Role::Admin,
    }
}

pub fn sample_create_product_req() -> CreateProductReq {
    CreateProductReq {
        name: "Test Widget".to_string(),
        slug: None, // auto-generated from name
        description: Some("A test product".to_string()),
        base_price: Decimal::new(1999, 2), // 19.99
        currency: None,                    // defaults to USD
        category: Some("Electronics".to_string()),
        brand: Some("TestBrand".to_string()),
    }
}

pub fn sample_create_product_req_2() -> CreateProductReq {
    CreateProductReq {
        name: "Another Widget".to_string(),
        slug: Some("another-widget".to_string()),
        description: None,
        base_price: Decimal::new(4999, 2), // 49.99
        currency: Some("KRW".to_string()),
        category: None,
        brand: None,
    }
}

pub fn sample_create_sku_req() -> CreateSkuReq {
    CreateSkuReq {
        sku_code: "WIDGET-BLUE-XL".to_string(),
        price: Decimal::new(2499, 2), // 24.99
        stock_quantity: 100,
        attributes: Some(serde_json::json!({"color": "blue", "size": "XL"})),
    }
}

pub fn sample_add_image_req() -> AddProductImageReq {
    AddProductImageReq {
        url: "https://cdn.example.com/img/widget-1.jpg".to_string(),
        alt_text: Some("Widget front view".to_string()),
        sort_order: Some(0),
        is_primary: Some(true),
    }
}
