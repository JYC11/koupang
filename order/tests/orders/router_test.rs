use axum::body::Body;
use axum::http::{Request, StatusCode};
use shared::test_utils::auth::{buyer_user, test_token};
use shared::test_utils::http::{authed_get, authed_json_request, body_json, json_request};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{sample_create_order_req, test_app_state, test_db};

fn create_order_request(token: &str, body: &impl serde::Serialize, key: &str) -> Request<Body> {
    Request::builder()
        .uri("/api/v1/orders")
        .method("POST")
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .header("idempotency-key", key)
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

// ── Create order ────────────────────────────────────────────

#[tokio::test]
async fn create_order_returns_202() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = order::app(state);
    let user = buyer_user();
    let token = test_token(&user);
    let seller = Uuid::new_v4();

    let resp = router
        .oneshot(create_order_request(
            &token,
            &sample_create_order_req(seller),
            "router-1",
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let body = body_json(resp).await;
    assert_eq!(body["status"].as_str().unwrap(), "pending");
    assert_eq!(body["currency"].as_str().unwrap(), "USD");
}

#[tokio::test]
async fn create_order_without_auth_returns_401() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = order::app(state);
    let seller = Uuid::new_v4();

    let resp = router
        .oneshot(json_request(
            "POST",
            "/api/v1/orders",
            &sample_create_order_req(seller),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_order_without_idempotency_key_returns_400() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = order::app(state);
    let user = buyer_user();
    let token = test_token(&user);
    let seller = Uuid::new_v4();

    let resp = router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/orders",
            &token,
            &sample_create_order_req(seller),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn create_order_idempotency_returns_same_id() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = buyer_user();
    let token = test_token(&user);
    let seller = Uuid::new_v4();

    let router1 = order::app(state.clone());
    let resp1 = router1
        .oneshot(create_order_request(
            &token,
            &sample_create_order_req(seller),
            "idem-router-1",
        ))
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::ACCEPTED);
    let body1 = body_json(resp1).await;

    let router2 = order::app(state);
    let resp2 = router2
        .oneshot(create_order_request(
            &token,
            &sample_create_order_req(seller),
            "idem-router-1",
        ))
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::ACCEPTED);
    let body2 = body_json(resp2).await;

    assert_eq!(body1["id"], body2["id"]);
}

// ── Get order detail ────────────────────────────────────────

#[tokio::test]
async fn get_order_detail_returns_200() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = buyer_user();
    let token = test_token(&user);
    let seller = Uuid::new_v4();

    let router = order::app(state.clone());
    let resp = router
        .oneshot(create_order_request(
            &token,
            &sample_create_order_req(seller),
            "detail-router-1",
        ))
        .await
        .unwrap();
    let order_id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let router2 = order::app(state);
    let resp2 = router2
        .oneshot(authed_get(&format!("/api/v1/orders/{}", order_id), &token))
        .await
        .unwrap();

    assert_eq!(resp2.status(), StatusCode::OK);

    let body = body_json(resp2).await;
    assert_eq!(body["id"].as_str().unwrap(), order_id);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["product_name"].as_str().unwrap(), "Test Widget");
}

#[tokio::test]
async fn get_nonexistent_order_returns_404() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = order::app(state);
    let user = buyer_user();
    let token = test_token(&user);

    let resp = router
        .oneshot(authed_get(
            &format!("/api/v1/orders/{}", Uuid::new_v4()),
            &token,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── List orders ─────────────────────────────────────────────

#[tokio::test]
async fn list_my_orders_returns_200() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = buyer_user();
    let token = test_token(&user);
    let seller = Uuid::new_v4();

    let router = order::app(state.clone());
    router
        .oneshot(create_order_request(
            &token,
            &sample_create_order_req(seller),
            "list-router-1",
        ))
        .await
        .unwrap();

    let router2 = order::app(state);
    let resp = router2
        .oneshot(authed_get("/api/v1/orders", &token))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
}

// ── Cancel order ────────────────────────────────────────────

#[tokio::test]
async fn cancel_order_returns_200() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = buyer_user();
    let token = test_token(&user);
    let seller = Uuid::new_v4();

    let router = order::app(state.clone());
    let resp = router
        .oneshot(create_order_request(
            &token,
            &sample_create_order_req(seller),
            "cancel-router-1",
        ))
        .await
        .unwrap();
    let order_id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let router2 = order::app(state);
    let cancel_body = serde_json::json!({ "reason": "changed my mind" });
    let resp2 = router2
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/orders/{}/cancel", order_id),
            &token,
            &cancel_body,
        ))
        .await
        .unwrap();

    assert_eq!(resp2.status(), StatusCode::OK);
}

#[tokio::test]
async fn cancel_other_buyers_order_returns_403() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let buyer = buyer_user();
    let other = buyer_user();
    let token_buyer = test_token(&buyer);
    let token_other = test_token(&other);
    let seller = Uuid::new_v4();

    let router = order::app(state.clone());
    let resp = router
        .oneshot(create_order_request(
            &token_buyer,
            &sample_create_order_req(seller),
            "cancel-access-1",
        ))
        .await
        .unwrap();
    let order_id = body_json(resp).await["id"].as_str().unwrap().to_string();

    let router2 = order::app(state);
    let cancel_body = serde_json::json!({ "reason": null });
    let resp2 = router2
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/orders/{}/cancel", order_id),
            &token_other,
            &cancel_body,
        ))
        .await
        .unwrap();

    assert_eq!(resp2.status(), StatusCode::FORBIDDEN);
}
