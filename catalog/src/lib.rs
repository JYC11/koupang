use products::service::CatalogService;
use shared::auth::jwt::JwtService;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use std::sync::Arc;

pub mod products;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<CatalogService>,
    pub jwt_service: Arc<JwtService>,
}

impl AppState {
    pub fn new(pool: PgPool, _redis_conn: Option<redis::aio::ConnectionManager>) -> Self {
        Self {
            service: Arc::new(CatalogService::new(pool)),
            jwt_service: Arc::new(JwtService::new(AuthConfig::new())),
        }
    }


    pub fn new_with_jwt(pool: PgPool, auth_config: AuthConfig) -> Self {
        Self {
            service: Arc::new(CatalogService::new(pool)),
            jwt_service: Arc::new(JwtService::new(auth_config)),
        }
    }
}

pub fn app(app_state: AppState) -> axum::Router {
    products::routes::product_routes(app_state)
}
