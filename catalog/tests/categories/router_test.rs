use crate::common::{
    admin, create_test_category_named, create_test_child_category, seller, test_app_state, test_db,
    test_token,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use catalog::app;
use catalog::categories::dtos::{CategoryRes, CreateCategoryReq};
use shared::test_utils::http::{authed_delete, authed_json_request, body_bytes, body_json};
use tower::ServiceExt;
use uuid::Uuid;

/// Helper: create a category via router, returns CategoryRes
async fn create_category_via_router(pool: &shared::db::PgPool) -> (CategoryRes, String) {
    let state = test_app_state(pool.clone());
    let router = app(state);
    let user = admin();
    let token = test_token(&user);

    let req = CreateCategoryReq {
        name: "Electronics".to_string(),
        slug: None,
        parent_id: None,
        description: Some("Electronic devices".to_string()),
    };

    let resp = router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/categories",
            &token,
            &req,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    let category: CategoryRes = serde_json::from_value(body["data"].clone()).unwrap();
    (category, token)
}

// ── Public endpoints ────────────────────────────────────────

#[tokio::test]
async fn list_root_categories_returns_200() {
    let db = test_db().await;
    create_test_category_named(&db.pool, "Electronics").await;
    create_test_category_named(&db.pool, "Books").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/categories")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let categories: Vec<CategoryRes> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(categories.len(), 2);
}

#[tokio::test]
async fn get_category_returns_200() {
    let db = test_db().await;
    let cat_id = create_test_category_named(&db.pool, "Electronics").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/categories/{}", cat_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let category: CategoryRes = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(category.name, "Electronics");
}

#[tokio::test]
async fn get_category_by_slug_returns_200() {
    let db = test_db().await;
    create_test_category_named(&db.pool, "Home Garden").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/categories/slug/home-garden")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let category: CategoryRes = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(category.name, "Home Garden");
}

#[tokio::test]
async fn get_nonexistent_category_returns_404() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/categories/{}", Uuid::new_v4()))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn get_children_returns_200() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    create_test_child_category(&db.pool, root_id, "Phones").await;
    create_test_child_category(&db.pool, root_id, "Laptops").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/categories/{}/children", root_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let children: Vec<CategoryRes> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(children.len(), 2);
}

#[tokio::test]
async fn get_subtree_returns_200() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    let phones_id = create_test_child_category(&db.pool, root_id, "Phones").await;
    create_test_child_category(&db.pool, phones_id, "Android").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/categories/{}/subtree", root_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let subtree: Vec<CategoryRes> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(subtree.len(), 3);
}

#[tokio::test]
async fn get_ancestors_returns_200() {
    let db = test_db().await;
    let root_id = create_test_category_named(&db.pool, "Electronics").await;
    let phones_id = create_test_child_category(&db.pool, root_id, "Phones").await;
    let android_id = create_test_child_category(&db.pool, phones_id, "Android").await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/categories/{}/ancestors", android_id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let ancestors: Vec<CategoryRes> = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert_eq!(ancestors.len(), 3);
    assert_eq!(ancestors[0].name, "Electronics");
    assert_eq!(ancestors[2].name, "Android");
}

// ── Protected endpoints ─────────────────────────────────────

#[tokio::test]
async fn create_category_without_auth_returns_401() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);

    let req = CreateCategoryReq {
        name: "Hacked".to_string(),
        slug: None,
        parent_id: None,
        description: None,
    };

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/categories")
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
async fn create_category_as_seller_returns_403() {
    let db = test_db().await;
    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let user = seller();
    let token = test_token(&user);

    let req = CreateCategoryReq {
        name: "Forbidden".to_string(),
        slug: None,
        parent_id: None,
        description: None,
    };

    let resp = router
        .oneshot(authed_json_request(
            "POST",
            "/api/v1/categories",
            &token,
            &req,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn create_category_as_admin_returns_200() {
    let db = test_db().await;
    let (category, _) = create_category_via_router(&db.pool).await;
    assert_eq!(category.name, "Electronics");
    assert!(!category.id.is_empty());
}

#[tokio::test]
async fn delete_category_via_router() {
    let db = test_db().await;
    let (category, token) = create_category_via_router(&db.pool).await;

    let state = test_app_state(db.pool.clone());
    let router = app(state);
    let resp = router
        .oneshot(authed_delete(
            &format!("/api/v1/categories/{}", category.id),
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
                .uri(&format!("/api/v1/categories/{}", category.id))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
