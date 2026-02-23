use crate::CommonAppState;
use crate::config::db_config::DbConfig;
use crate::db::{PgPool, init_db};
use crate::observability::init_tracing;
use axum::Router;
use std::error::Error;

pub struct ServiceConfig {
    pub name: &'static str,
    pub db_url_env_key: &'static str,
    pub migrations_dir: &'static str,
}

pub async fn run_service<F>(config: ServiceConfig, build_app: F) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(PgPool, CommonAppState) -> Router,
{
    init_tracing(config.name);
    let db_config = DbConfig::new(config.db_url_env_key);
    let pool = init_db(db_config, config.migrations_dir).await;
    let common_app_state = CommonAppState::new();
    let port = common_app_state.port;
    let app = build_app(pool, common_app_state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("{} service listening on port {}", config.name, port);
    axum::serve(listener, app).await?;
    Ok(())
}
