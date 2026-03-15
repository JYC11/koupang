use crate::auth::Role;
use crate::auth::jwt::{self, CurrentUser};
use crate::config::auth_config::AuthConfig;
use uuid::Uuid;

pub fn test_auth_config() -> AuthConfig {
    AuthConfig {
        access_token_secret: b"test-access-secret-key-for-testing".to_vec(),
        refresh_token_secret: b"test-refresh-secret-key-for-testing".to_vec(),
        access_token_expiry_secs: 3600,
        refresh_token_expiry_secs: 7200,
    }
}

pub fn test_token(user: &CurrentUser) -> String {
    let config = test_auth_config();
    jwt::generate_access_token(&config, &user.id, "testuser", user.role.clone()).unwrap()
}

pub fn seller_user() -> CurrentUser {
    CurrentUser {
        id: Uuid::new_v4(),
        role: Role::Seller,
    }
}

pub fn buyer_user() -> CurrentUser {
    CurrentUser {
        id: Uuid::new_v4(),
        role: Role::Buyer,
    }
}

pub fn admin_user() -> CurrentUser {
    CurrentUser {
        id: Uuid::new_v4(),
        role: Role::Admin,
    }
}
