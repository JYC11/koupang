use crate::common::{
    sample_add_image_req, sample_create_product_req, sample_create_sku_req, test_app_state,
    test_auth_config, test_db,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use catalog::app;
use catalog::products::dtos::ProductDetailRes;
use shared::auth::jwt::{CurrentUser, JwtService};
use shared::auth::Role;
use shared::test_utils::http::{body_bytes, body_json};
use tower::ServiceExt;
use uuid::Uuid;

/// Generate a valid JWT access token for test requests.
fn test_token(user: &CurrentUser) -> String {
    let jwt_service = JwtService::new(test_auth_config());
    jwt_service
        .generate_access_token(&user.id, "testuser", user.role.clone())
        .unwrap()
}

fn seller() -> CurrentUser {
    CurrentUser {
        id: Uuid::new_v4(),
        role: Role::Seller,
    }
}

fn admin() -> CurrentUser {
    CurrentUser {
        id: Uuid::new_v4(),
        role: Role::Admin,
    }
}

fn json_request(method: &str, uri: &str, body: &impl serde::Serialize) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method(method)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

fn authed_json_request(
    method: &str,
    uri: &str,
    token: &str,
    body: &impl serde::Serialize,
) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method(method)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

fn authed_get(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("GET")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap()
}

fn authed_delete(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("DELETE")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap()
}

/// Helper: create a product via router and return (product_id, seller, token)
async fn create_test_product(pool: &shared::db::PgPool) -> (String, CurrentUser, String) {
    let state = test_app_state(pool.clone());
    let router = app(state);
    let user = seller();
    let token = test_token(&user);

    let resp = router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/products",
            &token,
            &sample_create_product_req(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let product_id = body["data"]["id"].as_str().unwrap().to_string();
    (product_id, user, token)
}

// ── Public endpoint tests ───────────────────────────────────

#[tokio::test]
async fn list_active_products_returns_200() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/products")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert!(body.is_empty()); // no active products yet
}

#[tokio::test]
async fn get_product_detail_returns_200() {
    let db = test_db().await;
    let (product_id, _, _) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/products/{}", product_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let detail: ProductDetailRes = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(detail.product.name, "Test Widget");
    assert!(detail.skus.is_empty());
    assert!(detail.images.is_empty());
}

#[tokio::test]
async fn get_product_by_slug_returns_200() {
    let db = test_db().await;
    create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/products/slug/test-widget")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert_eq!(body["slug"].as_str().unwrap(), "test-widget");
}

#[tokio::test]
async fn get_nonexistent_product_returns_404() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/products/{}", Uuid::new_v4()))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── Protected endpoint tests: Create ────────────────────────

#[tokio::test]
async fn create_product_without_auth_returns_401() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(json_request(
            "POST",
            "/api/v1/products",
            &sample_create_product_req(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_product_returns_product() {
    let db = test_db().await;
    let (product_id, seller, _) = create_test_product(&db.pool).await;

    assert!(!product_id.is_empty());
    // Verify seller_id matches
    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/products/{}", product_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let detail: ProductDetailRes = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(detail.product.seller_id, seller.id.to_string());
}

#[tokio::test]
async fn create_product_with_invalid_body_returns_422() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let user = seller();
    let token = test_token(&user);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/products")
                .method("POST")
                .header("content-type", "application/json")
                .header("authorization", format!("Bearer {}", token))
                .body(Body::from(r#"{"name":"test"}"#)) // missing base_price
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── Protected endpoint tests: Update ────────────────────────

#[tokio::test]
async fn update_own_product_returns_200() {
    let db = test_db().await;
    let (product_id, _, token) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let update = serde_json::json!({ "name": "Updated Widget" });
    let resp = router
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/v1/products/{}", product_id),
            &token,
            &update,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn update_other_sellers_product_returns_403() {
    let db = test_db().await;
    let (product_id, _, _) = create_test_product(&db.pool).await;

    // Different seller
    let other = seller();
    let other_token = test_token(&other);

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let update = serde_json::json!({ "name": "Hacked" });
    let resp = router
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/v1/products/{}", product_id),
            &other_token,
            &update,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_can_update_any_product() {
    let db = test_db().await;
    let (product_id, _, _) = create_test_product(&db.pool).await;

    let admin = admin();
    let admin_token = test_token(&admin);

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let update = serde_json::json!({ "name": "Admin Updated" });
    let resp = router
        .oneshot(authed_json_request(
            "PUT",
            &format!("/api/v1/products/{}", product_id),
            &admin_token,
            &update,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Protected endpoint tests: Delete ────────────────────────

#[tokio::test]
async fn delete_own_product_returns_200() {
    let db = test_db().await;
    let (product_id, _, token) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .oneshot(authed_delete(
            &format!("/api/v1/products/{}", product_id),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify it's gone
    let state2 = test_app_state(db.pool.clone());
    let router2 = app(state2);
    let resp = router2
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/products/{}", product_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_without_auth_returns_401() {
    let db = test_db().await;
    let (product_id, _, _) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/products/{}", product_id))
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Seller/me endpoint ──────────────────────────────────────

#[tokio::test]
async fn list_my_products_returns_only_owned() {
    let db = test_db().await;
    let (_, seller, token) = create_test_product(&db.pool).await;

    // Create another product by a different seller
    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let other = crate::common::seller_user();
    let other_token = test_token(&other);
    let mut req2 = crate::common::sample_create_product_req_2();
    req2.slug = Some("other-product".to_string());
    let resp = router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/products",
            &other_token,
            &req2,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // My products should only include the first seller's product
    let state2 = test_app_state(db.pool.clone());
    let router2 = app(state2);
    let resp = router2
        .oneshot(authed_get("/api/v1/products/seller/me", &token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let products: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(products.len(), 1);
    assert_eq!(
        products[0]["seller_id"].as_str().unwrap(),
        seller.id.to_string()
    );
}

// ── SKU endpoints ───────────────────────────────────────────

#[tokio::test]
async fn create_and_list_skus_via_router() {
    let db = test_db().await;
    let (product_id, _, token) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    // Create SKU
    let resp = router
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/products/{}/skus", product_id),
            &token,
            &sample_create_sku_req(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert_eq!(body["data"]["sku_code"].as_str().unwrap(), "WIDGET-BLUE-XL");

    // List SKUs
    let resp = router
        .oneshot(authed_get(
            &format!("/api/v1/products/{}/skus", product_id),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let skus: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(skus.len(), 1);
}

#[tokio::test]
async fn adjust_stock_via_router() {
    let db = test_db().await;
    let (product_id, _, token) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    // Create SKU
    let resp = router
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/products/{}/skus", product_id),
            &token,
            &sample_create_sku_req(),
        ))
        .await
        .unwrap();
    let body = body_json(resp).await;
    let sku_id = body["data"]["id"].as_str().unwrap().to_string();

    // Adjust stock
    let stock_req = serde_json::json!({ "delta": -50 });
    let resp = router
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/products/skus/{}/stock", sku_id),
            &token,
            &stock_req,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── Image endpoints ─────────────────────────────────────────

#[tokio::test]
async fn add_and_list_images_via_router() {
    let db = test_db().await;
    let (product_id, _, token) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    // Add image
    let resp = router
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/products/{}/images", product_id),
            &token,
            &sample_add_image_req(),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert!(body["data"]["is_primary"].as_bool().unwrap());

    // List images
    let resp = router
        .oneshot(authed_get(
            &format!("/api/v1/products/{}/images", product_id),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let images: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(images.len(), 1);
}

#[tokio::test]
async fn delete_image_via_router() {
    let db = test_db().await;
    let (product_id, _, token) = create_test_product(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    // Add image
    let resp = router
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/products/{}/images", product_id),
            &token,
            &sample_add_image_req(),
        ))
        .await
        .unwrap();
    let body = body_json(resp).await;
    let image_id = body["data"]["id"].as_str().unwrap().to_string();

    // Delete image
    let resp = router
        .clone()
        .oneshot(authed_delete(
            &format!("/api/v1/products/{}/images/{}", product_id, image_id),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify it's gone
    let resp = router
        .oneshot(authed_get(
            &format!("/api/v1/products/{}/images", product_id),
            &token,
        ))
        .await
        .unwrap();
    let images: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert!(images.is_empty());
}
