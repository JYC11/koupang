use identity::AppState;
use identity::app;
use identity::users::grpc_service::IdentityGrpcService;
use shared::grpc::identity::identity_service_server::IdentityServiceServer;
use shared::server::{GrpcConfig, ServiceBuilder};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    ServiceBuilder::new("identity")
        .http_port_env("IDENTITY_PORT")
        .db_url_env("IDENTITY_DB_URL")
        .with_redis()
        .run_with_grpc(
            GrpcConfig {
                port_env_key: "IDENTITY_GRPC_PORT",
                default_port: 50051,
            },
            |infra| {
                let app_state = AppState::new(infra.db.clone(), infra.redis.clone());
                app(app_state)
            },
            |infra, addr| async move {
                let svc = IdentityGrpcService::new(infra.db);
                tonic::transport::Server::builder()
                    .add_service(IdentityServiceServer::new(svc))
                    .serve(addr)
                    .await
            },
        )
        .await
}
