use payment::consumers::handler::PaymentEventHandler;
use payment::{AppState, app};
use shared::server::{ConsumerRegistration, ServiceBuilder};
use std::error::Error;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("payment")
        .http_port_env("PAYMENT_PORT")
        .with_db("PAYMENT_DB_URL")
        .with_consumers(|_infra| {
            let handler = Arc::new(PaymentEventHandler::with_mock_gateway());
            vec![ConsumerRegistration {
                group_id: "payment-service".to_string(),
                topics: vec!["catalog.events".to_string(), "orders.events".to_string()],
                handler,
            }]
        })
        .run(|infra| {
            let app_state = AppState::new(infra.require_db().clone());
            app(app_state)
        })
        .await
}
