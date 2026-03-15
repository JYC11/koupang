use catalog::AppState;
use catalog::app;
use shared::server::ServiceBuilder;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("catalog")
        .http_port_env("CATALOG_PORT")
        .with_db("CATALOG_DB_URL")
        .with_redis()
        .run(|infra| {
            let app_state = AppState::new(infra.require_db().clone(), infra.redis.clone());
            app(app_state)
        })
        .await
}
