use crate::users::service::UserService;
use shared::CommonAppState;
use shared::config::db_config::DbConfig;
use shared::db::{PgPool, init_db};
use std::error::Error;

mod users;

struct AppState {
    common_app_state: CommonAppState,
    service: UserService,
}

impl AppState {
    fn new(pool: PgPool) -> Self {
        Self {
            common_app_state: CommonAppState::new(),
            service: UserService::new(pool),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let db_config = DbConfig::new();
    // TODO configure migrations
    let pool = init_db(db_config, "./migrations").await;
    let app_state = AppState::new(pool);

    Ok(())
}
