use identity::AppState;
use identity::users::dtos::{UserCreateReq, UserUpdateReq};
use identity::users::service::UserService;
use shared::auth::Role;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use shared::email::MockEmailService;
use shared::test_utils::db::TestDb;
use shared::test_utils::redis::TestRedis;
use std::sync::Arc;

pub async fn test_db() -> TestDb {
    TestDb::start("./migrations").await
}

pub fn test_auth_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: b"test-access-secret-key-for-testing".to_vec(),
        refresh_token_secret: b"test-refresh-secret-key-for-testing".to_vec(),
        access_token_expiry_secs: 3600,
        refresh_token_expiry_secs: 7200,
    }
}

pub fn test_user_service(pool: PgPool) -> UserService {
    let email_service = Arc::new(MockEmailService::new());
    UserService::new_with_config(pool, test_auth_config(), email_service, None)
}

pub async fn verify_user_email_directly(pool: &PgPool, username: &str) {
    sqlx::query("UPDATE users SET email_verified = TRUE WHERE username = $1")
        .bind(username)
        .execute(pool)
        .await
        .expect("Failed to verify user email directly");
}

pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_service(test_user_service(pool))
}

pub fn sample_create_req() -> UserCreateReq {
    UserCreateReq {
        username: "testuser".to_string(),
        password: "Password1!".to_string(),
        email: "test@example.com".to_string(),
        phone: "+82-10-1234-5678".to_string(),
        role: Role::Buyer,
    }
}

pub fn sample_create_req_2() -> UserCreateReq {
    UserCreateReq {
        username: "testuser2".to_string(),
        password: "Password2!".to_string(),
        email: "test2@example.com".to_string(),
        phone: "+82-10-9876-5432".to_string(),
        role: Role::Buyer,
    }
}

pub fn admin_create_req() -> UserCreateReq {
    UserCreateReq {
        username: "adminuser".to_string(),
        password: "AdminPass1!".to_string(),
        email: "admin@example.com".to_string(),
        phone: "+82-10-0000-0000".to_string(),
        role: Role::Admin,
    }
}

pub fn sample_update_req() -> UserUpdateReq {
    UserUpdateReq {
        username: "updateduser".to_string(),
        email: "updated@example.com".to_string(),
        phone: "+82-10-1111-2222".to_string(),
        role: Role::Buyer,
    }
}

pub async fn test_user_service_with_redis(pool: PgPool) -> (UserService, TestRedis) {
    let test_redis = TestRedis::start().await;
    let email_service = Arc::new(MockEmailService::new());
    let service = UserService::new_with_config(
        pool,
        test_auth_config(),
        email_service,
        Some(test_redis.conn.clone()),
    );
    (service, test_redis)
}
