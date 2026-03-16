use payment::AppState;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use shared::test_utils::db::TestDb;

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

pub fn test_app_state(pool: PgPool) -> AppState {
    AppState::new_with_jwt(pool, test_auth_config())
}
