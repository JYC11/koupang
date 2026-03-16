use cart::cart::dtos::{AddToCartReq, UpdateCartItemReq};
use cart::cart::service;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::{sample_add_item_req, test_redis};

// ── get_cart ────────────────────────────────────────────────

#[tokio::test]
async fn get_empty_cart_returns_zero() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let cart = service::get_cart(&mut conn, user_id).await.unwrap();
    assert_eq!(cart.item_count, 0);
    assert_eq!(cart.total, Decimal::ZERO);
}

// ── add_item ────────────────────────────────────────────────

#[tokio::test]
async fn add_item_returns_cart_with_item() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let req = sample_add_item_req();
    let cart = service::add_item(&mut conn, user_id, req).await.unwrap();

    assert_eq!(cart.item_count, 1);
    assert_eq!(cart.items[0].product_name, "Test Widget");
    assert_eq!(cart.items[0].quantity, 2);
    // 2 * $19.99 = $39.98
    assert_eq!(cart.total, Decimal::new(3998, 2));
}

#[tokio::test]
async fn add_same_sku_updates_quantity() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let req = sample_add_item_req();
    let sku_id = req.sku_id;
    service::add_item(&mut conn, user_id, req).await.unwrap();

    let req2 = AddToCartReq {
        product_id: Uuid::new_v4(),
        sku_id,
        quantity: 5,
        unit_price: Decimal::new(2999, 2),
        currency: Some("USD".to_string()),
        product_name: "Updated Widget".to_string(),
        image_url: None,
    };
    let cart = service::add_item(&mut conn, user_id, req2).await.unwrap();

    assert_eq!(cart.item_count, 1, "should overwrite existing SKU");
    assert_eq!(cart.items[0].quantity, 5);
}

#[tokio::test]
async fn add_item_rejects_when_cart_full() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    // Fill cart to 50 items
    for _ in 0..50 {
        let req = sample_add_item_req(); // Each has unique sku_id
        service::add_item(&mut conn, user_id, req).await.unwrap();
    }

    // 51st should fail
    let req = sample_add_item_req();
    let result = service::add_item(&mut conn, user_id, req).await;
    assert!(result.is_err(), "should reject at max capacity");
}

#[tokio::test]
async fn add_item_with_zero_quantity_fails() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let mut req = sample_add_item_req();
    req.quantity = 0;

    let result = service::add_item(&mut conn, user_id, req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn add_item_with_quantity_over_max_fails() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let mut req = sample_add_item_req();
    req.quantity = 100; // max is 99

    let result = service::add_item(&mut conn, user_id, req).await;
    assert!(result.is_err());
}

// ── update_item_quantity ────────────────────────────────────

#[tokio::test]
async fn update_item_quantity_succeeds() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let req = sample_add_item_req();
    let sku_id = req.sku_id;
    service::add_item(&mut conn, user_id, req).await.unwrap();

    let update = UpdateCartItemReq { quantity: 5 };
    let cart = service::update_item_quantity(&mut conn, user_id, sku_id, update)
        .await
        .unwrap();

    assert_eq!(cart.items[0].quantity, 5);
}

#[tokio::test]
async fn update_nonexistent_item_fails() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let update = UpdateCartItemReq { quantity: 5 };
    let result = service::update_item_quantity(&mut conn, user_id, Uuid::new_v4(), update).await;
    assert!(result.is_err());
}

// ── remove_item ─────────────────────────────────────────────

#[tokio::test]
async fn remove_item_reduces_count() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let req = sample_add_item_req();
    let sku_id = req.sku_id;
    service::add_item(&mut conn, user_id, req).await.unwrap();

    service::remove_item(&mut conn, user_id, sku_id)
        .await
        .unwrap();

    let cart = service::get_cart(&mut conn, user_id).await.unwrap();
    assert_eq!(cart.item_count, 0);
}

// ── clear_cart ──────────────────────────────────────────────

#[tokio::test]
async fn clear_cart_empties_everything() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    for _ in 0..3 {
        service::add_item(&mut conn, user_id, sample_add_item_req())
            .await
            .unwrap();
    }

    service::clear_cart(&mut conn, user_id).await.unwrap();

    let cart = service::get_cart(&mut conn, user_id).await.unwrap();
    assert_eq!(cart.item_count, 0);
}

// ── validate_cart ───────────────────────────────────────────

#[tokio::test]
async fn validate_cart_succeeds_with_valid_items() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    service::add_item(&mut conn, user_id, sample_add_item_req())
        .await
        .unwrap();

    let result = service::validate_cart(&mut conn, user_id).await.unwrap();
    assert!(result.all_valid);
    assert_eq!(result.items.len(), 1);
}

#[tokio::test]
async fn validate_empty_cart_fails() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let result = service::validate_cart(&mut conn, user_id).await;
    assert!(
        result.is_err(),
        "empty cart should fail checkout validation"
    );
}
