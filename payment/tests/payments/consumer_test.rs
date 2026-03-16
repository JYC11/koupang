use payment::consumers::inventory_events;
use payment::consumers::order_events;
use payment::gateway::mock::MockPaymentGateway;
use payment::ledger::repository;
use payment::ledger::value_objects::PaymentState;
use payment::payments::service;
use rust_decimal::Decimal;
use shared::events::{AggregateType, EventEnvelope, EventMetadata, EventType, SourceService};
use uuid::Uuid;

use crate::common::{test_app_state, test_db};

fn make_envelope(event_type: EventType, order_id: Uuid, extra: serde_json::Value) -> EventEnvelope {
    let (source, agg_type) = match event_type {
        EventType::InventoryReserved | EventType::InventoryReservationFailed => {
            (SourceService::Catalog, AggregateType::Inventory)
        }
        EventType::OrderConfirmed | EventType::OrderCancelled => {
            (SourceService::Order, AggregateType::Order)
        }
        _ => (SourceService::Payment, AggregateType::Payment),
    };

    let mut payload = serde_json::json!({ "order_id": order_id.to_string() });
    if let serde_json::Value::Object(map) = extra {
        for (k, v) in map {
            payload[k] = v;
        }
    }

    let metadata = EventMetadata::new(event_type, agg_type, order_id, source);
    EventEnvelope::new(metadata, payload)
}

// ── handle_inventory_reserved (authorize payment) ─────────────

#[tokio::test]
async fn handle_inventory_reserved_authorizes_payment() {
    let db = test_db().await;
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    let envelope = make_envelope(
        EventType::InventoryReserved,
        order_id,
        serde_json::json!({
            "total_amount": "50.00",
            "currency": "USD",
            "buyer_id": Uuid::new_v4().to_string(),
        }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    inventory_events::handle_inventory_reserved(&mut *conn, &db.pool, &gateway, &envelope)
        .await
        .unwrap();

    // Verify authorization was created and posted
    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(txs.len(), 1);
    assert_eq!(
        repository::derive_payment_state(&txs),
        PaymentState::Authorized
    );

    // Verify PaymentAuthorized outbox event
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'PaymentAuthorized' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn handle_inventory_reserved_gateway_decline_writes_payment_failed() {
    let db = test_db().await;
    let gateway = MockPaymentGateway::always_fails();
    let order_id = Uuid::now_v7();

    let envelope = make_envelope(
        EventType::InventoryReserved,
        order_id,
        serde_json::json!({
            "total_amount": "50.00",
            "currency": "USD",
            "buyer_id": Uuid::new_v4().to_string(),
        }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    // authorize_payment_on_tx returns Ok even on gateway decline (writes PaymentFailed outbox)
    inventory_events::handle_inventory_reserved(&mut *conn, &db.pool, &gateway, &envelope)
        .await
        .unwrap();

    // No authorized transaction
    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(repository::derive_payment_state(&txs), PaymentState::New);

    // PaymentFailed outbox event should exist
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'PaymentFailed' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn handle_inventory_reserved_creates_ledger_entries() {
    let db = test_db().await;
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    let envelope = make_envelope(
        EventType::InventoryReserved,
        order_id,
        serde_json::json!({
            "total_amount": "75.00",
            "currency": "USD",
            "buyer_id": Uuid::new_v4().to_string(),
        }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    inventory_events::handle_inventory_reserved(&mut *conn, &db.pool, &gateway, &envelope)
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    let entries = repository::list_entries_by_transaction(&db.pool, txs[0].id)
        .await
        .unwrap();
    assert_eq!(entries.len(), 2, "should have debit + credit pair");

    let amount = Decimal::new(7500, 2);
    for entry in &entries {
        assert_eq!(entry.amount, amount);
    }
}

// ── handle_order_confirmed (capture payment) ──────────────────

#[tokio::test]
async fn handle_order_confirmed_captures_authorized_payment() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();
    let amount = Decimal::new(5000, 2);

    // First authorize via pool-based path (easier setup)
    service::authorize_payment(&state, &gateway, order_id, amount, "USD")
        .await
        .unwrap();

    let envelope = make_envelope(
        EventType::OrderConfirmed,
        order_id,
        serde_json::json!({ "buyer_id": Uuid::new_v4().to_string() }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    order_events::handle_order_confirmed(&mut *conn, &db.pool, &gateway, &envelope)
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(
        repository::derive_payment_state(&txs),
        PaymentState::Captured
    );

    // PaymentCaptured outbox event
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'PaymentCaptured' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn handle_order_confirmed_without_authorization_fails() {
    let db = test_db().await;
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    let envelope = make_envelope(
        EventType::OrderConfirmed,
        order_id,
        serde_json::json!({ "buyer_id": Uuid::new_v4().to_string() }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    let result =
        order_events::handle_order_confirmed(&mut *conn, &db.pool, &gateway, &envelope).await;
    assert!(result.is_err(), "should fail without prior authorization");
}

// ── handle_order_cancelled (void payment) ─────────────────────

#[tokio::test]
async fn handle_order_cancelled_voids_authorized_payment() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();
    let amount = Decimal::new(5000, 2);

    // Authorize first
    service::authorize_payment(&state, &gateway, order_id, amount, "USD")
        .await
        .unwrap();

    let envelope = make_envelope(
        EventType::OrderCancelled,
        order_id,
        serde_json::json!({
            "buyer_id": Uuid::new_v4().to_string(),
            "reason": "Changed mind",
        }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    order_events::handle_order_cancelled(&mut *conn, &db.pool, &gateway, &envelope)
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(repository::derive_payment_state(&txs), PaymentState::Voided);

    // PaymentVoided outbox event
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM outbox_events WHERE event_type = 'PaymentVoided' AND aggregate_id = $1",
    )
    .bind(order_id)
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn handle_order_cancelled_noop_when_payment_is_new() {
    let db = test_db().await;
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    // No authorization exists — payment state is New
    let envelope = make_envelope(
        EventType::OrderCancelled,
        order_id,
        serde_json::json!({
            "buyer_id": Uuid::new_v4().to_string(),
            "reason": "Cancelled before payment",
        }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    order_events::handle_order_cancelled(&mut *conn, &db.pool, &gateway, &envelope)
        .await
        .unwrap();

    // No transactions should have been created
    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert!(txs.is_empty());
}

#[tokio::test]
async fn handle_order_cancelled_noop_when_payment_failed() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway_fail = MockPaymentGateway::always_fails();
    let gateway_ok = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    // Authorize with failing gateway -> PaymentFailed state
    service::authorize_payment(
        &state,
        &gateway_fail,
        order_id,
        Decimal::new(5000, 2),
        "USD",
    )
    .await
    .unwrap();

    let envelope = make_envelope(
        EventType::OrderCancelled,
        order_id,
        serde_json::json!({
            "buyer_id": Uuid::new_v4().to_string(),
            "reason": "Order cancelled after payment failure",
        }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    // Should succeed (no-op) since payment is in Failed state
    order_events::handle_order_cancelled(&mut *conn, &db.pool, &gateway_ok, &envelope)
        .await
        .unwrap();

    // No void transaction should exist
    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(repository::derive_payment_state(&txs), PaymentState::New);
}

// ── Full lifecycle through consumer handlers ──────────────────

#[tokio::test]
async fn full_lifecycle_authorize_then_capture_through_consumers() {
    let db = test_db().await;
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    // 1. InventoryReserved -> authorize
    let reserved_envelope = make_envelope(
        EventType::InventoryReserved,
        order_id,
        serde_json::json!({
            "total_amount": "99.99",
            "currency": "USD",
            "buyer_id": Uuid::new_v4().to_string(),
        }),
    );

    let mut conn = db.pool.acquire().await.unwrap();
    inventory_events::handle_inventory_reserved(&mut *conn, &db.pool, &gateway, &reserved_envelope)
        .await
        .unwrap();

    // 2. OrderConfirmed -> capture
    let confirmed_envelope = make_envelope(
        EventType::OrderConfirmed,
        order_id,
        serde_json::json!({ "buyer_id": Uuid::new_v4().to_string() }),
    );

    let mut conn2 = db.pool.acquire().await.unwrap();
    order_events::handle_order_confirmed(&mut *conn2, &db.pool, &gateway, &confirmed_envelope)
        .await
        .unwrap();

    // Final state should be Captured
    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(
        repository::derive_payment_state(&txs),
        PaymentState::Captured
    );

    // Check outbox events in order: PaymentAuthorized, PaymentCaptured
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT event_type FROM outbox_events WHERE aggregate_id = $1 ORDER BY created_at ASC",
    )
    .bind(order_id)
    .fetch_all(&db.pool)
    .await
    .unwrap();

    let events: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(events, vec!["PaymentAuthorized", "PaymentCaptured"]);
}
