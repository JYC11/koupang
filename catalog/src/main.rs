use catalog::consumers::handler::CatalogEventHandler;
use catalog::{AppState, app};
use shared::server::{ConsumerRegistration, ServiceBuilder};
use std::error::Error;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("catalog")
        .http_port_env("CATALOG_PORT")
        .with_db("CATALOG_DB_URL")
        .with_redis()
        .with_consumers(|infra| {
            let handler = Arc::new(CatalogEventHandler::new(infra.require_db().clone()));
            vec![ConsumerRegistration {
                group_id: "catalog-service".to_string(),
                topics: vec!["orders.events".to_string()],
                handler,
            }]
        })
        .with_outbox_relay()
        .run(|infra| {
            let app_state = AppState::new(infra.require_db().clone(), infra.redis.clone());
            app(app_state)
        })
        .await
}
