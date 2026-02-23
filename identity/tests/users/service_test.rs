use crate::common::{
    sample_create_req, sample_update_req, test_user_service, verify_user_email_directly,
};
use identity::users::dtos::{
    ChangePasswordReq, ForgotPasswordReq, ResetPasswordReq, UserLoginReq, UserRefreshReq,
};
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
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    verify_user_email_directly(&pool, &username).await;

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
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    verify_user_email_directly(&pool, &username).await;

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

#[sqlx::test(migrations = "./migrations")]
async fn login_unverified_user_fails(pool: PgPool) {
    let service = test_user_service(pool);
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let login_req = UserLoginReq { username, password };
    let result = service.login_user(login_req).await;
    assert!(result.is_err(), "Login should fail for unverified user");
}

#[sqlx::test(migrations = "./migrations")]
async fn verify_email_sets_email_verified(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    // Get the token from DB
    let row: (String,) = sqlx::query_as("SELECT token FROM email_verification_tokens LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    service
        .verify_email(identity::users::dtos::VerifyEmailReq { token: row.0 })
        .await
        .unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();
    assert!(user.email_verified);
}

#[sqlx::test(migrations = "./migrations")]
async fn create_user_generates_verification_token(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    service.create_user(req).await.unwrap();

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM email_verification_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "Should have created one verification token");
}

// ── Password Reset Tests ────────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn forgot_password_with_valid_email_creates_token(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let email = req.email.clone();
    service.create_user(req).await.unwrap();

    let result = service.forgot_password(ForgotPasswordReq { email }).await;
    assert!(result.is_ok());

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM password_reset_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1, "Should have created one password reset token");
}

#[sqlx::test(migrations = "./migrations")]
async fn forgot_password_with_invalid_email_does_not_fail(pool: PgPool) {
    let service = test_user_service(pool.clone());

    let result = service
        .forgot_password(ForgotPasswordReq {
            email: "nonexistent@example.com".to_string(),
        })
        .await;
    assert!(result.is_ok(), "Should not fail for nonexistent email");

    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM password_reset_tokens")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 0, "Should not have created any token");
}

#[sqlx::test(migrations = "./migrations")]
async fn reset_password_with_valid_token_succeeds(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let email = req.email.clone();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    // Trigger forgot password to create a token
    service
        .forgot_password(ForgotPasswordReq { email })
        .await
        .unwrap();

    // Get the token from DB
    let row: (String,) = sqlx::query_as("SELECT token FROM password_reset_tokens LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();

    // Reset the password
    let result = service
        .reset_password(ResetPasswordReq {
            token: row.0,
            new_password: "newpassword456".to_string(),
        })
        .await;
    assert!(result.is_ok());

    // Verify the new password works by logging in
    verify_user_email_directly(&pool, &username).await;
    let login_result = service
        .login_user(UserLoginReq {
            username,
            password: "newpassword456".to_string(),
        })
        .await;
    assert!(login_result.is_ok());
}

#[sqlx::test(migrations = "./migrations")]
async fn reset_password_with_invalid_token_fails(pool: PgPool) {
    let service = test_user_service(pool);

    let result = service
        .reset_password(ResetPasswordReq {
            token: "invalid-token-does-not-exist".to_string(),
            new_password: "newpassword456".to_string(),
        })
        .await;
    assert!(result.is_err());
}

// ── Password Change Tests ───────────────────────────────────

#[sqlx::test(migrations = "./migrations")]
async fn change_password_with_correct_current_succeeds(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let result = service
        .change_password(
            user.id,
            ChangePasswordReq {
                current_password: password,
                new_password: "newpassword456".to_string(),
            },
        )
        .await;
    assert!(result.is_ok());

    // Verify login works with new password
    verify_user_email_directly(&pool, &username).await;
    let login_result = service
        .login_user(UserLoginReq {
            username,
            password: "newpassword456".to_string(),
        })
        .await;
    assert!(login_result.is_ok());
}

#[sqlx::test(migrations = "./migrations")]
async fn change_password_with_wrong_current_fails(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    service.create_user(req).await.unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let result = service
        .change_password(
            user.id,
            ChangePasswordReq {
                current_password: "wrongpassword".to_string(),
                new_password: "newpassword456".to_string(),
            },
        )
        .await;
    assert!(result.is_err());
}

#[sqlx::test(migrations = "./migrations")]
async fn change_password_same_as_current_fails(pool: PgPool) {
    let service = test_user_service(pool.clone());
    let req = sample_create_req();
    let username = req.username.clone();
    let password = req.password.clone();
    service.create_user(req).await.unwrap();

    let user = identity::users::repository::get_user_by_username(&pool, &username)
        .await
        .unwrap();

    let result = service
        .change_password(
            user.id,
            ChangePasswordReq {
                current_password: password.clone(),
                new_password: password,
            },
        )
        .await;
    assert!(result.is_err(), "Should reject same password");
}
