use crate::users::routes::routes;
use crate::users::service::UserService;
use shared::CommonAppState;
use shared::config::db_config::DbConfig;
use shared::db::{PgPool, init_db};
use std::error::Error;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod users;

#[derive(Clone)]
pub struct AppState {
    pub common_app_state: CommonAppState,
    pub service: Arc<UserService>,
}

impl AppState {
    fn new(pool: PgPool) -> Self {
        Self {
            common_app_state: CommonAppState::new(),
            service: Arc::new(UserService::new(pool)),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "identity=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    let db_config = DbConfig::new();
    let pool = init_db(db_config, "./migrations/identity").await;
    let app_state = AppState::new(pool);
    let port = app_state.common_app_state.port;
    let app = routes(app_state);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("Identity service listening on port {}", port);
    axum::serve(listener, app).await?;
    Ok(())
}
