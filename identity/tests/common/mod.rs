use identity::AppState;
use identity::users::dtos::{UserCreateReq, UserUpdateReq};
use identity::users::service::UserService;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;

pub fn test_auth_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: b"test-access-secret-key-for-testing".to_vec(),
        refresh_token_secret: b"test-refresh-secret-key-for-testing".to_vec(),
        access_token_expiry_secs: 3600,
        refresh_token_expiry_secs: 7200,
    }
}

pub fn test_user_service(pool: PgPool) -> UserService {
    UserService::new_with_config(pool, test_auth_config())
}

pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_service(test_user_service(pool))
}

pub fn sample_create_req() -> UserCreateReq {
    UserCreateReq {
        username: "testuser".to_string(),
        password: "password123".to_string(),
        email: "test@example.com".to_string(),
        phone: "010-1234-5678".to_string(),
        role: "USER".to_string(),
    }
}

pub fn sample_create_req_2() -> UserCreateReq {
    UserCreateReq {
        username: "testuser2".to_string(),
        password: "password456".to_string(),
        email: "test2@example.com".to_string(),
        phone: "010-9876-5432".to_string(),
        role: "USER".to_string(),
    }
}

pub fn admin_create_req() -> UserCreateReq {
    UserCreateReq {
        username: "adminuser".to_string(),
        password: "adminpass123".to_string(),
        email: "admin@example.com".to_string(),
        phone: "010-0000-0000".to_string(),
        role: "ADMIN".to_string(),
    }
}

pub fn sample_update_req() -> UserUpdateReq {
    UserUpdateReq {
        username: "updateduser".to_string(),
        email: "updated@example.com".to_string(),
        phone: "010-1111-2222".to_string(),
        role: "USER".to_string(),
    }
}
