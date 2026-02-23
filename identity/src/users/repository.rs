use crate::users::dtos::{UserCreateReq, UserUpdateReq};
use crate::users::entities::{EmailVerificationTokenEntity, UserEntity};
use chrono::{DateTime, Utc};
use shared::db::PgExec;
use shared::errors::AppError;
use sqlx::PgConnection;
use uuid::Uuid;

pub async fn get_user_by_id<'e>(
    executor: impl PgExec<'e>,
    id: Uuid,
) -> Result<UserEntity, AppError> {
    sqlx::query_as::<_, UserEntity>("SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("User not found: {}", e)))
}

pub async fn get_user_by_username<'e>(
    executor: impl PgExec<'e>,
    username: &String,
) -> Result<UserEntity, AppError> {
    sqlx::query_as::<_, UserEntity>(
        "SELECT * FROM users WHERE username = $1 AND deleted_at IS NULL",
    )
    .bind(username)
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::NotFound(format!("User not found: {}", e)))
}

pub async fn create_user(tx: &mut PgConnection, req: UserCreateReq) -> Result<Uuid, AppError> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO users (username, password, email, phone, role)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
    )
    .bind(&req.username)
    .bind(&req.password)
    .bind(&req.email)
    .bind(&req.phone)
    .bind(&req.role)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create user: {}", e)))?;

    Ok(row.0)
}

pub async fn create_verification_token(
    tx: &mut PgConnection,
    user_id: Uuid,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO email_verification_tokens (user_id, token, expires_at)
             VALUES ($1, $2, $3)",
    )
    .bind(user_id)
    .bind(token)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        AppError::InternalServerError(format!("Failed to create verification token: {}", e))
    })?;

    Ok(())
}

pub async fn get_valid_verification_token<'e>(
    executor: impl PgExec<'e>,
    token: &str,
) -> Result<EmailVerificationTokenEntity, AppError> {
    sqlx::query_as::<_, EmailVerificationTokenEntity>(
        "SELECT * FROM email_verification_tokens
             WHERE token = $1 AND used_at IS NULL AND expires_at > NOW()",
    )
    .bind(token)
    .fetch_one(executor)
    .await
    .map_err(|_| AppError::BadRequest("Invalid or expired verification token".to_string()))
}

pub async fn mark_token_used(tx: &mut PgConnection, token_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE email_verification_tokens SET used_at = NOW() WHERE id = $1")
        .bind(token_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to mark token as used: {}", e))
        })?;

    Ok(())
}

pub async fn verify_user_email(tx: &mut PgConnection, user_id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET email_verified = TRUE, updated_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to verify user email: {}", e))
        })?;

    Ok(())
}

pub async fn update_user(
    tx: &mut PgConnection,
    id: Uuid,
    req: UserUpdateReq,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE users
             SET username = $1, email = $2, phone = $3, role = $4, updated_at = NOW()
             WHERE id = $5 AND deleted_at IS NULL",
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(&req.phone)
    .bind(&req.role)
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to update user: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    Ok(())
}

pub async fn delete_user(tx: &mut PgConnection, id: Uuid) -> Result<(), AppError> {
    let result =
        sqlx::query("UPDATE users SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to delete user: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    Ok(())
}
