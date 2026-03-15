use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;

pub mod consumers;
pub mod events;
pub mod orders;
pub mod outbox;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth_config: AuthConfig,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            auth_config: AuthConfig::new(),
        }
    }

    pub fn new_with_jwt(pool: PgPool, auth_config: AuthConfig) -> Self {
        Self { pool, auth_config }
    }
}

pub fn app(app_state: AppState) -> axum::Router {
    orders::routes::order_routes(app_state)
}
