use order::consumers::inventory_events;
use order::consumers::payment_events;
use order::orders::dtos::ValidCreateOrderReq;
use order::orders::repository;
use order::orders::value_objects::{OrderId, OrderStatus};
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use uuid::Uuid;

use crate::common::{sample_create_order_req, test_db};

/// Create an order in the database and return (order_id, buyer_id).
async fn create_test_order(pool: &shared::db::PgPool) -> (Uuid, Uuid) {
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

/// Transition an order to a specific status by walking through the state machine.
async fn advance_order_to(pool: &shared::db::PgPool, order_id: OrderId, target: &OrderStatus) {
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

fn make_envelope(event_type: EventType, order_id: Uuid, extra: serde_json::Value) -> EventEnvelope {
    let source = match event_type {
        EventType::InventoryReserved | EventType::InventoryReservationFailed => {
            SourceService::Catalog
        }
        EventType::PaymentAuthorized | EventType::PaymentFailed | EventType::PaymentTimedOut => {
            SourceService::Payment
        }
        _ => SourceService::Order,
    };
    let aggregate_type = match event_type {
        EventType::InventoryReserved | EventType::InventoryReservationFailed => {
            AggregateType::Inventory
        }
        EventType::PaymentAuthorized | EventType::PaymentFailed | EventType::PaymentTimedOut => {
            AggregateType::Payment
        }
        _ => AggregateType::Order,
    };

    let mut payload = serde_json::json!({ "order_id": order_id.to_string() });
    if let serde_json::Value::Object(map) = extra {
        for (k, v) in map {
            payload[k] = v;
        }
    }

    let metadata = EventMetadata::new(event_type, aggregate_type, order_id, source);
    EventEnvelope::new(metadata, payload)
}

// ── handle_inventory_reserved ─────────────────────────────────

#[tokio::test]
async fn handle_inventory_reserved_transitions_to_inventory_reserved() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    let envelope = make_envelope(
        EventType::InventoryReserved,
        order_id,
        serde_json::json!({}),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    inventory_events::handle_inventory_reserved(&mut *conn, &envelope)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::InventoryReserved);
}

#[tokio::test]
async fn handle_inventory_reserved_rejects_non_pending_order() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    // Advance to InventoryReserved first
    advance_order_to(
        &db.pool,
        OrderId::new(order_id),
        &OrderStatus::InventoryReserved,
    )
    .await;

    let envelope = make_envelope(
        EventType::InventoryReserved,
        order_id,
        serde_json::json!({}),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    let result = inventory_events::handle_inventory_reserved(&mut *conn, &envelope).await;
    assert!(result.is_err(), "should reject double transition");
}

// ── handle_inventory_reservation_failed ───────────────────────

#[tokio::test]
async fn handle_inventory_reservation_failed_cancels_order() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    let envelope = make_envelope(
        EventType::InventoryReservationFailed,
        order_id,
        serde_json::json!({ "reason": "Insufficient stock for SKU abc" }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    inventory_events::handle_inventory_reservation_failed(&mut *conn, &envelope)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(
        order.cancelled_reason.as_deref(),
        Some("Insufficient stock for SKU abc")
    );
}

#[tokio::test]
async fn handle_inventory_reservation_failed_uses_default_reason() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    // No "reason" field in payload
    let envelope = make_envelope(
        EventType::InventoryReservationFailed,
        order_id,
        serde_json::json!({}),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    inventory_events::handle_inventory_reservation_failed(&mut *conn, &envelope)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(
        order.cancelled_reason.as_deref(),
        Some("Inventory reservation failed")
    );
}

// ── handle_payment_authorized ─────────────────────────────────

#[tokio::test]
async fn handle_payment_authorized_confirms_order_and_writes_outbox() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    // Must be in InventoryReserved state for PaymentAuthorized transition
    advance_order_to(
        &db.pool,
        OrderId::new(order_id),
        &OrderStatus::InventoryReserved,
    )
    .await;

    let envelope = make_envelope(
        EventType::PaymentAuthorized,
        order_id,
        serde_json::json!({}),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    payment_events::handle_payment_authorized(&mut *conn, &envelope)
        .await
        .unwrap();

    // Order should be auto-confirmed (PaymentAuthorized -> Confirmed)
    let order = repository::get_order_by_id(&db.pool, OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Confirmed);

    // OrderConfirmed outbox event should be written
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'OrderConfirmed' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn handle_payment_authorized_rejects_pending_order() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;
    // Order is in Pending state — PaymentAuthorized requires InventoryReserved

    let envelope = make_envelope(
        EventType::PaymentAuthorized,
        order_id,
        serde_json::json!({}),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    let result = payment_events::handle_payment_authorized(&mut *conn, &envelope).await;
    assert!(result.is_err(), "should reject transition from Pending");
}

// ── handle_payment_failed ─────────────────────────────────────

#[tokio::test]
async fn handle_payment_failed_cancels_order_and_writes_outbox() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    // Advance to InventoryReserved (a cancellable state)
    advance_order_to(
        &db.pool,
        OrderId::new(order_id),
        &OrderStatus::InventoryReserved,
    )
    .await;

    let envelope = make_envelope(
        EventType::PaymentFailed,
        order_id,
        serde_json::json!({ "reason": "Gateway declined" }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    payment_events::handle_payment_failed(&mut *conn, &envelope)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(order.cancelled_reason.as_deref(), Some("Gateway declined"));

    // OrderCancelled outbox event should be written
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'OrderCancelled' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn handle_payment_failed_uses_default_reason() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    advance_order_to(
        &db.pool,
        OrderId::new(order_id),
        &OrderStatus::InventoryReserved,
    )
    .await;

    // No "reason" field in payload
    let envelope = make_envelope(EventType::PaymentFailed, order_id, serde_json::json!({}));

    let mut conn = db.pool.acquire().await.unwrap();
    payment_events::handle_payment_failed(&mut *conn, &envelope)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.cancelled_reason.as_deref(), Some("Payment failed"));
}

// ── handle_payment_timed_out ──────────────────────────────────

#[tokio::test]
async fn handle_payment_timed_out_cancels_order() {
    let db = test_db().await;
    let (order_id, _buyer_id) = create_test_order(&db.pool).await;

    advance_order_to(
        &db.pool,
        OrderId::new(order_id),
        &OrderStatus::InventoryReserved,
    )
    .await;

    let envelope = make_envelope(EventType::PaymentTimedOut, order_id, serde_json::json!({}));

    let mut conn = db.pool.acquire().await.unwrap();
    payment_events::handle_payment_timed_out(&mut *conn, &envelope)
        .await
        .unwrap();

    let order = repository::get_order_by_id(&db.pool, OrderId::new(order_id))
        .await
        .unwrap();
    assert_eq!(order.status, OrderStatus::Cancelled);
    assert_eq!(order.cancelled_reason.as_deref(), Some("Payment timed out"));
}
