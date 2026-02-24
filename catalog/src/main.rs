use catalog::app;
use catalog::AppState;
use shared::health::health_routes;
use shared::server::{run_service_with_infra, NoGrpc, ServiceConfig};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run_service_with_infra(
        ServiceConfig {
            name: "catalog",
            port_env_key: "CATALOG_PORT",
            db_url_env_key: "CATALOG_DB_URL",
            migrations_dir: "./migrations",
        },
        None::<NoGrpc>,
        |pool, redis_conn| {
            let app_state = AppState::new(pool, redis_conn);
            app(app_state).merge(health_routes("catalog"))
        },
    )
        .await
}
