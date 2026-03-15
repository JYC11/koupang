use cart::AppState;
use cart::app;
use shared::server::ServiceBuilder;
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("cart")
        .http_port_env("CART_PORT")
        .with_redis()
        .run(|infra| {
            let app_state = AppState::new(infra.require_redis().clone());
            app(app_state)
        })
        .await
}
