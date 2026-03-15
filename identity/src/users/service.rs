use crate::AppState;
use crate::user_cache_key;
use crate::users::dtos::{
    ChangePasswordReq, ForgotPasswordReq, ResetPasswordReq, UserCreateReq, UserLoginReq,
    UserLoginRes, UserRefreshReq, UserRefreshRes, UserRes, UserUpdateReq, ValidUserCreateReq,
    ValidUserUpdateReq, VerifyEmailReq,
};
use crate::users::repository::{
    create_password_reset_token, create_user, create_verification_token, delete_user,
    get_user_by_email, get_user_by_id, get_user_by_username, get_valid_password_reset_token,
    get_valid_verification_token, mark_reset_token_used, mark_token_used, update_user,
    update_user_password, verify_user_email,
};
use crate::users::value_objects::{
    Email, EmailTokenId, HashedPassword, Password, PasswordTokenId, UserId, Username,
};
use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::{Duration, Utc};
use shared::auth::jwt::{self, JwtTokens};
use shared::db::transaction_support::{TxError, with_transaction};
use shared::email::EmailMessage;
use shared::errors::AppError;

pub async fn create_user_account(state: &AppState, req: UserCreateReq) -> Result<(), AppError> {
    let validated: ValidUserCreateReq = req.try_into()?;
    let hashed_password = hash_password(validated.password.as_str())?;
    let email = validated.email.as_str().to_string();

    let token = generate_verification_token();
    let token_clone = token.clone();
    let expires_at = Utc::now() + Duration::hours(24);

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            let user_id = create_user(tx.as_executor(), validated, &hashed_password)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            create_verification_token(tx.as_executor(), user_id, &token_clone, expires_at)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create user: {}", e)))?;

    let email_message = EmailMessage {
        to: email,
        subject: "Verify your email address".to_string(),
        body_html: format!(
            "<h1>Email Verification</h1><p>Your verification token is: <strong>{}</strong></p>",
            token
        ),
    };
    if let Err(e) = state.email_service.send_email(email_message).await {
        tracing::error!(error = %e, "Failed to send verification email");
    }

    Ok(())
}

pub async fn verify_email(state: &AppState, req: VerifyEmailReq) -> Result<(), AppError> {
    let token_entity = get_valid_verification_token(&state.pool, &req.token).await?;

    with_transaction(&state.pool, |tx| {
        let token_id = EmailTokenId::new(token_entity.id);
        let user_id = UserId::new(token_entity.user_id);
        Box::pin(async move {
            verify_user_email(tx.as_executor(), user_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            mark_token_used(tx.as_executor(), token_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to verify email: {}", e)))?;

    Ok(())
}

pub async fn get_user(state: &AppState, id: UserId) -> Result<UserRes, AppError> {
    let user = get_user_by_id(&state.pool, id).await?;
    Ok(UserRes::new(user))
}

pub async fn update_user_account(
    state: &AppState,
    id: UserId,
    req: UserUpdateReq,
) -> Result<(), AppError> {
    let validated: ValidUserUpdateReq = req.try_into()?;

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            update_user(tx.as_executor(), id, validated)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to update user: {}", e)))?;

    state.cache.evict(&user_cache_key(id)).await;

    Ok(())
}

pub async fn delete_user_account(state: &AppState, id: UserId) -> Result<(), AppError> {
    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            delete_user(tx.as_executor(), id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to delete user: {}", e)))?;

    state.cache.evict(&user_cache_key(id)).await;

    Ok(())
}

pub async fn login_user(state: &AppState, req: UserLoginReq) -> Result<UserLoginRes, AppError> {
    let user = get_user_by_username(&state.pool, Username::new(&req.username)?).await?;

    if !user.email_verified {
        return Err(AppError::Forbidden(
            "Please verify your email before logging in".to_string(),
        ));
    }

    verify_password(&req.password, &HashedPassword::new(user.password.clone()))?;

    let access_token =
        jwt::generate_access_token(&state.auth_config, &user.id, &user.username, user.role)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;

    let refresh_token = jwt::generate_refresh_token(&state.auth_config, &user.id)
        .map_err(|e| AppError::Unauthorized(e.to_string()))?;

    Ok(UserLoginRes::Success(JwtTokens {
        access_token,
        refresh_token,
    }))
}

pub async fn generate_refresh_token(
    state: &AppState,
    req: UserRefreshReq,
) -> Result<UserRefreshRes, AppError> {
    let claims = jwt::validate_refresh_token(&state.auth_config, &req.refresh_token)
        .map_err(|e| AppError::Unauthorized(e.to_string()))?;

    let user = get_user_by_id(&state.pool, UserId::new(claims.sub)).await?;

    let access_token =
        jwt::generate_access_token(&state.auth_config, &user.id, &user.username, user.role)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;

    Ok(UserRefreshRes { access_token })
}

pub async fn forgot_password(state: &AppState, req: ForgotPasswordReq) -> Result<(), AppError> {
    // Look up user by email — if not found, return Ok silently (don't leak email existence)
    let user = match get_user_by_email(&state.pool, Email::new(&req.email)?).await {
        Ok(user) => user,
        Err(_) => return Ok(()),
    };

    let token = generate_verification_token();
    let token_clone = token.clone();
    let user_id = UserId::new(user.id);
    let expires_at = Utc::now() + Duration::hours(24);

    with_transaction(&state.pool, |tx| {
        Box::pin(async move {
            create_password_reset_token(tx.as_executor(), user_id, &token_clone, expires_at)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| {
        AppError::InternalServerError(format!("Failed to create password reset token: {}", e))
    })?;

    let email_message = EmailMessage {
        to: req.email,
        subject: "Reset your password".to_string(),
        body_html: format!(
            "<h1>Password Reset</h1><p>Your password reset token is: <strong>{}</strong></p>",
            token
        ),
    };
    if let Err(e) = state.email_service.send_email(email_message).await {
        tracing::error!(error = %e, "Failed to send password reset email");
    }

    Ok(())
}

pub async fn reset_password(state: &AppState, req: ResetPasswordReq) -> Result<(), AppError> {
    let token_entity = get_valid_password_reset_token(&state.pool, &req.token).await?;
    let validated_password = Password::new(&req.new_password)?;
    let hashed_password = hash_password(validated_password.as_str())?;

    let token_id = PasswordTokenId::new(token_entity.id);
    let user_id = UserId::new(token_entity.user_id);

    with_transaction(&state.pool, |tx| {
        let hashed = hashed_password.clone();
        Box::pin(async move {
            update_user_password(tx.as_executor(), user_id, &hashed)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            mark_reset_token_used(tx.as_executor(), token_id)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;

            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to reset password: {}", e)))?;

    Ok(())
}

pub async fn change_password(
    state: &AppState,
    user_id: UserId,
    req: ChangePasswordReq,
) -> Result<(), AppError> {
    let user = get_user_by_id(&state.pool, user_id).await?;

    verify_password(
        &req.current_password,
        &HashedPassword::new(user.password.clone()),
    )?;

    if req.current_password == req.new_password {
        return Err(AppError::BadRequest(
            "New password must be different from the current password".to_string(),
        ));
    }

    let validated_password = Password::new(&req.new_password)?;
    let hashed_password = hash_password(validated_password.as_str())?;

    with_transaction(&state.pool, |tx| {
        let hashed = hashed_password.clone();
        Box::pin(async move {
            update_user_password(tx.as_executor(), user_id, &hashed)
                .await
                .map_err(|e| TxError::Other(e.to_string()))?;
            Ok(())
        })
    })
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to change password: {}", e)))?;

    state.cache.evict(&user_cache_key(user_id)).await;

    Ok(())
}

fn generate_verification_token() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Hash a plaintext password using Argon2
fn hash_password(password: &str) -> Result<HashedPassword, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| HashedPassword::new(hash.to_string()))
        .map_err(|e| AppError::InternalServerError(format!("Failed to hash password: {}", e)))
}

/// Verify a plaintext password against a stored hash
fn verify_password(password: &str, hash: &HashedPassword) -> Result<(), AppError> {
    let parsed_hash = PasswordHash::new(hash.as_str())
        .map_err(|e| AppError::InternalServerError(format!("Invalid password hash: {}", e)))?;

    let argon2 = Argon2::default();
    match argon2.verify_password(password.as_bytes(), &parsed_hash) {
        Ok(_) => Ok(()),
        Err(argon2::password_hash::Error::Password) => {
            Err(AppError::Unauthorized("Invalid credentials".to_string()))
        }
        Err(e) => Err(AppError::InternalServerError(format!(
            "Failed to verify password: {}",
            e
        ))),
    }
}
