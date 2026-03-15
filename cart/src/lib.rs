use shared::config::auth_config::AuthConfig;
use shared::errors::AppError;

pub mod cart;

#[derive(Clone)]
pub struct AppState {
    pub redis: redis::aio::ConnectionManager,
    pub auth_config: AuthConfig,
}

impl AppState {
    pub fn new(redis: redis::aio::ConnectionManager) -> Self {
        Self {
            redis,
            auth_config: AuthConfig::new(),
        }
    }

    pub fn new_with_jwt(redis: redis::aio::ConnectionManager, auth_config: AuthConfig) -> Self {
        Self { redis, auth_config }
    }

    pub fn redis_conn(&self) -> Result<redis::aio::ConnectionManager, AppError> {
        Ok(self.redis.clone())
    }
}

pub fn app(app_state: AppState) -> axum::Router {
    cart::routes::cart_routes(app_state)
}
