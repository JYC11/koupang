use axum::http::StatusCode;
use payment::gateway::mock::MockPaymentGateway;
use payment::payments::service;
use rust_decimal::Decimal;
use shared::test_utils::auth::{buyer_user, test_token};
use shared::test_utils::http::{authed_get, body_json};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{test_app_state, test_db};

// ── GET /api/v1/payments/{order_id} ─────────────────────────

#[tokio::test]
async fn get_payment_status_new_returns_200() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = payment::app(state);
    let user = buyer_user();
    let token = test_token(&user);
    let order_id = Uuid::now_v7();

    let resp = router
        .oneshot(authed_get(
            &format!("/api/v1/payments/{}", order_id),
            &token,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["state"].as_str().unwrap(), "new");
    assert!(body["transactions"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn get_payment_status_after_authorize() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let gateway = MockPaymentGateway::always_succeeds();
    let order_id = Uuid::now_v7();

    service::authorize_payment(&state, &gateway, order_id, Decimal::new(5000, 2), "USD")
        .await
        .unwrap();

    let router = payment::app(state);
    let user = buyer_user();
    let token = test_token(&user);

    let resp = router
        .oneshot(authed_get(
            &format!("/api/v1/payments/{}", order_id),
            &token,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["state"].as_str().unwrap(), "authorized");

    let txs = body["transactions"].as_array().unwrap();
    assert_eq!(txs.len(), 1);
    assert_eq!(
        txs[0]["transaction_type"].as_str().unwrap(),
        "authorization"
    );
    assert_eq!(txs[0]["status"].as_str().unwrap(), "posted");

    let entries = txs[0]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2, "debit + credit pair");
}

#[tokio::test]
async fn get_payment_status_after_full_lifecycle() {
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

    let router = payment::app(state);
    let user = buyer_user();
    let token = test_token(&user);

    let resp = router
        .oneshot(authed_get(
            &format!("/api/v1/payments/{}", order_id),
            &token,
        ))
        .await
        .unwrap();

    let body = body_json(resp).await;
    assert_eq!(body["state"].as_str().unwrap(), "captured");

    let txs = body["transactions"].as_array().unwrap();
    assert_eq!(txs.len(), 2);

    let balances = body["balances"].as_array().unwrap();
    assert!(!balances.is_empty());
}

#[tokio::test]
async fn get_payment_status_without_auth_returns_401() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = payment::app(state);

    let resp = router
        .oneshot(
            axum::http::Request::builder()
                .uri(&format!("/api/v1/payments/{}", Uuid::now_v7()))
                .method("GET")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
