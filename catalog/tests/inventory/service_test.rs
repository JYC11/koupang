use catalog::inventory::repository;
use catalog::inventory::service;
use rust_decimal::Decimal;
use shared::db::PgPool;
use uuid::Uuid;

use crate::common::{test_app_state, test_db};

/// Insert a product + SKU via raw SQL and return (product_id, sku_id).
async fn create_test_sku(pool: &PgPool, stock: i32) -> (Uuid, Uuid) {
    let seller_id = Uuid::new_v4();
    let product_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO products (seller_id, name, slug, base_price, currency, status) \
         VALUES ($1, $2, $3, $4, $5, 'active') RETURNING id",
    )
    .bind(seller_id)
    .bind("Test Product")
    .bind(&format!("test-{}", Uuid::new_v4()))
    .bind(Decimal::new(1999, 2))
    .bind("USD")
    .fetch_one(pool)
    .await
    .unwrap();

    let sku_id: (Uuid,) = sqlx::query_as(
        "INSERT INTO skus (product_id, sku_code, price, stock_quantity, attributes, status) \
         VALUES ($1, $2, $3, $4, '{}'::jsonb, 'active') RETURNING id",
    )
    .bind(product_id.0)
    .bind(&format!("SKU-{}", Uuid::new_v4()))
    .bind(Decimal::new(1999, 2))
    .bind(stock)
    .fetch_one(pool)
    .await
    .unwrap();

    (product_id.0, sku_id.0)
}

// ── reserve_for_order ─────────────────────────────────────────

#[tokio::test]
async fn reserve_for_order_reserves_and_writes_outbox() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();
    let buyer_id = Uuid::new_v4();

    let items = vec![(sku_id, 10)];
    service::reserve_for_order(&state, order_id, buyer_id, "19.99", "USD", &items)
        .await
        .unwrap();

    // Verify reservation was created
    let mut conn = db.pool.acquire().await.unwrap();
    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.reserved_quantity, 10);
    assert_eq!(availability.available_quantity, 90);

    // Verify InventoryReserved outbox event
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'InventoryReserved' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn reserve_for_order_multiple_skus() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let (_, sku_id1) = create_test_sku(&db.pool, 50).await;
    let (_, sku_id2) = create_test_sku(&db.pool, 30).await;
    let order_id = Uuid::now_v7();
    let buyer_id = Uuid::new_v4();

    let items = vec![(sku_id1, 5), (sku_id2, 3)];
    service::reserve_for_order(&state, order_id, buyer_id, "39.98", "USD", &items)
        .await
        .unwrap();

    let mut conn = db.pool.acquire().await.unwrap();
    let avail1 = repository::get_sku_availability(&mut *conn, sku_id1)
        .await
        .unwrap();
    assert_eq!(avail1.reserved_quantity, 5);

    let avail2 = repository::get_sku_availability(&mut *conn, sku_id2)
        .await
        .unwrap();
    assert_eq!(avail2.reserved_quantity, 3);
}

#[tokio::test]
async fn reserve_for_order_insufficient_stock_writes_failure_event() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let (_, sku_id) = create_test_sku(&db.pool, 5).await;
    let order_id = Uuid::now_v7();
    let buyer_id = Uuid::new_v4();

    let items = vec![(sku_id, 100)]; // way more than available
    let result =
        service::reserve_for_order(&state, order_id, buyer_id, "19.99", "USD", &items).await;

    assert!(result.is_err());

    // InventoryReservationFailed should still be written (on separate tx)
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'InventoryReservationFailed' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn reserve_for_order_outbox_payload_contains_expected_fields() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();
    let buyer_id = Uuid::new_v4();

    let items = vec![(sku_id, 5)];
    service::reserve_for_order(&state, order_id, buyer_id, "19.99", "USD", &items)
        .await
        .unwrap();

    let row: (serde_json::Value,) = sqlx::query_as(
        "SELECT payload FROM outbox_events WHERE event_type = 'InventoryReserved' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();

    // Outbox payload is the full serialized EventEnvelope; inner payload is at ["payload"]
    let envelope = row.0;
    assert_eq!(
        envelope["metadata"]["event_type"].as_str().unwrap(),
        "InventoryReserved"
    );
    let payload = &envelope["payload"];
    assert_eq!(payload["order_id"].as_str().unwrap(), order_id.to_string());
    assert_eq!(payload["buyer_id"].as_str().unwrap(), buyer_id.to_string());
    assert_eq!(payload["total_amount"].as_str().unwrap(), "19.99");
    assert_eq!(payload["currency"].as_str().unwrap(), "USD");
    assert!(payload["items"].as_array().is_some());
    assert_eq!(payload["items"].as_array().unwrap().len(), 1);
}

// ── release_for_order ─────────────────────────────────────────

#[tokio::test]
async fn release_for_order_releases_all_reservations() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let (_, sku_id1) = create_test_sku(&db.pool, 100).await;
    let (_, sku_id2) = create_test_sku(&db.pool, 50).await;
    let order_id = Uuid::now_v7();
    let buyer_id = Uuid::new_v4();

    let items = vec![(sku_id1, 10), (sku_id2, 5)];
    service::reserve_for_order(&state, order_id, buyer_id, "29.98", "USD", &items)
        .await
        .unwrap();

    service::release_for_order(&state, order_id).await.unwrap();

    let mut conn = db.pool.acquire().await.unwrap();
    let avail1 = repository::get_sku_availability(&mut *conn, sku_id1)
        .await
        .unwrap();
    assert_eq!(avail1.reserved_quantity, 0);
    assert_eq!(avail1.available_quantity, 100);

    let avail2 = repository::get_sku_availability(&mut *conn, sku_id2)
        .await
        .unwrap();
    assert_eq!(avail2.reserved_quantity, 0);
    assert_eq!(avail2.available_quantity, 50);
}

// ── confirm_for_order ─────────────────────────────────────────

#[tokio::test]
async fn confirm_for_order_deducts_stock() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();
    let buyer_id = Uuid::new_v4();

    let items = vec![(sku_id, 15)];
    service::reserve_for_order(&state, order_id, buyer_id, "19.99", "USD", &items)
        .await
        .unwrap();

    service::confirm_for_order(&state, order_id, &items)
        .await
        .unwrap();

    let mut conn = db.pool.acquire().await.unwrap();
    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.stock_quantity, 85);
    assert_eq!(availability.reserved_quantity, 0);
    assert_eq!(availability.available_quantity, 85);
}
