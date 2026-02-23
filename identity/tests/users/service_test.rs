use crate::common::{sample_create_req, sample_update_req, test_user_service};
use identity::users::dtos::{UserLoginReq, UserRefreshReq};
use shared::auth::middleware::GetCurrentUser;
use shared::db::PgPool;
use uuid::Uuid;

#[sqlx::test(migrations = "./migrations")]
async fn create_user_succeeds(pool: PgPool) {
    let service = test_user_service(pool);
    let req = sample_create_req();
    let result = service.create_user(req).await;
    assert!(result.is_ok());
}

#[sqlx::test(migrations = "./migrations")]
async fn create_user_hashes_password(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    // Fetch the raw entity to check the stored password
    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();
    assert!(
        user.password.starts_with("$argon2"),
        "Password should be hashed with argon2, got: {}",
        user.password
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn get_user_returns_user_res(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let email = req.email.clone();
    service.create_user(req).await.unwrap();

    // Get the user id from the repository
    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let user_res = service.get_user(entity.id).await.unwrap();
    assert_eq!(user_res.username, username);
    assert_eq!(user_res.email, email);
    // UserRes should not contain password field (it's not in the struct)
    assert_eq!(user_res.id, entity.id.to_string());
}

#[sqlx::test(migrations = "./migrations")]
async fn get_nonexistent_user_returns_error(pool: PgPool) {
    let service = test_user_service(pool);
    let result = service.get_user(Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn update_user_changes_fields(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let update_req = sample_update_req();
    let new_username = update_req.username.clone();
    service.update_user(entity.id, update_req).await.unwrap();

    let updated = service.get_user(entity.id).await.unwrap();
    assert_eq!(updated.username, new_username);
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_user_makes_unfetchable(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    service.delete_user(entity.id).await.unwrap();

    let result = service.get_user(entity.id).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn login_correct_credentials_returns_tokens(pool: PgPool) {
    let service = test_user_service(pool);
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let login_req = UserLoginReq { username, password };
    let result = service.login_user(login_req).await.unwrap();

    match result {
        identity::users::dtos::UserLoginRes::Success(tokens) => {
            assert!(!tokens.access_token.is_empty());
            assert!(!tokens.refresh_token.is_empty());
        }
        identity::users::dtos::UserLoginRes::Failure(_) => {
            panic!("Expected successful login");
        }
    }
}

#[sqlx::test(migrations = "./migrations")]
async fn login_wrong_password_fails(pool: PgPool) {
    let service = test_user_service(pool);
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let login_req = UserLoginReq {
        username,
        password: "wrongpassword".to_string(),
    };
    let result = service.login_user(login_req).await;
    assert!(result.is_err(), "Login with wrong password should fail");
}

#[sqlx::test(migrations = "./migrations")]
async fn login_nonexistent_username_fails(pool: PgPool) {
    let service = test_user_service(pool);
    let login_req = UserLoginReq {
        username: "nonexistent".to_string(),
        password: "password123".to_string(),
    };
    let result = service.login_user(login_req).await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_token_returns_new_access_token(pool: PgPool) {
    let service = test_user_service(pool);
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let login_res = service
        .login_user(UserLoginReq { username, password })
        .await
        .unwrap();

    let refresh_token = match login_res {
        identity::users::dtos::UserLoginRes::Success(tokens) => tokens.refresh_token,
        _ => panic!("Expected successful login"),
    };

    let refresh_res = service
        .generate_refresh_token(UserRefreshReq { refresh_token })
        .await
        .unwrap();

    assert!(!refresh_res.access_token.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_with_invalid_token_fails(pool: PgPool) {
    let service = test_user_service(pool);
    let result = service
        .generate_refresh_token(UserRefreshReq {
            refresh_token: "garbage.invalid.token".to_string(),
        })
        .await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn get_current_user_returns_correct_user(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let role = req.role.clone();
    service.create_user(req).await.unwrap();

    let entity = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let current_user = service.get_by_id(entity.id).await.unwrap();
    assert_eq!(current_user.id, entity.id);
    assert_eq!(current_user.role, role);
}
