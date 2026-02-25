use crate::common::sample_create_req;
use identity::users::dtos::ValidUserCreateReq;
use identity::users::grpc_service::IdentityGrpcService;
use identity::users::repository::create_user;
use shared::db::PgPool;
use shared::grpc::identity::GetUserRequest;
use shared::grpc::identity::identity_service_client::IdentityServiceClient;
use shared::grpc::identity::identity_service_server::IdentityServiceServer;
use shared::test_utils::grpc::start_test_grpc_server;
use tonic::Code;
use uuid::Uuid;

async fn start_grpc_server(pool: PgPool) -> String {
    let svc = IdentityGrpcService::new(pool);
    let router = tonic::transport::Server::builder().add_service(IdentityServiceServer::new(svc));
    start_test_grpc_server(router).await
}

#[tokio::test]
async fn get_user_returns_correct_response() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();

    let req = sample_create_req();
    let username = req.username.clone();
    let email = req.email.clone();
    let role = req.role;
    let password = req.password.clone();
    let validated: ValidUserCreateReq = req.try_into().unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    let url = start_grpc_server(pool).await;
    let mut client = IdentityServiceClient::connect(url).await.unwrap();

    let response = client
        .get_user(GetUserRequest {
            user_id: user_id.to_string(),
        })
        .await
        .unwrap()
        .into_inner();

    assert_eq!(response.id, user_id.to_string());
    assert_eq!(response.username, username);
    assert_eq!(response.email, email);
    assert_eq!(response.role, role.to_string());
    assert!(!response.email_verified);
}

#[tokio::test]
async fn get_user_verified_email_flag() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();

    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    let validated: ValidUserCreateReq = req.try_into().unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    crate::common::verify_user_email_directly(&pool, &username).await;

    let url = start_grpc_server(pool).await;
    let mut client = IdentityServiceClient::connect(url).await.unwrap();

    let response = client
        .get_user(GetUserRequest {
            user_id: user_id.to_string(),
        })
        .await
        .unwrap()
        .into_inner();

    assert!(response.email_verified);
}

#[tokio::test]
async fn get_user_nonexistent_returns_not_found() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();

    let url = start_grpc_server(pool).await;
    let mut client = IdentityServiceClient::connect(url).await.unwrap();

    let status = client
        .get_user(GetUserRequest {
            user_id: Uuid::now_v7().to_string(),
        })
        .await
        .unwrap_err();

    assert_eq!(status.code(), Code::NotFound);
}

#[tokio::test]
async fn get_user_invalid_uuid_returns_invalid_argument() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();

    let url = start_grpc_server(pool).await;
    let mut client = IdentityServiceClient::connect(url).await.unwrap();

    let status = client
        .get_user(GetUserRequest {
            user_id: "not-a-uuid".to_string(),
        })
        .await
        .unwrap_err();

    assert_eq!(status.code(), Code::InvalidArgument);
    assert!(status.message().contains("Invalid UUID"));
}

#[tokio::test]
async fn get_user_empty_id_returns_invalid_argument() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();

    let url = start_grpc_server(pool).await;
    let mut client = IdentityServiceClient::connect(url).await.unwrap();

    let status = client
        .get_user(GetUserRequest {
            user_id: String::new(),
        })
        .await
        .unwrap_err();

    assert_eq!(status.code(), Code::InvalidArgument);
}

#[tokio::test]
async fn get_user_deleted_returns_not_found() {
    let db = crate::common::test_db().await;
    let pool = db.pool.clone();

    let req = sample_create_req();
    let password = req.password.clone();
    let validated: ValidUserCreateReq = req.try_into().unwrap();

    let mut conn = pool.acquire().await.unwrap();
    let user_id = create_user(&mut *conn, validated, &password).await.unwrap();

    // Soft-delete the user
    sqlx::query("UPDATE users SET deleted_at = NOW() WHERE id = $1")
        .bind(user_id.value())
        .execute(&pool)
        .await
        .unwrap();

    let url = start_grpc_server(pool).await;
    let mut client = IdentityServiceClient::connect(url).await.unwrap();

    let status = client
        .get_user(GetUserRequest {
            user_id: user_id.to_string(),
        })
        .await
        .unwrap_err();

    assert_eq!(status.code(), Code::NotFound);
}
