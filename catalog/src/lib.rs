use shared::cache::RedisCache;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;

pub mod brands;
pub mod categories;
pub mod common;
pub mod consumers;
pub mod inventory;
pub mod products;

const PRODUCT_CACHE_TTL: u64 = 300; // 5 minutes

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cache: RedisCache,
    pub auth_config: AuthConfig,
}

impl AppState {
    pub fn new(pool: PgPool, redis_conn: Option<redis::aio::ConnectionManager>) -> Self {
        Self {
            pool,
            cache: RedisCache::new(redis_conn, PRODUCT_CACHE_TTL),
            auth_config: AuthConfig::new(),
        }
    }

    pub fn new_with_jwt(pool: PgPool, auth_config: AuthConfig) -> Self {
        Self {
            pool,
            cache: RedisCache::new(None, PRODUCT_CACHE_TTL),
            auth_config,
        }
    }
}

pub fn app(app_state: AppState) -> axum::Router {
    let category_routes = categories::routes::category_routes(app_state.clone());
    let brand_routes = brands::routes::brand_routes(app_state.clone());

    products::routes::product_routes(app_state)
        .merge(category_routes)
        .merge(brand_routes)
}
