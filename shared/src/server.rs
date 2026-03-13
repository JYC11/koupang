use crate::cache::init_optional_redis;
use crate::config::db_config::DbConfig;
use crate::db::{PgPool, init_db};
use crate::health::health_routes;
use crate::observability::init_tracing;
use axum::Router;
use std::error::Error;
use std::future::Future;
use std::net::SocketAddr;

// ---------------------------------------------------------------------------
// Infra — initialized resources passed to service build closures
// ---------------------------------------------------------------------------

/// Infrastructure resources initialized by [`ServiceBuilder`].
///
/// All fields are cheap to clone (Arc-backed).
#[derive(Clone)]
pub struct Infra {
    pub db: PgPool,
    pub redis: Option<redis::aio::ConnectionManager>,
}

// ---------------------------------------------------------------------------
// GrpcConfig (shared between old and new API)
// ---------------------------------------------------------------------------

pub struct GrpcConfig {
    pub port_env_key: &'static str,
    pub default_port: u16,
}

// ---------------------------------------------------------------------------
// ServiceBuilder — composable bootstrap
// ---------------------------------------------------------------------------

/// Composable builder for service bootstrap.
///
/// Initializes tracing, database, optional Redis, and serves HTTP
/// (optionally with a gRPC sidecar). Health routes (`GET /health`)
/// are merged automatically.
///
/// # Example (HTTP only)
/// ```ignore
/// ServiceBuilder::new("catalog")
///     .http_port_env("CATALOG_PORT")
///     .db_url_env("CATALOG_DB_URL")
///     .with_redis()
///     .run(|infra| {
///         let state = AppState::new(infra.db.clone(), infra.redis.clone());
///         app(state)
///     })
///     .await
/// ```
pub struct ServiceBuilder {
    name: &'static str,
    http_port_env: &'static str,
    db_url_env: &'static str,
    migrations_dir: &'static str,
    redis: bool,
}

impl ServiceBuilder {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            http_port_env: "PORT",
            db_url_env: "DATABASE_URL",
            migrations_dir: "./migrations",
            redis: false,
        }
    }

    pub fn http_port_env(mut self, key: &'static str) -> Self {
        self.http_port_env = key;
        self
    }

    pub fn db_url_env(mut self, key: &'static str) -> Self {
        self.db_url_env = key;
        self
    }

    pub fn migrations_dir(mut self, dir: &'static str) -> Self {
        self.migrations_dir = dir;
        self
    }

    pub fn with_redis(mut self) -> Self {
        self.redis = true;
        self
    }

    /// Run HTTP server only (no gRPC sidecar).
    pub async fn run<F>(self, build_app: F) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce(&Infra) -> Router,
    {
        let infra = self.init_infra().await;
        let port = self.parse_http_port();
        let app = build_app(&infra).merge(health_routes(self.name));

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
        tracing::info!("{} listening on port {port}", self.name);
        axum::serve(listener, app).await?;
        Ok(())
    }

    /// Run HTTP + gRPC servers concurrently.
    ///
    /// `build_app` borrows [`Infra`] (clone what you need).
    /// `build_grpc` takes ownership of [`Infra`] so the returned future is `Send`.
    pub async fn run_with_grpc<F, G, Fut>(
        self,
        grpc_config: GrpcConfig,
        build_app: F,
        build_grpc: G,
    ) -> Result<(), Box<dyn Error>>
    where
        F: FnOnce(&Infra) -> Router,
        G: FnOnce(Infra, SocketAddr) -> Fut,
        Fut: Future<Output = Result<(), tonic::transport::Error>> + Send,
    {
        let infra = self.init_infra().await;
        let port = self.parse_http_port();
        let app = build_app(&infra).merge(health_routes(self.name));

        let grpc_port: u16 = std::env::var(grpc_config.port_env_key)
            .unwrap_or_else(|_| grpc_config.default_port.to_string())
            .parse()
            .expect("gRPC port must be a valid u16");
        let grpc_addr: SocketAddr = format!("0.0.0.0:{grpc_port}").parse()?;

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
        tracing::info!("{} HTTP listening on port {port}", self.name);
        tracing::info!("{} gRPC listening on port {grpc_port}", self.name);

        tokio::select! {
            result = axum::serve(listener, app) => { result?; }
            result = build_grpc(infra, grpc_addr) => { result?; }
        }
        Ok(())
    }

    async fn init_infra(&self) -> Infra {
        init_tracing(self.name);
        let db_config = DbConfig::new(self.db_url_env);
        let pool = init_db(db_config, self.migrations_dir).await;
        let redis = if self.redis {
            init_optional_redis().await
        } else {
            None
        };
        Infra { db: pool, redis }
    }

    fn parse_http_port(&self) -> u16 {
        std::env::var(self.http_port_env)
            .unwrap_or_else(|_| "3000".to_string())
            .parse()
            .expect("HTTP port must be a valid u16")
    }
}
