use order::orders::dtos::{OrderFilter, ValidCreateOrderReq};
use order::orders::repository;
use order::orders::value_objects::{OrderId, OrderStatus};
use shared::db::pagination_support::PaginationParams;
use uuid::Uuid;

use crate::common::{sample_create_order_req, test_db};

fn validated_req(idempotency_key: &str, seller_id: Uuid) -> ValidCreateOrderReq {
    let req = sample_create_order_req(seller_id);
    ValidCreateOrderReq::new(idempotency_key, req).expect("sample should validate")
}

fn no_filter() -> OrderFilter {
    OrderFilter { status: None }
}

// ── Create order ────────────────────────────────────────────

#[tokio::test]
async fn create_order_inserts_order_and_items() {
    let db = test_db().await;
    let seller = Uuid::new_v4();
    let buyer = Uuid::new_v4();
    let validated = validated_req("create-test-1", seller);

    let mut conn = db.pool.acquire().await.unwrap();
    let order_id = repository::create_order(&mut *conn, buyer, &validated)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(order.buyer_id, buyer);
    assert_eq!(order.status, OrderStatus::Pending);
    assert_eq!(order.currency, "USD");
    assert_eq!(order.idempotency_key, "create-test-1");

    let items = repository::list_order_items(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].quantity, 2);
    assert_eq!(items[0].seller_id, seller);
}

#[tokio::test]
async fn get_nonexistent_order_fails() {
    let db = test_db().await;
    let result = repository::get_order_by_id(&db.pool, OrderId::new(Uuid::new_v4())).await;
    assert!(result.is_err());
}

// ── Idempotency key ─────────────────────────────────────────

#[tokio::test]
async fn get_order_by_idempotency_key_returns_existing() {
    let db = test_db().await;
    let seller = Uuid::new_v4();
    let buyer = Uuid::new_v4();
    let validated = validated_req("idem-key-1", seller);

    let mut conn = db.pool.acquire().await.unwrap();
    let order_id = repository::create_order(&mut *conn, buyer, &validated)
        .await
        .unwrap();

    let found = repository::get_order_by_idempotency_key(&db.pool, "idem-key-1")
        .await
        .unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().id, order_id.value());
}

#[tokio::test]
async fn get_order_by_idempotency_key_returns_none_for_unknown() {
    let db = test_db().await;
    let found = repository::get_order_by_idempotency_key(&db.pool, "nonexistent-key")
        .await
        .unwrap();
    assert!(found.is_none());
}

#[tokio::test]
async fn duplicate_idempotency_key_fails() {
    let db = test_db().await;
    let seller = Uuid::new_v4();
    let buyer = Uuid::new_v4();

    let mut conn = db.pool.acquire().await.unwrap();
    let v1 = validated_req("dup-key", seller);
    repository::create_order(&mut *conn, buyer, &v1)
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    let v2 = validated_req("dup-key", seller);
    let result = repository::create_order(&mut *conn2, buyer, &v2).await;
    assert!(result.is_err(), "duplicate idempotency_key should fail");
}

// ── Status updates ──────────────────────────────────────────

#[tokio::test]
async fn update_order_status_changes_status() {
    let db = test_db().await;
    let seller = Uuid::new_v4();
    let buyer = Uuid::new_v4();
    let validated = validated_req("status-1", seller);

    let mut conn = db.pool.acquire().await.unwrap();
    let order_id = repository::create_order(&mut *conn, buyer, &validated)
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::update_order_status(&mut *conn2, order_id, &OrderStatus::InventoryReserved, None)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::InventoryReserved);
}

#[tokio::test]
async fn update_order_status_with_cancelled_reason() {
    let db = test_db().await;
    let seller = Uuid::new_v4();
    let buyer = Uuid::new_v4();
    let validated = validated_req("cancel-1", seller);

    let mut conn = db.pool.acquire().await.unwrap();
    let order_id = repository::create_order(&mut *conn, buyer, &validated)
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::update_order_status(
        &mut *conn2,
        order_id,
        &OrderStatus::Cancelled,
        Some("out of stock"),
    )
    .await
    .unwrap();

    let order = repository::get_order_by_id(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(order.cancelled_reason.as_deref(), Some("out of stock"));
}

#[tokio::test]
async fn update_nonexistent_order_status_fails() {
    let db = test_db().await;
    let mut conn = db.pool.acquire().await.unwrap();
    let result = repository::update_order_status(
        &mut *conn,
        OrderId::new(Uuid::new_v4()),
        &OrderStatus::Cancelled,
        None,
    )
    .await;
    assert!(result.is_err());
}

// ── Keyset pagination ───────────────────────────────────────

#[tokio::test]
async fn list_orders_by_buyer_returns_buyer_orders() {
    let db = test_db().await;
    let seller = Uuid::new_v4();
    let buyer = Uuid::new_v4();
    let other_buyer = Uuid::new_v4();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::create_order(&mut *conn, buyer, &validated_req("list-1", seller))
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::create_order(&mut *conn2, buyer, &validated_req("list-2", seller))
        .await
        .unwrap();

    let mut conn3 = db.pool.acquire().await.unwrap();
    repository::create_order(&mut *conn3, other_buyer, &validated_req("list-3", seller))
        .await
        .unwrap();

    let orders = repository::list_orders_by_buyer(
        &db.pool,
        buyer,
        &PaginationParams::default(),
        &no_filter(),
    )
    .await
    .unwrap();

    assert_eq!(orders.len(), 2, "should only see buyer's orders");
    for o in &orders {
        assert_eq!(o.buyer_id, buyer);
        assert_eq!(o.item_count, 1);
    }
}

#[tokio::test]
async fn list_orders_by_buyer_with_status_filter() {
    let db = test_db().await;
    let seller = Uuid::new_v4();
    let buyer = Uuid::new_v4();

    let mut conn = db.pool.acquire().await.unwrap();
    let oid1 = repository::create_order(&mut *conn, buyer, &validated_req("filter-1", seller))
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::create_order(&mut *conn2, buyer, &validated_req("filter-2", seller))
        .await
        .unwrap();

    // Cancel one order
    let mut conn3 = db.pool.acquire().await.unwrap();
    repository::update_order_status(&mut *conn3, oid1, &OrderStatus::Cancelled, None)
        .await
        .unwrap();

    let filter = OrderFilter {
        status: Some(OrderStatus::Pending),
    };
    let orders =
        repository::list_orders_by_buyer(&db.pool, buyer, &PaginationParams::default(), &filter)
            .await
            .unwrap();

    assert_eq!(orders.len(), 1, "only pending order");
    assert_eq!(orders[0].status, OrderStatus::Pending);
}

#[tokio::test]
async fn list_orders_by_seller_returns_seller_orders() {
    let db = test_db().await;
    let seller1 = Uuid::new_v4();
    let seller2 = Uuid::new_v4();
    let buyer = Uuid::new_v4();

    let mut conn = db.pool.acquire().await.unwrap();
    repository::create_order(&mut *conn, buyer, &validated_req("seller-1", seller1))
        .await
        .unwrap();

    let mut conn2 = db.pool.acquire().await.unwrap();
    repository::create_order(&mut *conn2, buyer, &validated_req("seller-2", seller2))
        .await
        .unwrap();

    let orders = repository::list_orders_by_seller(
        &db.pool,
        seller1,
        &PaginationParams::default(),
        &no_filter(),
    )
    .await
    .unwrap();

    assert_eq!(orders.len(), 1, "only seller1's items");
}
