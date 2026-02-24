use crate::common::{
    create_test_brand_named, create_test_category_named, test_app_state, test_auth_config, test_db,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use catalog::app;
use catalog::brands::dtos::{BrandRes, CreateBrandReq};
use catalog::categories::dtos::CategoryRes;
use shared::auth::jwt::{CurrentUser, JwtService};
use shared::auth::Role;
use shared::test_utils::http::{body_bytes, body_json};
use tower::ServiceExt;
use uuid::Uuid;

fn test_token(user: &CurrentUser) -> String {
    let jwt_service = JwtService::new(test_auth_config());
    jwt_service
        .generate_access_token(&user.id, "testuser", user.role.clone())
        .unwrap()
}

fn admin() -> CurrentUser {
    CurrentUser {
        id: Uuid::new_v4(),
        role: Role::Admin,
    }
}

fn seller() -> CurrentUser {
    CurrentUser {
        id: Uuid::new_v4(),
        role: Role::Seller,
    }
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

fn authed_delete(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("DELETE")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
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

/// Helper: create a brand via router
async fn create_brand_via_router(pool: &shared::db::PgPool) -> (BrandRes, String) {
    let state = test_app_state(pool.clone());
    let router = app(state);
    let user = admin();
    let token = test_token(&user);

    let req = CreateBrandReq {
        name: "Samsung".to_string(),
        slug: None,
        description: Some("Korean electronics".to_string()),
        logo_url: Some("https://cdn.example.com/samsung.png".to_string()),
    };

    let resp = router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/brands",
            &token,
            &req,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let brand: BrandRes = serde_json::from_value(body["data"].clone()).unwrap();
    (brand, token)
}

// ── Public endpoints ────────────────────────────────────────

#[tokio::test]
async fn list_brands_returns_200() {
    let db = test_db().await;
    create_test_brand_named(&db.pool, "Samsung").await;
    create_test_brand_named(&db.pool, "Apple").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/brands")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let brands: Vec<BrandRes> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(brands.len(), 2);
}

#[tokio::test]
async fn get_brand_returns_200() {
    let db = test_db().await;
    let brand_id = create_test_brand_named(&db.pool, "Samsung").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/brands/{}", brand_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let brand: BrandRes = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(brand.name, "Samsung");
}

#[tokio::test]
async fn get_brand_by_slug_returns_200() {
    let db = test_db().await;
    create_test_brand_named(&db.pool, "LG Electronics").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/brands/slug/lg-electronics")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let brand: BrandRes = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(brand.name, "LG Electronics");
}

#[tokio::test]
async fn get_nonexistent_brand_returns_404() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/brands/{}", Uuid::new_v4()))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_categories_for_brand_returns_200() {
    let db = test_db().await;
    let brand_id = create_test_brand_named(&db.pool, "Samsung").await;
    let cat_id = create_test_category_named(&db.pool, "Electronics").await;

    // Associate via SQL (already tested at service level)
    sqlx::query("INSERT INTO brand_categories (brand_id, category_id) VALUES ($1, $2)")
        .bind(brand_id)
        .bind(cat_id)
        .execute(&db.pool)
        .await
        .unwrap();

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/brands/{}/categories", brand_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let categories: Vec<CategoryRes> =
        serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(categories.len(), 1);
    assert_eq!(categories[0].name, "Electronics");
}

// ── Protected endpoints ─────────────────────────────────────

#[tokio::test]
async fn create_brand_without_auth_returns_401() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let req = CreateBrandReq {
        name: "Hacked".to_string(),
        slug: None,
        description: None,
        logo_url: None,
    };

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/brands")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_brand_as_seller_returns_403() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let user = seller();
    let token = test_token(&user);

    let req = CreateBrandReq {
        name: "Forbidden".to_string(),
        slug: None,
        description: None,
        logo_url: None,
    };

    let resp = router
        .oneshot(authed_json_request("POST", "/api/v1/brands", &token, &req))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn create_brand_as_admin_returns_200() {
    let db = test_db().await;
    let (brand, _) = create_brand_via_router(&db.pool).await;
    assert_eq!(brand.name, "Samsung");
    assert!(!brand.id.is_empty());
}

#[tokio::test]
async fn delete_brand_via_router() {
    let db = test_db().await;
    let (brand, token) = create_brand_via_router(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .oneshot(authed_delete(
            &format!("/api/v1/brands/{}", brand.id),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify deleted
    let state2 = test_app_state(db.pool.clone());
    let router2 = app(state2);
    let resp = router2
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/brands/{}", brand.id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn associate_category_via_router() {
    let db = test_db().await;
    let (brand, token) = create_brand_via_router(&db.pool).await;
    let cat_id = create_test_category_named(&db.pool, "Electronics").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let body = serde_json::json!({ "category_id": cat_id });
    let resp = router
        .clone()
        .oneshot(authed_json_request(
            "POST",
            &format!("/api/v1/brands/{}/categories", brand.id),
            &token,
            &body,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify via list
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/brands/{}/categories", brand.id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let categories: Vec<CategoryRes> =
        serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(categories.len(), 1);
}

#[tokio::test]
async fn disassociate_category_via_router() {
    let db = test_db().await;
    let (brand, token) = create_brand_via_router(&db.pool).await;
    let cat_id = create_test_category_named(&db.pool, "Electronics").await;
    let brand_uuid = uuid::Uuid::parse_str(&brand.id).unwrap();

    // Associate first
    sqlx::query("INSERT INTO brand_categories (brand_id, category_id) VALUES ($1, $2)")
        .bind(brand_uuid)
        .bind(cat_id)
        .execute(&db.pool)
        .await
        .unwrap();

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .clone()
        .oneshot(authed_delete(
            &format!("/api/v1/brands/{}/categories/{}", brand.id, cat_id),
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify empty
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/brands/{}/categories", brand.id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let categories: Vec<CategoryRes> =
        serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert!(categories.is_empty());
}
