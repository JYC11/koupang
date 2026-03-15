use payment::AppState;
use payment::app;
use shared::server::ServiceBuilder;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("payment")
        .http_port_env("PAYMENT_PORT")
        .with_db("PAYMENT_DB_URL")
        .run(|infra| {
            let app_state = AppState::new(infra.require_db().clone());
            app(app_state)
        })
        .await
}
