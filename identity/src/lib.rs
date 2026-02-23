use shared::CommonAppState;
use shared::db::PgPool;
use std::sync::Arc;
use users::routes::user_routes;
use users::service::UserService;

pub mod users;

#[derive(Clone)]
pub struct AppState {
    pub common_app_state: CommonAppState,
    pub service: Arc<UserService>,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            common_app_state: CommonAppState::new(),
            service: Arc::new(UserService::new(pool)),
        }
    }

    pub fn new_with_service(service: UserService) -> Self {
        Self {
            common_app_state: CommonAppState::new(),
            service: Arc::new(service),
        }
    }
}

pub fn app(app_state: AppState) -> axum::Router {
    user_routes(app_state)
}
