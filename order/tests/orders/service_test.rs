use order::orders::service;
use order::orders::value_objects::OrderId;
use shared::db::pagination_support::PaginationParams;
use shared::test_utils::auth::{buyer_user, seller_user};
use uuid::Uuid;

use crate::common::{sample_create_order_req, test_app_state, test_db};

// ── Create order ────────────────────────────────────────────

#[tokio::test]
async fn create_order_returns_pending_order() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let seller = Uuid::new_v4();

    let req = sample_create_order_req(seller);
    let order = service::create_order(&state, &buyer, "svc-create-1", req)
        .await
        .unwrap();

    assert_eq!(order.buyer_id, buyer.id.to_string());
    assert_eq!(
        order.status,
        order::orders::value_objects::OrderStatus::Pending
    );
    assert_eq!(order.currency, "USD");
}

#[tokio::test]
async fn create_order_writes_outbox_event() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let seller = Uuid::new_v4();

    let req = sample_create_order_req(seller);
    service::create_order(&state, &buyer, "svc-outbox-1", req)
        .await
        .unwrap();

    // Verify outbox event was created
    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE event_type = 'OrderCreated'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row.0, 1, "should have one OrderCreated outbox event");
}

#[tokio::test]
async fn create_order_idempotency_returns_same_order() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let seller = Uuid::new_v4();

    let req1 = sample_create_order_req(seller);
    let order1 = service::create_order(&state, &buyer, "svc-idem-1", req1)
        .await
        .unwrap();

    let req2 = sample_create_order_req(seller);
    let order2 = service::create_order(&state, &buyer, "svc-idem-1", req2)
        .await
        .unwrap();

    assert_eq!(order1.id, order2.id, "same key should return same order");
}

#[tokio::test]
async fn create_order_with_empty_items_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();

    let req = order::orders::dtos::CreateOrderReq {
        items: vec![],
        currency: Some("USD".to_string()),
        shipping_address: crate::common::sample_shipping_address(),
    };
    let result = service::create_order(&state, &buyer, "svc-empty-1", req).await;
    assert!(result.is_err());
}

// ── Get order detail ────────────────────────────────────────

#[tokio::test]
async fn get_order_detail_returns_order_with_items() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let seller = Uuid::new_v4();

    let req = sample_create_order_req(seller);
    let order = service::create_order(&state, &buyer, "svc-detail-1", req)
        .await
        .unwrap();

    let order_id = OrderId::new(order.id.parse().unwrap());
    let detail = service::get_order_detail(&state, &buyer, order_id)
        .await
        .unwrap();

    assert_eq!(detail.order.id, order.id);
    assert_eq!(detail.items.len(), 1);
    assert_eq!(detail.items[0].product_name, "Test Widget");
}

#[tokio::test]
async fn get_order_detail_denies_other_buyer() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let other_buyer = buyer_user();
    let seller = Uuid::new_v4();

    let order = service::create_order(
        &state,
        &buyer,
        "svc-access-1",
        sample_create_order_req(seller),
    )
    .await
    .unwrap();

    let order_id = OrderId::new(order.id.parse().unwrap());
    let result = service::get_order_detail(&state, &other_buyer, order_id).await;
    assert!(result.is_err(), "other buyer should be denied");
}

// ── List my orders ──────────────────────────────────────────

#[tokio::test]
async fn list_my_orders_returns_only_buyers_orders() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer1 = buyer_user();
    let buyer2 = buyer_user();
    let seller = Uuid::new_v4();

    service::create_order(
        &state,
        &buyer1,
        "svc-list-1",
        sample_create_order_req(seller),
    )
    .await
    .unwrap();
    service::create_order(
        &state,
        &buyer2,
        "svc-list-2",
        sample_create_order_req(seller),
    )
    .await
    .unwrap();

    let filter = order::orders::dtos::OrderFilter { status: None };
    let result = service::list_my_orders(&state, buyer1.id, PaginationParams::default(), filter)
        .await
        .unwrap();

    assert_eq!(result.items.len(), 1);
}

// ── Cancel order ────────────────────────────────────────────

#[tokio::test]
async fn cancel_pending_order_succeeds() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let seller = Uuid::new_v4();

    let order = service::create_order(
        &state,
        &buyer,
        "svc-cancel-1",
        sample_create_order_req(seller),
    )
    .await
    .unwrap();

    let order_id = OrderId::new(order.id.parse().unwrap());
    service::cancel_order(
        &state,
        &buyer,
        order_id,
        Some("changed my mind".to_string()),
    )
    .await
    .unwrap();

    let detail = service::get_order_detail(&state, &buyer, order_id)
        .await
        .unwrap();
    assert_eq!(
        detail.order.status,
        order::orders::value_objects::OrderStatus::Cancelled
    );
    assert_eq!(
        detail.order.cancelled_reason.as_deref(),
        Some("changed my mind")
    );
}

#[tokio::test]
async fn cancel_writes_outbox_event() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let seller = Uuid::new_v4();

    let order = service::create_order(
        &state,
        &buyer,
        "svc-cancel-outbox-1",
        sample_create_order_req(seller),
    )
    .await
    .unwrap();

    let order_id = OrderId::new(order.id.parse().unwrap());
    service::cancel_order(&state, &buyer, order_id, None)
        .await
        .unwrap();

    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE event_type = 'OrderCancelled'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn cancel_already_cancelled_order_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let seller = Uuid::new_v4();

    let order = service::create_order(
        &state,
        &buyer,
        "svc-double-cancel",
        sample_create_order_req(seller),
    )
    .await
    .unwrap();

    let order_id = OrderId::new(order.id.parse().unwrap());
    service::cancel_order(&state, &buyer, order_id, None)
        .await
        .unwrap();

    let result = service::cancel_order(&state, &buyer, order_id, None).await;
    assert!(result.is_err(), "cancelling again should fail");
}

#[tokio::test]
async fn cancel_other_buyers_order_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let other = buyer_user();
    let seller = Uuid::new_v4();

    let order = service::create_order(
        &state,
        &buyer,
        "svc-cancel-access",
        sample_create_order_req(seller),
    )
    .await
    .unwrap();

    let order_id = OrderId::new(order.id.parse().unwrap());
    let result = service::cancel_order(&state, &other, order_id, None).await;
    assert!(result.is_err(), "other buyer cannot cancel");
}
