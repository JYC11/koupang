use crate::common::{
    admin_create_req, sample_create_req, sample_create_req_2, sample_update_req, test_app_state,
    verify_user_email_directly,
};
use axum::body::Body;
use axum::http::{Request, StatusCode};
use identity::app;
use identity::users::dtos::{UserCreateReq, UserLoginReq};
use identity::users::value_objects::Username;
use shared::auth::jwt::JwtTokens;
use shared::db::PgPool;
use shared::test_utils::http::{body_bytes, body_json};
use tower::ServiceExt;

fn register_request(req: &UserCreateReq) -> Request<Body> {
    Request::builder()
        .uri("/api/v1/users/register")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(req).unwrap()))
        .unwrap()
}

fn login_request(username: &str, password: &str) -> Request<Body> {
    let login_req = UserLoginReq {
        username: username.to_string(),
        password: password.to_string(),
    };
    Request::builder()
        .uri("/api/v1/users/login")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(&login_req).unwrap()))
        .unwrap()
}

/// Register a user and login, returning (access_token, refresh_token, user_id)
async fn register_and_login(pool: &PgPool, req: &UserCreateReq) -> (String, String, String) {
    let state = test_app_state(pool.clone());
    let router = app(state);

    // Register
    let resp = router.clone().oneshot(register_request(req)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Verify email directly in DB so login succeeds
    verify_user_email_directly(pool, &req.username).await;

    // Login
    let resp = router
        .clone()
        .oneshot(login_request(&req.username, &req.password))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let tokens: JwtTokens = serde_json::from_slice(&body_bytes(resp).await).unwrap();

    // Get user id via internal endpoint - find the user
    let user = identity::users::repository::get_user_by_username(
        pool,
        Username::new(&req.username).unwrap(),
    )
    .await
    .unwrap();

    (
        tokens.access_token,
        tokens.refresh_token,
        user.id.to_string(),
    )
}

// ── Register Tests ──────────────────────────────────────────

#[tokio::test]
async fn register_returns_201() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);
    let req = sample_create_req();

    let resp = router.oneshot(register_request(&req)).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn register_duplicate_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);
    let req = sample_create_req();

    let resp = router
        .clone()
        .oneshot(register_request(&req))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = router.oneshot(register_request(&req)).await.unwrap();
    assert!(
        resp.status().is_client_error() || resp.status().is_server_error(),
        "Expected error status for duplicate registration, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn register_missing_fields_returns_422() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/register")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"username":"test"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// ── Login Tests ─────────────────────────────────────────────

#[tokio::test]
async fn login_returns_tokens() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool.clone());
    let router = app(state);
    let req = sample_create_req();

    // Register first
    router
        .clone()
        .oneshot(register_request(&req))
        .await
        .unwrap();

    // Verify email
    verify_user_email_directly(&pool, &req.username).await;

    // Login
    let resp = router
        .oneshot(login_request(&req.username, &req.password))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let tokens: JwtTokens = serde_json::from_slice(&body_bytes(resp).await).unwrap();
    assert!(!tokens.access_token.is_empty());
    assert!(!tokens.refresh_token.is_empty());
}

#[tokio::test]
async fn login_wrong_credentials_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);
    let req = sample_create_req();

    // Register first
    router
        .clone()
        .oneshot(register_request(&req))
        .await
        .unwrap();

    // Login with wrong password
    let resp = router
        .oneshot(login_request(&req.username, "wrongpassword"))
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "Expected client error for wrong credentials, got {}",
        resp.status()
    );
}

// ── Refresh Tests ───────────────────────────────────────────

#[tokio::test]
async fn refresh_returns_new_access_token() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (_, refresh_token, _) = register_and_login(&pool, &sample_create_req()).await;
    let state = test_app_state(pool);
    let router = app(state);

    let refresh_req = serde_json::json!({ "refresh_token": refresh_token });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/refresh")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&refresh_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert!(body["access_token"].as_str().is_some());
    assert!(!body["access_token"].as_str().unwrap().is_empty());
}

// ── GET User Tests ──────────────────────────────────────────

#[tokio::test]
async fn get_user_without_auth_returns_401() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", uuid::Uuid::new_v4()))
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_own_user_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (access_token, _, user_id) = register_and_login(&pool, &sample_create_req()).await;
    let state = test_app_state(pool);
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", user_id))
                .method("GET")
                .header("authorization", format!("Bearer {}", access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = body_json(resp).await;
    assert_eq!(body["id"].as_str().unwrap(), user_id);
}

#[tokio::test]
async fn get_other_user_non_admin_returns_403() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (access_token, _, _) = register_and_login(&pool, &sample_create_req()).await;

    // Register another user
    let state = test_app_state(pool.clone());
    let router = app(state);
    let req2 = sample_create_req_2();
    router
        .clone()
        .oneshot(register_request(&req2))
        .await
        .unwrap();
    let other_user = identity::users::repository::get_user_by_username(
        &pool,
        Username::new(&req2.username).unwrap(),
    )
    .await
    .unwrap();

    let state2 = test_app_state(pool);
    let router2 = app(state2);
    let resp = router2
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", other_user.id))
                .method("GET")
                .header("authorization", format!("Bearer {}", access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn admin_can_get_any_user() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    // Register a regular user
    let state = test_app_state(pool.clone());
    let router = app(state);
    let user_req = sample_create_req();
    router
        .clone()
        .oneshot(register_request(&user_req))
        .await
        .unwrap();
    let regular_user = identity::users::repository::get_user_by_username(
        &pool,
        Username::new(&user_req.username).unwrap(),
    )
    .await
    .unwrap();

    // Register and login as admin
    let (admin_token, _, _) = register_and_login(&pool, &admin_create_req()).await;

    let state2 = test_app_state(pool);
    let router2 = app(state2);
    let resp = router2
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", regular_user.id))
                .method("GET")
                .header("authorization", format!("Bearer {}", admin_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

// ── PUT User Tests ──────────────────────────────────────────

#[tokio::test]
async fn update_own_user_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (access_token, _, user_id) = register_and_login(&pool, &sample_create_req()).await;
    let state = test_app_state(pool);
    let router = app(state);

    let update_req = sample_update_req();
    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", user_id))
                .method("PUT")
                .header("authorization", format!("Bearer {}", access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&update_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn update_other_user_non_admin_returns_403() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (access_token, _, _) = register_and_login(&pool, &sample_create_req()).await;

    // Register another user
    let state = test_app_state(pool.clone());
    let router = app(state);
    let req2 = sample_create_req_2();
    router
        .clone()
        .oneshot(register_request(&req2))
        .await
        .unwrap();
    let other_user = identity::users::repository::get_user_by_username(
        &pool,
        Username::new(&req2.username).unwrap(),
    )
    .await
    .unwrap();

    let state2 = test_app_state(pool);
    let router2 = app(state2);
    let update_req = sample_update_req();
    let resp = router2
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", other_user.id))
                .method("PUT")
                .header("authorization", format!("Bearer {}", access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&update_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

// ── DELETE User Tests ───────────────────────────────────────

#[tokio::test]
async fn delete_own_user_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (access_token, _, user_id) = register_and_login(&pool, &sample_create_req()).await;
    let state = test_app_state(pool);
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", user_id))
                .method("DELETE")
                .header("authorization", format!("Bearer {}", access_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn delete_without_auth_returns_401() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);

    let resp = router
        .oneshot(
            Request::builder()
                .uri(&format!("/api/v1/users/{}", uuid::Uuid::new_v4()))
                .method("DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── Email Verification Tests ────────────────────────────────

#[tokio::test]
async fn login_unverified_email_returns_403() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);
    let req = sample_create_req();

    // Register but do NOT verify email
    router
        .clone()
        .oneshot(register_request(&req))
        .await
        .unwrap();

    // Attempt login
    let resp = router
        .oneshot(login_request(&req.username, &req.password))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn verify_email_with_valid_token_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool.clone());
    let router = app(state);
    let req = sample_create_req();

    // Register
    router
        .clone()
        .oneshot(register_request(&req))
        .await
        .unwrap();

    // Get the token from the DB
    let row: (String,) = sqlx::query_as("SELECT token FROM email_verification_tokens LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    let verify_req = serde_json::json!({ "token": row.0 });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/verify-email")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&verify_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn verify_email_with_invalid_token_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);

    let verify_req = serde_json::json!({ "token": "invalid-token-that-does-not-exist" });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/verify-email")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&verify_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "Expected client error for invalid token, got {}",
        resp.status()
    );
}

// ── Password Reset Tests ────────────────────────────────────

#[tokio::test]
async fn forgot_password_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool.clone());
    let router = app(state);
    let req = sample_create_req();

    // Register a user first
    router
        .clone()
        .oneshot(register_request(&req))
        .await
        .unwrap();

    let forgot_req = serde_json::json!({ "email": req.email });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/forgot-password")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&forgot_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn forgot_password_nonexistent_email_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);

    let forgot_req = serde_json::json!({ "email": "nonexistent@example.com" });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/forgot-password")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&forgot_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "Should return 200 even for nonexistent email (no leak)"
    );
}

#[tokio::test]
async fn reset_password_with_valid_token_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool.clone());
    let router = app(state);
    let req = sample_create_req();

    // Register
    router
        .clone()
        .oneshot(register_request(&req))
        .await
        .unwrap();

    // Forgot password
    let forgot_req = serde_json::json!({ "email": req.email });
    router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/forgot-password")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&forgot_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // Get the reset token from DB
    let row: (String,) = sqlx::query_as("SELECT token FROM password_reset_tokens LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Reset password
    let reset_req = serde_json::json!({ "token": row.0, "new_password": "NewPass1!" });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/reset-password")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&reset_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn reset_password_with_invalid_token_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);

    let reset_req = serde_json::json!({ "token": "bad-token", "new_password": "NewPass1!" });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/reset-password")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&reset_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "Expected client error for invalid reset token, got {}",
        resp.status()
    );
}

// ── Password Change Tests ───────────────────────────────────

#[tokio::test]
async fn change_password_returns_200() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let (access_token, _, _) = register_and_login(&pool, &req).await;
    let state = test_app_state(pool);
    let router = app(state);

    let change_req = serde_json::json!({
        "current_password": req.password,
        "new_password": "NewPassword4!"
    });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/change-password")
                .method("POST")
                .header("authorization", format!("Bearer {}", access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&change_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn change_password_wrong_current_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let (access_token, _, _) = register_and_login(&pool, &req).await;
    let state = test_app_state(pool);
    let router = app(state);

    let change_req = serde_json::json!({
        "current_password": "wrongpassword",
        "new_password": "NewPassword4!"
    });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/change-password")
                .method("POST")
                .header("authorization", format!("Bearer {}", access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&change_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "Expected client error for wrong current password, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn change_password_same_password_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let req = sample_create_req();
    let (access_token, _, _) = register_and_login(&pool, &req).await;
    let state = test_app_state(pool);
    let router = app(state);

    let change_req = serde_json::json!({
        "current_password": req.password,
        "new_password": req.password
    });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/change-password")
                .method("POST")
                .header("authorization", format!("Bearer {}", access_token))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&change_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "Expected client error for same password, got {}",
        resp.status()
    );
}

#[tokio::test]
async fn change_password_without_auth_returns_401() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool);
    let router = app(state);

    let change_req = serde_json::json!({
        "current_password": "password123",
        "new_password": "NewPassword4!"
    });
    let resp = router
        .oneshot(
            Request::builder()
                .uri("/api/v1/users/change-password")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&change_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
