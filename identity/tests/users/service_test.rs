use crate::common::{
    sample_create_req, sample_update_req, test_user_service, test_user_service_with_redis,
    verify_user_email_directly,
};
use identity::users::dtos::{
    ChangePasswordReq, ForgotPasswordReq, ResetPasswordReq, UserLoginReq, UserRefreshReq,
};
use redis::AsyncCommands;
use shared::auth::middleware::GetCurrentUser;
use shared::test_utils::redis::TestRedis;
use uuid::Uuid;

#[tokio::test]
async fn create_user_succeeds() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool);
    let req = sample_create_req();
    let result = service.create_user(req).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn create_user_hashes_password() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    // Fetch the raw entity to check the stored password
    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();
    assert!(
        user.password.starts_with("$argon2"),
        "Password should be hashed with argon2, got: {}",
        user.password
    );
}

#[tokio::test]
async fn get_user_returns_user_res() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let email = req.email.clone();
    service.create_user(req).await.unwrap();

    // Get the user id from the repository
    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let user_res = service.get_user(entity.id).await.unwrap();
    assert_eq!(user_res.username, username);
    assert_eq!(user_res.email, email);
    // UserRes should not contain password field (it's not in the struct)
    assert_eq!(user_res.id, entity.id.to_string());
}

#[tokio::test]
async fn get_nonexistent_user_returns_error() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool);
    let result = service.get_user(Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_user_changes_fields() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let update_req = sample_update_req();
    let new_username = update_req.username.clone();
    service.update_user(entity.id, update_req).await.unwrap();

    let updated = service.get_user(entity.id).await.unwrap();
    assert_eq!(updated.username, new_username);
}

#[tokio::test]
async fn delete_user_makes_unfetchable() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    service.delete_user(entity.id).await.unwrap();

    let result = service.get_user(entity.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn login_correct_credentials_returns_tokens() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    verify_user_email_directly(&pool, &username).await;

    let login_req = UserLoginReq { username, password };
    let result = service.login_user(login_req).await.unwrap();

    match result {
        identity::users::dtos::UserLoginRes::Success(tokens) => {
            assert!(!tokens.access_token.is_empty());
            assert!(!tokens.refresh_token.is_empty());
        }
        identity::users::dtos::UserLoginRes::Failure(_) => {
            panic!("Expected successful login");
        }
    }
}

#[tokio::test]
async fn login_wrong_password_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool);
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let login_req = UserLoginReq {
        username,
        password: "wrongpassword".to_string(),
    };
    let result = service.login_user(login_req).await;
    assert!(result.is_err(), "Login with wrong password should fail");
}

#[tokio::test]
async fn login_nonexistent_username_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool);
    let login_req = UserLoginReq {
        username: "nonexistent".to_string(),
        password: "password123".to_string(),
    };
    let result = service.login_user(login_req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn refresh_token_returns_new_access_token() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    verify_user_email_directly(&pool, &username).await;

    let login_res = service
        .login_user(UserLoginReq { username, password })
        .await
        .unwrap();

    let refresh_token = match login_res {
        identity::users::dtos::UserLoginRes::Success(tokens) => tokens.refresh_token,
        _ => panic!("Expected successful login"),
    };

    let refresh_res = service
        .generate_refresh_token(UserRefreshReq { refresh_token })
        .await
        .unwrap();

    assert!(!refresh_res.access_token.is_empty());
}

#[tokio::test]
async fn refresh_with_invalid_token_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool);
    let result = service
        .generate_refresh_token(UserRefreshReq {
            refresh_token: "garbage.invalid.token".to_string(),
        })
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn get_current_user_returns_correct_user() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let role = req.role.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let current_user = service.get_by_id(entity.id).await.unwrap();
    assert_eq!(current_user.id, entity.id);
    assert_eq!(current_user.role, role);
}

#[tokio::test]
async fn login_unverified_user_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool);
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let login_req = UserLoginReq { username, password };
    let result = service.login_user(login_req).await;
    assert!(result.is_err(), "Login should fail for unverified user");
}

#[tokio::test]
async fn verify_email_sets_email_verified() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    // Get the token from DB
    let row: (String,) = sqlx::query_as("SELECT token FROM email_verification_tokens LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    service
        .verify_email(identity::users::dtos::VerifyEmailReq { token: row.0 })
        .await
        .unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();
    assert!(user.email_verified);
}

#[tokio::test]
async fn create_user_generates_verification_token() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    service.create_user(req).await.unwrap();

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM email_verification_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "Should have created one verification token");
}

// ── Password Reset Tests ────────────────────────────────────

#[tokio::test]
async fn forgot_password_with_valid_email_creates_token() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let email = req.email.clone();
    service.create_user(req).await.unwrap();

    let result = service.forgot_password(ForgotPasswordReq { email }).await;
    assert!(result.is_ok());

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM password_reset_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "Should have created one password reset token");
}

#[tokio::test]
async fn forgot_password_with_invalid_email_does_not_fail() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());

    let result = service
        .forgot_password(ForgotPasswordReq {
            email: "nonexistent@example.com".to_string(),
        })
        .await;
    assert!(result.is_ok(), "Should not fail for nonexistent email");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM password_reset_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "Should not have created any token");
}

#[tokio::test]
async fn reset_password_with_valid_token_succeeds() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let email = req.email.clone();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    // Trigger forgot password to create a token
    service
        .forgot_password(ForgotPasswordReq { email })
        .await
        .unwrap();

    // Get the token from DB
    let row: (String,) = sqlx::query_as("SELECT token FROM password_reset_tokens LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Reset the password
    let result = service
        .reset_password(ResetPasswordReq {
            token: row.0,
            new_password: "NewPassword4!".to_string(),
        })
        .await;
    assert!(result.is_ok());

    // Verify the new password works by logging in
    verify_user_email_directly(&pool, &username).await;
    let login_result = service
        .login_user(UserLoginReq {
            username,
            password: "NewPassword4!".to_string(),
        })
        .await;
    assert!(login_result.is_ok());
}

#[tokio::test]
async fn reset_password_with_invalid_token_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool);

    let result = service
        .reset_password(ResetPasswordReq {
            token: "invalid-token-does-not-exist".to_string(),
            new_password: "NewPassword4!".to_string(),
        })
        .await;
    assert!(result.is_err());
}

// ── Password Change Tests ───────────────────────────────────

#[tokio::test]
async fn change_password_with_correct_current_succeeds() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let result = service
        .change_password(
            user.id,
            ChangePasswordReq {
                current_password: password,
                new_password: "NewPassword4!".to_string(),
            },
        )
        .await;
    assert!(result.is_ok());

    // Verify login works with new password
    verify_user_email_directly(&pool, &username).await;
    let login_result = service
        .login_user(UserLoginReq {
            username,
            password: "NewPassword4!".to_string(),
        })
        .await;
    assert!(login_result.is_ok());
}

#[tokio::test]
async fn change_password_with_wrong_current_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let result = service
        .change_password(
            user.id,
            ChangePasswordReq {
                current_password: "wrongpassword".to_string(),
                new_password: "NewPassword4!".to_string(),
            },
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn change_password_same_as_current_fails() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let result = service
        .change_password(
            user.id,
            ChangePasswordReq {
                current_password: password.clone(),
                new_password: password,
            },
        )
        .await;
    assert!(result.is_err(), "Should reject same password");
}

// ── Redis Cache Behavior Tests ──────────────────────────────

#[tokio::test]
async fn get_by_id_caches_user_in_redis() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let test_redis = TestRedis::start().await;
    let mut assert_conn = test_redis.conn.clone();
    let email_service = std::sync::Arc::new(shared::email::MockEmailService::new());
    let service = identity::users::service::UserService::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    // Call get_by_id which should cache the user
    let current_user = service.get_by_id(entity.id).await.unwrap();

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
    let (service, _test_redis) = test_user_service_with_redis(pool.clone()).await;

    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    // First call — populates cache
    let first = service.get_by_id(entity.id).await.unwrap();
    // Second call — should hit cache
    let second = service.get_by_id(entity.id).await.unwrap();

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
    let service = identity::users::service::UserService::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    // Populate cache
    service.get_by_id(entity.id).await.unwrap();
    let key = format!("user:{}", entity.id);
    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_some(), "Cache should be populated before update");

    // Update user — should evict cache
    service
        .update_user(entity.id, sample_update_req())
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
    let service = identity::users::service::UserService::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    // Populate cache
    service.get_by_id(entity.id).await.unwrap();
    let key = format!("user:{}", entity.id);
    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_some(), "Cache should be populated before delete");

    // Delete user — should evict cache
    service.delete_user(entity.id).await.unwrap();

    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(cached.is_none(), "Cache should be evicted after delete");

    // get_by_id should also fail
    let result = service.get_by_id(entity.id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn change_password_evicts_cache() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();
    let test_redis = TestRedis::start().await;
    let mut assert_conn = test_redis.conn.clone();
    let email_service = std::sync::Arc::new(shared::email::MockEmailService::new());
    let service = identity::users::service::UserService::new_with_config(
        pool.clone(),
        crate::common::test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );

    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    // Populate cache
    service.get_by_id(entity.id).await.unwrap();
    let key = format!("user:{}", entity.id);
    let cached: Option<String> = assert_conn.get(&key).await.unwrap();
    assert!(
        cached.is_some(),
        "Cache should be populated before password change"
    );

    // Change password — should evict cache
    service
        .change_password(
            entity.id,
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
