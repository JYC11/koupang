use brands::service::BrandService;
use categories::service::CategoryService;
use products::service::CatalogService;
use shared::auth::jwt::JwtService;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use std::sync::Arc;

pub mod brands;
pub mod categories;
pub mod common;
pub mod products;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<CatalogService>,
    pub category_service: Arc<CategoryService>,
    pub brand_service: Arc<BrandService>,
    pub jwt_service: Arc<JwtService>,
}

impl AppState {
    pub fn new(pool: PgPool, redis_conn: Option<redis::aio::ConnectionManager>) -> Self {
        Self {
            service: Arc::new(CatalogService::new(pool.clone(), redis_conn)),
            category_service: Arc::new(CategoryService::new(pool.clone())),
            brand_service: Arc::new(BrandService::new(pool)),
            jwt_service: Arc::new(JwtService::new(AuthConfig::new())),
        }
    }

    pub fn new_with_jwt(pool: PgPool, auth_config: AuthConfig) -> Self {
        Self {
            service: Arc::new(CatalogService::new(pool.clone(), None)),
            category_service: Arc::new(CategoryService::new(pool.clone())),
            brand_service: Arc::new(BrandService::new(pool)),
            jwt_service: Arc::new(JwtService::new(auth_config)),
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
