use crate::common::{
    sample_create_req, sample_update_req, test_app_state, test_app_state_with_redis,
};
use identity::users::dtos::{ChangePasswordReq, UserRefreshReq};
use identity::users::service;
use identity::users::value_objects::{UserId, Username};
use redis::AsyncCommands;
use shared::auth::middleware::GetCurrentUser;
use shared::test_utils::redis::TestRedis;

// ── Service-Specific Logic Tests ────────────────────────────

#[tokio::test]
async fn create_user_hashes_password() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service::create_user_account(&state, req).await.unwrap();

    // Fetch the raw entity to check the stored password
    let user =
        identity::users::repository::get_user_by_username(&pool, Username::new(&username).unwrap())
            .await
            .unwrap();
    assert!(
        user.password.starts_with("$argon2"),
        "Password should be hashed with argon2, got: {}",
        user.password
    );
}

#[tokio::test]
async fn refresh_with_invalid_token_fails() {
    let db = crate::common::test_db().await;
    let state = test_app_state(db.pool.clone());
    let result = service::generate_refresh_token(
        &state,
        UserRefreshReq {
            refresh_token: "garbage.invalid.token".to_string(),
        },
    )
    .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_current_user_returns_correct_user() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let state = test_app_state(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let role = req.role.clone();
    service::create_user_account(&state, req).await.unwrap();

    let entity =
        identity::users::repository::get_user_by_username(&pool, Username::new(&username).unwrap())
            .await
            .unwrap();

    let current_user = state.get_by_id(entity.id).await.unwrap();
    assert_eq!(current_user.id, entity.id);
    assert_eq!(current_user.role, role);
}

// ── Redis Cache Behavior Tests ──────────────────────────────

#[tokio::test]
async fn get_by_id_caches_user_in_redis() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let test_redis = TestRedis::start().await;
    let mut assert_conn = test_redis.conn.clone();
    let email_service = std::sync::Arc::new(shared::email::MockEmailService::new());
    let state = identity::AppState::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    service::create_user_account(&state, req).await.unwrap();

    let entity =
        identity::users::repository::get_user_by_username(&pool, Username::new(&username).unwrap())
            .await
            .unwrap();

    // Call get_by_id which should cache the user
    let current_user = state.get_by_id(entity.id).await.unwrap();

    // Verify the key exists in Redis
    let key = format!("user:{}", entity.id);
    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_some(), "user:{{id}} key should exist in Redis");

    // Verify deserialized data matches
    let cached_user: shared::auth::jwt::CurrentUser =
        serde_json::from_str(&cached.unwrap()).unwrap();
    assert_eq!(cached_user.id, current_user.id);
    assert_eq!(cached_user.role, current_user.role);
}

#[tokio::test]
async fn get_by_id_cache_hit_returns_same_data() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let (state, _test_redis) = test_app_state_with_redis(pool.clone()).await;

    let req = sample_create_req();
    let username = req.username.clone();
    service::create_user_account(&state, req).await.unwrap();

    let entity =
        identity::users::repository::get_user_by_username(&pool, Username::new(&username).unwrap())
            .await
            .unwrap();

    // First call — populates cache
    let first = state.get_by_id(entity.id).await.unwrap();
    // Second call — should hit cache
    let second = state.get_by_id(entity.id).await.unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(first.role, second.role);
}

#[tokio::test]
async fn update_user_evicts_cache() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let test_redis = TestRedis::start().await;
    let mut assert_conn = test_redis.conn.clone();
    let email_service = std::sync::Arc::new(shared::email::MockEmailService::new());
    let state = identity::AppState::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    service::create_user_account(&state, req).await.unwrap();

    let entity =
        identity::users::repository::get_user_by_username(&pool, Username::new(&username).unwrap())
            .await
            .unwrap();

    // Populate cache
    state.get_by_id(entity.id).await.unwrap();
    let key = format!("user:{}", entity.id);
    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_some(), "Cache should be populated before update");

    // Update user — should evict cache
    service::update_user_account(&state, UserId::new(entity.id), sample_update_req())
        .await
        .unwrap();

    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_none(), "Cache should be evicted after update");
}

#[tokio::test]
async fn delete_user_evicts_cache() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let test_redis = TestRedis::start().await;
    let mut assert_conn = test_redis.conn.clone();
    let email_service = std::sync::Arc::new(shared::email::MockEmailService::new());
    let state = identity::AppState::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    service::create_user_account(&state, req).await.unwrap();

    let entity =
        identity::users::repository::get_user_by_username(&pool, Username::new(&username).unwrap())
            .await
            .unwrap();

    // Populate cache
    state.get_by_id(entity.id).await.unwrap();
    let key = format!("user:{}", entity.id);
    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_some(), "Cache should be populated before delete");

    // Delete user — should evict cache
    service::delete_user_account(&state, UserId::new(entity.id))
        .await
        .unwrap();

    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_none(), "Cache should be evicted after delete");

    // get_by_id should also fail
    let result = state.get_by_id(entity.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn change_password_evicts_cache() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let test_redis = TestRedis::start().await;
    let mut assert_conn = test_redis.conn.clone();
    let email_service = std::sync::Arc::new(shared::email::MockEmailService::new());
    let state = identity::AppState::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service::create_user_account(&state, req).await.unwrap();

    let entity =
        identity::users::repository::get_user_by_username(&pool, Username::new(&username).unwrap())
            .await
            .unwrap();

    // Populate cache
    state.get_by_id(entity.id).await.unwrap();
    let key = format!("user:{}", entity.id);
    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(
        cached.is_some(),
        "Cache should be populated before password change"
    );

    // Change password — should evict cache
    service::change_password(
        &state,
        UserId::new(entity.id),
        ChangePasswordReq {
            current_password: password,
            new_password: "NewPassword4!".to_string(),
        },
    )
    .await
    .unwrap();

    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(
        cached.is_none(),
        "Cache should be evicted after password change"
    );
}
