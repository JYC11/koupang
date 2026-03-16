use axum::http::StatusCode;
use shared::test_utils::auth::{buyer_user, test_token};
use shared::test_utils::http::{
    authed_delete, authed_get, authed_json_request, body_json, json_request,
};
use tower::ServiceExt;
use uuid::Uuid;

use crate::common::{sample_add_item_req, test_app_state, test_redis};

// ── GET /api/v1/cart ────────────────────────────────────────

#[tokio::test]
async fn get_empty_cart_returns_200() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let router = cart::app(state);
    let user = buyer_user();
    let token = test_token(&user);

    let resp = router
        .oneshot(authed_get("/api/v1/cart", &token))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["item_count"].as_u64().unwrap(), 0);
}

#[tokio::test]
async fn get_cart_without_auth_returns_401() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let router = cart::app(state);

    let resp = router
        .oneshot(
            axum::http::Request::builder()
                .uri("/api/v1/cart")
                .method("GET")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── POST /api/v1/cart/items ─────────────────────────────────

#[tokio::test]
async fn add_item_returns_cart_with_new_item() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let router = cart::app(state);
    let user = buyer_user();
    let token = test_token(&user);

    let resp = router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/cart/items",
            &token,
            &sample_add_item_req(),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["item_count"].as_u64().unwrap(), 1);
    assert_eq!(
        body["items"][0]["product_name"].as_str().unwrap(),
        "Test Widget"
    );
}

#[tokio::test]
async fn add_item_without_auth_returns_401() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let router = cart::app(state);

    let resp = router
        .oneshot(json_request(
            "POST",
            "/api/v1/cart/items",
            &sample_add_item_req(),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── PUT /api/v1/cart/items/{sku_id} ─────────────────────────

#[tokio::test]
async fn update_item_quantity_returns_updated_cart() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let user = buyer_user();
    let token = test_token(&user);
    let req = sample_add_item_req();
    let sku_id = req.sku_id;

    let router = cart::app(state.clone());
    router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/cart/items",
            &token,
            &req,
        ))
        .await
        .unwrap();

    let update_body = serde_json::json!({ "quantity": 5 });
    let router2 = cart::app(state);
    let resp = router2
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/v1/cart/items/{}", sku_id),
            &token,
            &update_body,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["items"][0]["quantity"].as_u64().unwrap(), 5);
}

// ── DELETE /api/v1/cart/items/{sku_id} ──────────────────────

#[tokio::test]
async fn remove_item_returns_200() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let user = buyer_user();
    let token = test_token(&user);
    let req = sample_add_item_req();
    let sku_id = req.sku_id;

    let router = cart::app(state.clone());
    router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/cart/items",
            &token,
            &req,
        ))
        .await
        .unwrap();

    let router2 = cart::app(state);
    let resp = router2
        .oneshot(authed_delete(
            &format!("/api/v1/cart/items/{}", sku_id),
            &token,
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
}

// ── DELETE /api/v1/cart ─────────────────────────────────────

#[tokio::test]
async fn clear_cart_returns_200() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let user = buyer_user();
    let token = test_token(&user);

    let router = cart::app(state.clone());
    router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/cart/items",
            &token,
            &sample_add_item_req(),
        ))
        .await
        .unwrap();

    let router2 = cart::app(state.clone());
    let resp = router2
        .oneshot(authed_delete("/api/v1/cart", &token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify cart is empty
    let router3 = cart::app(state);
    let resp2 = router3
        .oneshot(authed_get("/api/v1/cart", &token))
        .await
        .unwrap();
    let body = body_json(resp2).await;
    assert_eq!(body["item_count"].as_u64().unwrap(), 0);
}

// ── POST /api/v1/cart/validate ──────────────────────────────

#[tokio::test]
async fn validate_cart_with_items_returns_200() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let user = buyer_user();
    let token = test_token(&user);

    let router = cart::app(state.clone());
    router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/cart/items",
            &token,
            &sample_add_item_req(),
        ))
        .await
        .unwrap();

    let router2 = cart::app(state);
    let resp = router2
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/cart/validate",
            &token,
            &serde_json::json!({}),
        ))
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body["all_valid"].as_bool().unwrap());
}

// ── User isolation ──────────────────────────────────────────

#[tokio::test]
async fn different_users_see_different_carts() {
    let redis = test_redis().await;
    let state = test_app_state(redis.conn.clone());
    let user1 = buyer_user();
    let user2 = buyer_user();
    let token1 = test_token(&user1);
    let token2 = test_token(&user2);

    // User 1 adds item
    let router = cart::app(state.clone());
    router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/cart/items",
            &token1,
            &sample_add_item_req(),
        ))
        .await
        .unwrap();

    // User 2 sees empty cart
    let router2 = cart::app(state);
    let resp = router2
        .oneshot(authed_get("/api/v1/cart", &token2))
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body["item_count"].as_u64().unwrap(), 0);
}
