use crate::cache::init_optional_redis;
use crate::config::db_config::DbConfig;
use crate::db::{PgPool, init_db};
use crate::observability::init_tracing;
use axum::Router;
use std::error::Error;
use std::future::Future;
use std::net::SocketAddr;

pub struct ServiceConfig {
    pub name: &'static str,
    pub port_env_key: &'static str,
    pub db_url_env_key: &'static str,
    pub migrations_dir: &'static str,
}

pub struct GrpcConfig {
    pub port_env_key: &'static str,
    pub default_port: u16,
}

/// Convenience type for the gRPC tuple in services that don't run a gRPC sidecar.
/// Usage: `None::<NoGrpc>`
pub type NoGrpc = (
    GrpcConfig,
    fn(PgPool, SocketAddr) -> std::future::Ready<Result<(), tonic::transport::Error>>,
);

pub async fn run_service_with_infra<F, G, Fut>(
    config: ServiceConfig,
    grpc: Option<(GrpcConfig, G)>,
    build_app: F,
) -> Result<(), Box<dyn Error>>
where
    F: FnOnce(PgPool, Option<redis::aio::ConnectionManager>) -> Router,
    G: FnOnce(PgPool, SocketAddr) -> Fut,
    Fut: Future<Output = Result<(), tonic::transport::Error>> + Send,
{
    init_tracing(config.name);

    let db_config = DbConfig::new(config.db_url_env_key);
    let port: u16 = std::env::var(config.port_env_key)
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("PORT must be a valid u16");
    let pool = init_db(db_config, config.migrations_dir).await;
    let redis_conn = init_optional_redis().await;

    let app = build_app(pool.clone(), redis_conn);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("{} HTTP service listening on port {}", config.name, port);

    match grpc {
        Some((grpc_config, grpc_builder)) => {
            let grpc_port: u16 = std::env::var(grpc_config.port_env_key)
                .unwrap_or_else(|_| grpc_config.default_port.to_string())
                .parse()
                .expect("gRPC port must be a valid u16");
            let grpc_addr: SocketAddr = format!("0.0.0.0:{}", grpc_port).parse()?;
            tracing::info!(
                "{} gRPC service listening on port {}",
                config.name,
                grpc_port
            );

            tokio::select! {
                result = axum::serve(listener, app) => { result?; }
                result = grpc_builder(pool, grpc_addr) => { result?; }
            }
        }
        None => {
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}
