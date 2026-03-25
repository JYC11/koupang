use catalog::inventory::entities::ReservationStatus;
use catalog::inventory::repository;
use rust_decimal::Decimal;
use shared::db::PgPool;
use uuid::Uuid;

use crate::common::test_db;

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

// ── Reserve inventory ─────────────────────────────────────────

#[tokio::test]
async fn reserve_inventory_succeeds_with_sufficient_stock() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id, 10)
        .await
        .unwrap();

    // Verify reservation record was created
    let reservation = repository::get_reservation(&mut *conn, order_id, sku_id)
        .await
        .unwrap();
    assert!(reservation.is_some());
    let reservation = reservation.unwrap();
    assert_eq!(reservation.order_id, order_id);
    assert_eq!(reservation.sku_id, sku_id);
    assert_eq!(reservation.quantity, 10);
    assert_eq!(reservation.status, ReservationStatus::Reserved);
}

#[tokio::test]
async fn reserve_inventory_updates_sku_reserved_quantity() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id, 25)
        .await
        .unwrap();

    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.stock_quantity, 100);
    assert_eq!(availability.reserved_quantity, 25);
    assert_eq!(availability.available_quantity, 75);
}

#[tokio::test]
async fn reserve_inventory_fails_with_insufficient_stock() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 5).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let result = repository::reserve_inventory(&mut *conn, order_id, sku_id, 10).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Insufficient stock"), "got: {err}");
}

#[tokio::test]
async fn reserve_inventory_fails_when_all_stock_already_reserved() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 10).await;
    let order1 = Uuid::now_v7();
    let order2 = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    // Reserve all 10 units
    repository::reserve_inventory(&mut *conn, order1, sku_id, 10)
        .await
        .unwrap();

    // Second reservation should fail — no available stock
    let result = repository::reserve_inventory(&mut *conn, order2, sku_id, 1).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn reserve_inventory_duplicate_order_sku_fails() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id, 5)
        .await
        .unwrap();

    // Duplicate (same order_id + sku_id) should fail due to UNIQUE constraint
    let result = repository::reserve_inventory(&mut *conn, order_id, sku_id, 5).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn reserve_inventory_exact_stock_succeeds() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 10).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id, 10)
        .await
        .unwrap();

    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.available_quantity, 0);
}

// ── Release reservation ───────────────────────────────────────

#[tokio::test]
async fn release_reservation_restores_available_stock() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id, 30)
        .await
        .unwrap();

    repository::release_reservation(&mut *conn, order_id, sku_id)
        .await
        .unwrap();

    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.reserved_quantity, 0);
    assert_eq!(availability.available_quantity, 100);

    // Reservation status should be 'released'
    let reservation = repository::get_reservation(&mut *conn, order_id, sku_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reservation.status, ReservationStatus::Released);
    assert!(reservation.released_at.is_some());
}

#[tokio::test]
async fn release_nonexistent_reservation_fails() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();
    let sku_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let result = repository::release_reservation(&mut *conn, order_id, sku_id).await;
    assert!(result.is_err());
}

// ── Confirm reservation ───────────────────────────────────────

#[tokio::test]
async fn confirm_reservation_deducts_stock_and_reserved() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id, 20)
        .await
        .unwrap();

    repository::confirm_reservation(&mut *conn, order_id, sku_id)
        .await
        .unwrap();

    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.stock_quantity, 80);
    assert_eq!(availability.reserved_quantity, 0);
    assert_eq!(availability.available_quantity, 80);

    // Reservation status should be 'confirmed'
    let reservation = repository::get_reservation(&mut *conn, order_id, sku_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reservation.status, ReservationStatus::Confirmed);
    assert!(reservation.confirmed_at.is_some());
}

#[tokio::test]
async fn confirm_nonexistent_reservation_fails() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();
    let sku_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    let result = repository::confirm_reservation(&mut *conn, order_id, sku_id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn confirm_reservation_fails_when_stock_reduced_below_reserved() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 20).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id, 15)
        .await
        .unwrap();

    // Simulate admin reducing stock below reserved quantity
    sqlx::query("UPDATE skus SET stock_quantity = 5 WHERE id = $1")
        .bind(sku_id)
        .execute(&mut *conn)
        .await
        .unwrap();

    // Confirm should fail with a meaningful error, not a generic 500
    let result = repository::confirm_reservation(&mut *conn, order_id, sku_id).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("stock was reduced below reserved quantity"),
        "Expected constraint violation message, got: {err}"
    );
}

// ── Release all reservations ──────────────────────────────────

#[tokio::test]
async fn release_all_reservations_releases_multiple_skus() {
    let db = test_db().await;
    let (_, sku_id1) = create_test_sku(&db.pool, 100).await;
    let (_, sku_id2) = create_test_sku(&db.pool, 50).await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id1, 10)
        .await
        .unwrap();
    repository::reserve_inventory(&mut *conn, order_id, sku_id2, 5)
        .await
        .unwrap();

    repository::release_all_reservations(&mut *conn, order_id)
        .await
        .unwrap();

    // Both SKUs should have full availability restored
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

#[tokio::test]
async fn release_all_reservations_noop_when_none_exist() {
    let db = test_db().await;
    let order_id = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    // Should not error when there are no reservations
    repository::release_all_reservations(&mut *conn, order_id)
        .await
        .unwrap();
}

// ── SKU availability view ─────────────────────────────────────

#[tokio::test]
async fn get_sku_availability_returns_correct_values() {
    let db = test_db().await;
    let (product_id, sku_id) = create_test_sku(&db.pool, 50).await;

    let mut conn = db.pool.acquire().await.unwrap();
    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();

    assert_eq!(availability.sku_id, sku_id);
    assert_eq!(availability.product_id, product_id);
    assert_eq!(availability.stock_quantity, 50);
    assert_eq!(availability.reserved_quantity, 0);
    assert_eq!(availability.available_quantity, 50);
}

#[tokio::test]
async fn get_sku_availability_nonexistent_fails() {
    let db = test_db().await;

    let mut conn = db.pool.acquire().await.unwrap();
    let result = repository::get_sku_availability(&mut *conn, Uuid::new_v4()).await;
    assert!(result.is_err());
}

// ── Multi-order reservation scenario ──────────────────────────

#[tokio::test]
async fn multiple_orders_can_reserve_same_sku() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order1 = Uuid::now_v7();
    let order2 = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order1, sku_id, 30)
        .await
        .unwrap();
    repository::reserve_inventory(&mut *conn, order2, sku_id, 40)
        .await
        .unwrap();

    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.reserved_quantity, 70);
    assert_eq!(availability.available_quantity, 30);
}

#[tokio::test]
async fn release_one_order_does_not_affect_another() {
    let db = test_db().await;
    let (_, sku_id) = create_test_sku(&db.pool, 100).await;
    let order1 = Uuid::now_v7();
    let order2 = Uuid::now_v7();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::reserve_inventory(&mut *conn, order1, sku_id, 30)
        .await
        .unwrap();
    repository::reserve_inventory(&mut *conn, order2, sku_id, 20)
        .await
        .unwrap();

    // Release order1 only
    repository::release_reservation(&mut *conn, order1, sku_id)
        .await
        .unwrap();

    let availability = repository::get_sku_availability(&mut *conn, sku_id)
        .await
        .unwrap();
    assert_eq!(availability.reserved_quantity, 20);
    assert_eq!(availability.available_quantity, 80);
}
