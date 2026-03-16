use order::consumers::handler::OrderEventHandler;
use order::{AppState, app};
use shared::server::{ConsumerRegistration, ServiceBuilder};
use std::error::Error;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("order")
        .http_port_env("ORDER_PORT")
        .with_db("ORDER_DB_URL")
        .with_consumers(|_infra| {
            vec![ConsumerRegistration {
                group_id: "order-service".to_string(),
                topics: vec!["catalog.events".to_string(), "payments.events".to_string()],
                handler: Arc::new(OrderEventHandler::new()),
            }]
        })
        .run(|infra| {
            let app_state = AppState::new(infra.require_db().clone());
            app(app_state)
        })
        .await
}
