use payment::gateway::mock::MockPaymentGateway;
use payment::ledger::repository;
use payment::ledger::value_objects::PaymentState;
use payment::payments::service;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::common::{test_app_state, test_db};

// ── Authorize payment ───────────────────────────────────────

#[tokio::test]
async fn authorize_payment_creates_posted_authorization() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();
    let amount = Decimal::new(5000, 2); // $50.00

    service::authorize_payment(&state, &gateway, order_id, amount, "USD")
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(txs.len(), 1);
    assert_eq!(
        repository::derive_payment_state(&txs),
        PaymentState::Authorized
    );

    // Should have entries
    let entries = repository::list_entries_by_transaction(&db.pool, txs[0].id)
        .await
        .unwrap();
    assert_eq!(entries.len(), 2, "debit + credit pair");
}

#[tokio::test]
async fn authorize_payment_writes_outbox_event() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    service::authorize_payment(&state, &gateway, order_id, Decimal::new(5000, 2), "USD")
        .await
        .unwrap();

    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE event_type = 'PaymentAuthorized'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn authorize_payment_idempotent_on_retry() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();
    let amount = Decimal::new(5000, 2);

    service::authorize_payment(&state, &gateway, order_id, amount, "USD")
        .await
        .unwrap();
    service::authorize_payment(&state, &gateway, order_id, amount, "USD")
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(txs.len(), 1, "should not duplicate on retry");
}

#[tokio::test]
async fn authorize_gateway_decline_writes_payment_failed() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_fails();
    let order_id = Uuid::now_v7();

    service::authorize_payment(&state, &gateway, order_id, Decimal::new(5000, 2), "USD")
        .await
        .unwrap(); // returns Ok, but writes PaymentFailed event

    let row: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM outbox_events WHERE event_type = 'PaymentFailed'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
    assert_eq!(row.0, 1);
}

#[tokio::test]
async fn authorize_tampered_amount_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::tampered_amount(Decimal::new(9999, 2));
    let order_id = Uuid::now_v7();

    let result =
        service::authorize_payment(&state, &gateway, order_id, Decimal::new(5000, 2), "USD").await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("tampering"), "got: {err}");
}

#[tokio::test]
async fn authorize_below_min_amount_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    let result = service::authorize_payment(
        &state,
        &gateway,
        order_id,
        Decimal::new(10, 2), // $0.10 — below $0.50 min
        "USD",
    )
    .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn authorize_unsupported_currency_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    let result =
        service::authorize_payment(&state, &gateway, order_id, Decimal::new(5000, 2), "XYZ").await;

    assert!(result.is_err());
}

// ── Capture payment ─────────────────────────────────────────

#[tokio::test]
async fn capture_authorized_payment_succeeds() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    service::authorize_payment(&state, &gateway, order_id, Decimal::new(5000, 2), "USD")
        .await
        .unwrap();
    service::capture_payment(&state, &gateway, order_id)
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(
        repository::derive_payment_state(&txs),
        PaymentState::Captured
    );
}

#[tokio::test]
async fn capture_without_authorization_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    let result = service::capture_payment(&state, &gateway, order_id).await;
    assert!(result.is_err());
}

// ── Void payment ────────────────────────────────────────────

#[tokio::test]
async fn void_authorized_payment_succeeds() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    service::authorize_payment(&state, &gateway, order_id, Decimal::new(5000, 2), "USD")
        .await
        .unwrap();
    service::void_payment(&state, &gateway, order_id)
        .await
        .unwrap();

    let txs = repository::list_transactions_by_order(&db.pool, order_id)
        .await
        .unwrap();
    assert_eq!(repository::derive_payment_state(&txs), PaymentState::Voided);
}

#[tokio::test]
async fn void_without_authorization_fails() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    let result = service::void_payment(&state, &gateway, order_id).await;
    assert!(result.is_err());
}

// ── Full lifecycle ──────────────────────────────────────────

#[tokio::test]
async fn full_authorize_capture_lifecycle() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();
    let amount = Decimal::new(7500, 2); // $75.00

    // Authorize
    service::authorize_payment(&state, &gateway, order_id, amount, "USD")
        .await
        .unwrap();

    // Capture
    service::capture_payment(&state, &gateway, order_id)
        .await
        .unwrap();

    // Check balances
    let balances = repository::get_account_balances(&db.pool, order_id)
        .await
        .unwrap();
    assert!(balances.len() >= 2, "should have accounts with balances");

    // Check outbox events
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT event_type FROM outbox_events WHERE aggregate_id = $1 ORDER BY created_at ASC",
    )
    .bind(order_id)
    .fetch_all(&db.pool)
    .await
    .unwrap();

    let events: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert!(events.contains(&"PaymentAuthorized"));
    assert!(events.contains(&"PaymentCaptured"));
}
