use identity::AppState;
use identity::app;
use shared::config::db_config::DbConfig;
use shared::db::init_db;
use std::error::Error;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "identity=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    let db_config = DbConfig::new("IDENTITY_DB_URL");
    let pool = init_db(db_config, "./.migrations/identity").await;
    let app_state = AppState::new(pool);
    let port = app_state.common_app_state.port;
    let app = app(app_state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Identity service listening on port {}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
