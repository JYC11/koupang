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
        .with_redis()
        .with_consumers(|infra| {
            let handler = Arc::new(PaymentEventHandler::new(
                infra.require_db().clone(),
                Arc::new(payment::gateway::mock::MockPaymentGateway::always_succeeds()),
                infra.redis.clone(),
            ));
            vec![ConsumerRegistration {
                group_id: "payment-service".to_string(),
                topics: vec![
                    "catalog.events".to_string(),
                    "orders.events".to_string(),
                    "payments.events".to_string(), // self-consumption for capture retry
                ],
                handler,
            }]
        })
        .with_outbox_relay(None)
        .run(|infra| {
            let app_state = AppState::new(infra.require_db().clone());
            app(app_state)
        })
        .await
}
