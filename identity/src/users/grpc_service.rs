use crate::users::repository::get_user_by_id;
use shared::db::PgPool;
use shared::grpc::identity::identity_service_server::IdentityService;
use shared::grpc::identity::{GetUserRequest, GetUserResponse};
use tonic::{Request, Response, Status};
use uuid::Uuid;

pub struct IdentityGrpcService {
    pool: PgPool,
}

impl IdentityGrpcService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[tonic::async_trait]
impl IdentityService for IdentityGrpcService {
    // this could potentially be called A LOT, so there could be a lot more optimizing done if needed
    async fn get_user(
        &self,
        request: Request<GetUserRequest>,
    ) -> Result<Response<GetUserResponse>, Status> {
        let user_id = Uuid::parse_str(&request.into_inner().user_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid UUID: {}", e)))?;

        let user = get_user_by_id(&self.pool, user_id)
            .await
            .map_err(|e| match e {
                shared::errors::AppError::NotFound(msg) => Status::not_found(msg),
                other => Status::internal(other.to_string()),
            })?;

        Ok(Response::new(GetUserResponse {
            id: user.id.to_string(),
            username: user.username,
            email: user.email,
            role: user.role.to_string(),
            email_verified: user.email_verified,
        }))
    }
}
