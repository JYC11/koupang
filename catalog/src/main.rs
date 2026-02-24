use catalog::AppState;
use catalog::app;
use shared::health::health_routes;
use shared::server::{NoGrpc, ServiceConfig, run_service_with_infra};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run_service_with_infra(
        ServiceConfig {
            name: "catalog",
            port_env_key: "CATALOG_PORT",
            db_url_env_key: "CATALOG_DB_URL",
            migrations_dir: "./.migrations/catalog",
        },
        None::<NoGrpc>,
        |pool, redis_conn| {
            let app_state = AppState::new(pool, redis_conn);
            app(app_state).merge(health_routes("catalog"))
        },
    )
    .await
}
