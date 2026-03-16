use cart::cart::repository::{self, CartItemStored};
use chrono::Utc;
use uuid::Uuid;

use crate::common::test_redis;

fn sample_stored(sku_id: Uuid) -> CartItemStored {
    CartItemStored {
        product_id: Uuid::new_v4(),
        sku_id,
        quantity: 2,
        unit_price: "19.99".to_string(),
        currency: "USD".to_string(),
        product_name: "Test Widget".to_string(),
        image_url: Some("https://example.com/widget.jpg".to_string()),
        added_at: Utc::now(),
    }
}

// ── set + get ───────────────────────────────────────────────

#[tokio::test]
async fn set_and_get_cart_item() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();
    let sku_id = Uuid::new_v4();
    let stored = sample_stored(sku_id);

    repository::set_cart_item(&mut conn, user_id, sku_id, &stored)
        .await
        .unwrap();

    let item = repository::get_cart_item(&mut conn, user_id, sku_id)
        .await
        .unwrap();
    assert!(item.is_some());
    let item = item.unwrap();
    assert_eq!(item.sku_id, sku_id);
    assert_eq!(item.product_name, "Test Widget");
    assert_eq!(item.quantity, 2);
}

#[tokio::test]
async fn get_nonexistent_item_returns_none() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let item = repository::get_cart_item(&mut conn, user_id, Uuid::new_v4())
        .await
        .unwrap();
    assert!(item.is_none());
}

// ── get_cart (hgetall) ──────────────────────────────────────

#[tokio::test]
async fn get_cart_returns_all_items() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let sku1 = Uuid::new_v4();
    let sku2 = Uuid::new_v4();
    repository::set_cart_item(&mut conn, user_id, sku1, &sample_stored(sku1))
        .await
        .unwrap();
    repository::set_cart_item(&mut conn, user_id, sku2, &sample_stored(sku2))
        .await
        .unwrap();

    let items = repository::get_cart(&mut conn, user_id).await.unwrap();
    assert_eq!(items.len(), 2);
}

#[tokio::test]
async fn get_empty_cart_returns_empty_vec() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    let items = repository::get_cart(&mut conn, user_id).await.unwrap();
    assert!(items.is_empty());
}

// ── cart_item_count (hlen) ──────────────────────────────────

#[tokio::test]
async fn cart_item_count_correct() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    assert_eq!(
        repository::cart_item_count(&mut conn, user_id)
            .await
            .unwrap(),
        0
    );

    let sku1 = Uuid::new_v4();
    repository::set_cart_item(&mut conn, user_id, sku1, &sample_stored(sku1))
        .await
        .unwrap();
    assert_eq!(
        repository::cart_item_count(&mut conn, user_id)
            .await
            .unwrap(),
        1
    );
}

// ── item_exists (hexists) ───────────────────────────────────

#[tokio::test]
async fn item_exists_true_when_present() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();
    let sku_id = Uuid::new_v4();

    repository::set_cart_item(&mut conn, user_id, sku_id, &sample_stored(sku_id))
        .await
        .unwrap();

    assert!(
        repository::item_exists(&mut conn, user_id, sku_id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn item_exists_false_when_absent() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    assert!(
        !repository::item_exists(&mut conn, user_id, Uuid::new_v4())
            .await
            .unwrap()
    );
}

// ── remove_cart_item (hdel) ─────────────────────────────────

#[tokio::test]
async fn remove_cart_item_deletes_item() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();
    let sku_id = Uuid::new_v4();

    repository::set_cart_item(&mut conn, user_id, sku_id, &sample_stored(sku_id))
        .await
        .unwrap();
    assert!(
        repository::item_exists(&mut conn, user_id, sku_id)
            .await
            .unwrap()
    );

    repository::remove_cart_item(&mut conn, user_id, sku_id)
        .await
        .unwrap();
    assert!(
        !repository::item_exists(&mut conn, user_id, sku_id)
            .await
            .unwrap()
    );
}

// ── clear_cart (del) ────────────────────────────────────────

#[tokio::test]
async fn clear_cart_removes_all_items() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user_id = Uuid::new_v4();

    for _ in 0..3 {
        let sku = Uuid::new_v4();
        repository::set_cart_item(&mut conn, user_id, sku, &sample_stored(sku))
            .await
            .unwrap();
    }
    assert_eq!(
        repository::cart_item_count(&mut conn, user_id)
            .await
            .unwrap(),
        3
    );

    repository::clear_cart(&mut conn, user_id).await.unwrap();
    assert_eq!(
        repository::cart_item_count(&mut conn, user_id)
            .await
            .unwrap(),
        0
    );
}

// ── User isolation ──────────────────────────────────────────

#[tokio::test]
async fn different_users_have_separate_carts() {
    let redis = test_redis().await;
    let mut conn = redis.conn.clone();
    let user1 = Uuid::new_v4();
    let user2 = Uuid::new_v4();
    let sku = Uuid::new_v4();

    repository::set_cart_item(&mut conn, user1, sku, &sample_stored(sku))
        .await
        .unwrap();

    assert!(
        repository::item_exists(&mut conn, user1, sku)
            .await
            .unwrap()
    );
    assert!(
        !repository::item_exists(&mut conn, user2, sku)
            .await
            .unwrap()
    );
}
