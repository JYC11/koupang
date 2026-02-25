use crate::common::{
    admin, associate_brand_category, create_test_brand_named, create_test_category_named,
    sample_add_image_req, sample_create_product_req, sample_create_sku_req, seller, seller_user,
    test_app_state, test_db, test_token,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use catalog::app;
use catalog::products::dtos::{CreateProductReq, ProductDetailRes, UpdateProductReq};
use catalog::products::value_objects::{ProductId, ProductStatus};
use shared::auth::jwt::CurrentUser;
use shared::test_utils::http::{
    authed_delete, authed_get, authed_json_request, body_bytes, body_json, json_request,
};
use tower::ServiceExt;
use uuid::Uuid;

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

    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert!(items.is_empty()); // no active products yet
    assert!(body["next_cursor"].is_null());
    assert!(body["prev_cursor"].is_null());
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
    let other = seller_user();
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

    let body = body_json(resp).await;
    let products = body["items"].as_array().unwrap();
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

// ── Pagination integration test ─────────────────────────────
// Cursor math is unit-tested in shared::db::pagination_support::tests.
// This single test verifies the HTTP layer wires pagination correctly.

/// Helper: create N active products for a seller, returns (seller, token)
async fn create_n_active_products(pool: &shared::db::PgPool, n: usize) -> (CurrentUser, String) {
    let state = test_app_state(pool.clone());
    let user = seller();
    let token = test_token(&user);
    let service = &state.service;

    for i in 0..n {
        let mut req = sample_create_product_req();
        req.name = format!("Product {}", i);
        req.slug = Some(format!("product-{}", i));
        let product = service.create_product(&user, req).await.unwrap();
        let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());
        service
            .update_product(
                &user,
                product_id,
                UpdateProductReq {
                    name: None,
                    slug: None,
                    description: None,
                    base_price: None,
                    currency: None,
                    category_id: None,
                    brand_id: None,
                    status: Some(ProductStatus::Active),
                },
            )
            .await
            .unwrap();
    }
    (user, token)
}

#[tokio::test]
async fn pagination_limit_is_respected() {
    let db = test_db().await;
    create_n_active_products(&db.pool, 5).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/products?limit=2")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert!(body["next_cursor"].as_str().is_some());
    assert!(body["prev_cursor"].is_null());
}

#[tokio::test]
async fn pagination_includes_image_url() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = seller();
    let service = &state.service;

    // Create product and activate it
    let product = service
        .create_product(&user, sample_create_product_req())
        .await
        .unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());
    service
        .update_product(
            &user,
            product_id,
            UpdateProductReq {
                name: None,
                slug: None,
                description: None,
                base_price: None,
                currency: None,
                category_id: None,
                brand_id: None,
                status: Some(ProductStatus::Active),
            },
        )
        .await
        .unwrap();

    // Add an image
    service
        .add_image(&user, product_id, sample_add_image_req())
        .await
        .unwrap();

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

    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]["image_url"].as_str().unwrap(),
        "https://cdn.example.com/img/widget-1.jpg"
    );
}

// ── Filter tests ─────────────────────────────────────────────

/// Helper: create a product with specific attributes and activate it.
/// Returns the product id as string.
async fn create_active_product(
    service: &catalog::products::service::CatalogService,
    user: &CurrentUser,
    req: CreateProductReq,
) -> String {
    let product = service.create_product(user, req).await.unwrap();
    let product_id = ProductId::new(uuid::Uuid::parse_str(&product.id).unwrap());
    service
        .update_product(
            user,
            product_id,
            UpdateProductReq {
                name: None,
                slug: None,
                description: None,
                base_price: None,
                currency: None,
                category_id: None,
                brand_id: None,
                status: Some(ProductStatus::Active),
            },
        )
        .await
        .unwrap();
    product.id
}

#[tokio::test]
async fn filter_by_category() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = seller();
    let service = &state.service;

    let cat_a = create_test_category_named(&db.pool, "Phones").await;
    let cat_b = create_test_category_named(&db.pool, "Laptops").await;

    create_active_product(
        service,
        &user,
        CreateProductReq {
            name: "Phone One".into(),
            slug: Some("phone-one".into()),
            description: None,
            base_price: rust_decimal::Decimal::new(999, 2),
            currency: None,
            category_id: Some(cat_a.value()),
            brand_id: None,
        },
    )
    .await;

    create_active_product(
        service,
        &user,
        CreateProductReq {
            name: "Laptop One".into(),
            slug: Some("laptop-one".into()),
            description: None,
            base_price: rust_decimal::Decimal::new(1999, 2),
            currency: None,
            category_id: Some(cat_b.value()),
            brand_id: None,
        },
    )
    .await;

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/products?category_id={}", cat_a.value()))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"].as_str().unwrap(), "Phone One");
}

#[tokio::test]
async fn filter_by_brand() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = seller();
    let service = &state.service;

    let cat = create_test_category_named(&db.pool, "Electronics").await;
    let brand_a = create_test_brand_named(&db.pool, "BrandAlpha").await;
    let brand_b = create_test_brand_named(&db.pool, "BrandBeta").await;
    associate_brand_category(&db.pool, brand_a, cat).await;
    associate_brand_category(&db.pool, brand_b, cat).await;

    create_active_product(
        service,
        &user,
        CreateProductReq {
            name: "Alpha Widget".into(),
            slug: Some("alpha-widget".into()),
            description: None,
            base_price: rust_decimal::Decimal::new(500, 2),
            currency: None,
            category_id: Some(cat.value()),
            brand_id: Some(brand_a.value()),
        },
    )
    .await;

    create_active_product(
        service,
        &user,
        CreateProductReq {
            name: "Beta Widget".into(),
            slug: Some("beta-widget".into()),
            description: None,
            base_price: rust_decimal::Decimal::new(700, 2),
            currency: None,
            category_id: Some(cat.value()),
            brand_id: Some(brand_b.value()),
        },
    )
    .await;

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/products?brand_id={}", brand_b.value()))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"].as_str().unwrap(), "Beta Widget");
}

#[tokio::test]
async fn filter_by_price_range() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = seller();
    let service = &state.service;

    // Create products at different prices: 5.00, 15.00, 25.00
    for (i, price) in [(500i64, 2u32), (1500, 2), (2500, 2)].iter().enumerate() {
        create_active_product(
            service,
            &user,
            CreateProductReq {
                name: format!("Price Product {}", i),
                slug: Some(format!("price-product-{}", i)),
                description: None,
                base_price: rust_decimal::Decimal::new(price.0, price.1),
                currency: None,
                category_id: None,
                brand_id: None,
            },
        )
        .await;
    }

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/products?min_price=10&max_price=20")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"].as_str().unwrap(), "Price Product 1");
}

#[tokio::test]
async fn filter_by_search() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = seller();
    let service = &state.service;

    create_active_product(
        service,
        &user,
        CreateProductReq {
            name: "Blue Sneaker".into(),
            slug: Some("blue-sneaker".into()),
            description: None,
            base_price: rust_decimal::Decimal::new(5999, 2),
            currency: None,
            category_id: None,
            brand_id: None,
        },
    )
    .await;

    create_active_product(
        service,
        &user,
        CreateProductReq {
            name: "Red Jacket".into(),
            slug: Some("red-jacket".into()),
            description: None,
            base_price: rust_decimal::Decimal::new(8999, 2),
            currency: None,
            category_id: None,
            brand_id: None,
        },
    )
    .await;

    let router = app(state);
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/products?search=Sneaker")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"].as_str().unwrap(), "Blue Sneaker");
}

#[tokio::test]
async fn filter_by_status_seller_me() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let user = seller();
    let token = test_token(&user);
    let service = &state.service;

    // Create a draft product (default status)
    service
        .create_product(
            &user,
            CreateProductReq {
                name: "Draft Product".into(),
                slug: Some("draft-product".into()),
                description: None,
                base_price: rust_decimal::Decimal::new(1000, 2),
                currency: None,
                category_id: None,
                brand_id: None,
            },
        )
        .await
        .unwrap();

    // Create an active product
    create_active_product(
        service,
        &user,
        CreateProductReq {
            name: "Active Product".into(),
            slug: Some("active-product".into()),
            description: None,
            base_price: rust_decimal::Decimal::new(2000, 2),
            currency: None,
            category_id: None,
            brand_id: None,
        },
    )
    .await;

    // Without filter — should return both
    let router = app(state.clone());
    let resp = router
        .oneshot(authed_get("/api/v1/products/seller/me", &token))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 2);

    // Filter by status=draft — should return only the draft product
    let router = app(state.clone());
    let resp = router
        .oneshot(authed_get(
            "/api/v1/products/seller/me?status=draft",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"].as_str().unwrap(), "Draft Product");

    // Filter by status=active — should return only the active product
    let router = app(state);
    let resp = router
        .oneshot(authed_get(
            "/api/v1/products/seller/me?status=active",
            &token,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"].as_str().unwrap(), "Active Product");
}
