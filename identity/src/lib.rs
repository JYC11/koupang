use shared::db::PgPool;
use shared::email::MockEmailService;
use std::sync::Arc;
use users::routes::user_routes;
use users::service::UserService;

pub mod users;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<UserService>,
}

impl AppState {
    pub fn new(pool: PgPool, redis_conn: Option<redis::aio::ConnectionManager>) -> Self {
        let email_service = Arc::new(MockEmailService::new());
        Self {
            service: Arc::new(UserService::new(pool, email_service, redis_conn)),
        }
    }

    pub fn new_with_service(service: UserService) -> Self {
        Self {
            service: Arc::new(service),
        }
    }
}

pub fn app(app_state: AppState) -> axum::Router {
    user_routes(app_state)
}
