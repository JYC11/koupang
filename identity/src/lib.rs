use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::GetCurrentUser;
use shared::cache::RedisCache;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use shared::email::{EmailService, MockEmailService};
use shared::errors::AppError;
use std::sync::Arc;
use users::routes::user_routes;
use users::value_objects::UserId;

pub mod users;

const USER_CACHE_TTL_SECS: u64 = 300; // 5 minutes
const USER_CACHE_PREFIX: &str = "user:";

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub cache: RedisCache,
    pub auth_config: AuthConfig,
    pub email_service: Arc<dyn EmailService>,
}

impl AppState {
    pub fn new(pool: PgPool, redis_conn: Option<redis::aio::ConnectionManager>) -> Self {
        Self {
            pool,
            cache: RedisCache::new(redis_conn, USER_CACHE_TTL_SECS),
            auth_config: AuthConfig::new(),
            email_service: Arc::new(MockEmailService::new()),
        }
    }

    pub fn new_with_config(
        pool: PgPool,
        auth_config: AuthConfig,
        email_service: Arc<dyn EmailService>,
        redis_conn: Option<redis::aio::ConnectionManager>,
    ) -> Self {
        Self {
            pool,
            cache: RedisCache::new(redis_conn, USER_CACHE_TTL_SECS),
            auth_config,
            email_service,
        }
    }
}

fn user_cache_key(id: UserId) -> String {
    format!("{USER_CACHE_PREFIX}{id}")
}

#[async_trait::async_trait]
impl GetCurrentUser for AppState {
    async fn get_by_id(&self, id: uuid::Uuid) -> Result<CurrentUser, AppError> {
        let user_id = UserId::new(id);
        let cache_key = user_cache_key(user_id);

        if let Some(cached) = self.cache.get::<CurrentUser>(&cache_key).await {
            return Ok(cached);
        }

        let user = users::repository::get_user_by_id(&self.pool, user_id).await?;
        let current_user = CurrentUser {
            id: user.id,
            role: user.role,
        };
        self.cache.set(&cache_key, &current_user).await;

        Ok(current_user)
    }
}

pub fn app(app_state: AppState) -> axum::Router {
    user_routes(app_state)
}
