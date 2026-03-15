use order::AppState;
use order::app;
use shared::server::ServiceBuilder;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("order")
        .http_port_env("ORDER_PORT")
        .with_db("ORDER_DB_URL")
        .run(|infra| {
            let app_state = AppState::new(infra.require_db().clone());
            app(app_state)
        })
        .await
}
