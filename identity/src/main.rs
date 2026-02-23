use identity::AppState;
use identity::app;
use shared::health::health_routes;
use shared::server::{ServiceConfig, run_service};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run_service(
        ServiceConfig {
            name: "identity",
            db_url_env_key: "IDENTITY_DB_URL",
            migrations_dir: "./.migrations/identity",
        },
        |pool, common_app_state| {
            let app_state = AppState::new(pool, common_app_state);
            app(app_state).merge(health_routes("identity"))
        },
    )
    .await
}
