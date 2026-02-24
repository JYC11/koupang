use identity::AppState;
use identity::app;
use identity::users::grpc_service::IdentityGrpcService;
use shared::grpc::identity::identity_service_server::IdentityServiceServer;
use shared::health::health_routes;
use shared::server::{GrpcConfig, ServiceConfig, run_service_with_infra};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    run_service_with_infra(
        ServiceConfig {
            name: "identity",
            port_env_key: "IDENTITY_PORT",
            db_url_env_key: "IDENTITY_DB_URL",
            migrations_dir: "./migrations",
        },
        Some((
            GrpcConfig {
                port_env_key: "IDENTITY_GRPC_PORT",
                default_port: 50051,
            },
            |pool, addr| async move {
                let svc = IdentityGrpcService::new(pool);
                tonic::transport::Server::builder()
                    .add_service(IdentityServiceServer::new(svc))
                    .serve(addr)
                    .await
            },
        )),
        |pool, redis_conn| {
            let app_state = AppState::new(pool, redis_conn);
            app(app_state).merge(health_routes("identity"))
        },
    )
    .await
}
