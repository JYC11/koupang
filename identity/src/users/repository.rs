use crate::users::dtos::{ValidUserCreateReq, ValidUserUpdateReq};
use crate::users::entities::{EmailVerificationTokenEntity, PasswordResetTokenEntity, UserEntity};
use crate::users::value_objects::{Email, EmailTokenId, PasswordTokenId, UserId, Username};
use chrono::{DateTime, Utc};
use shared::db::PgExec;
use shared::errors::AppError;
use sqlx::PgConnection;
use uuid::Uuid;

pub async fn get_user_by_id<'e>(
    executor: impl PgExec<'e>,
    id: UserId,
) -> Result<UserEntity, AppError> {
    sqlx::query_as::<_, UserEntity>("SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL")
        .bind(id.value())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("User not found: {}", e)))
}

pub async fn get_user_by_username<'e>(
    executor: impl PgExec<'e>,
    username: Username,
) -> Result<UserEntity, AppError> {
    sqlx::query_as::<_, UserEntity>(
        "SELECT * FROM users WHERE username = $1 AND deleted_at IS NULL",
    )
    .bind(username.as_str())
    .fetch_one(executor)
    .await
    .map_err(|e| AppError::NotFound(format!("User not found: {}", e)))
}

pub async fn create_user(
    tx: &mut PgConnection,
    req: ValidUserCreateReq,
    hashed_password: &str,
) -> Result<UserId, AppError> {
    let row: (Uuid,) = sqlx::query_as(
        "INSERT INTO users (username, password, email, phone, role)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id",
    )
    .bind(req.username.as_str())
    .bind(hashed_password)
    .bind(req.email.as_str())
    .bind(req.phone.as_str())
    .bind(&req.role)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create user: {}", e)))?;

    Ok(UserId::new(row.0))
}

pub async fn create_verification_token(
    tx: &mut PgConnection,
    user_id: UserId,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO email_verification_tokens (user_id, token, expires_at)
             VALUES ($1, $2, $3)",
    )
    .bind(user_id.value())
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

pub async fn mark_token_used(
    tx: &mut PgConnection,
    token_id: EmailTokenId,
) -> Result<(), AppError> {
    sqlx::query("UPDATE email_verification_tokens SET used_at = NOW() WHERE id = $1")
        .bind(token_id.value())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to mark token as used: {}", e))
        })?;

    Ok(())
}

pub async fn verify_user_email(tx: &mut PgConnection, user_id: UserId) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET email_verified = TRUE, updated_at = NOW() WHERE id = $1")
        .bind(user_id.value())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to verify user email: {}", e))
        })?;

    Ok(())
}

pub async fn update_user(
    tx: &mut PgConnection,
    id: UserId,
    req: ValidUserUpdateReq,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE users
             SET username = $1, email = $2, phone = $3, role = $4, updated_at = NOW()
             WHERE id = $5 AND deleted_at IS NULL",
    )
    .bind(req.username.as_str())
    .bind(req.email.as_str())
    .bind(req.phone.as_str())
    .bind(&req.role)
    .bind(id.value())
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to update user: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    Ok(())
}

pub async fn delete_user(tx: &mut PgConnection, id: UserId) -> Result<(), AppError> {
    let result =
        sqlx::query("UPDATE users SET deleted_at = NOW() WHERE id = $1 AND deleted_at IS NULL")
            .bind(id.value())
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to delete user: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    Ok(())
}

// ── Password Reset ──────────────────────────────────────────

pub async fn get_user_by_email<'e>(
    executor: impl PgExec<'e>,
    email: Email,
) -> Result<UserEntity, AppError> {
    sqlx::query_as::<_, UserEntity>("SELECT * FROM users WHERE email = $1 AND deleted_at IS NULL")
        .bind(email.as_str())
        .fetch_one(executor)
        .await
        .map_err(|e| AppError::NotFound(format!("User not found: {}", e)))
}

pub async fn create_password_reset_token(
    tx: &mut PgConnection,
    user_id: UserId,
    token: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO password_reset_tokens (user_id, token, expires_at)
             VALUES ($1, $2, $3)",
    )
    .bind(user_id.value())
    .bind(token)
    .bind(expires_at)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        AppError::InternalServerError(format!("Failed to create password reset token: {}", e))
    })?;

    Ok(())
}

pub async fn get_valid_password_reset_token<'e>(
    executor: impl PgExec<'e>,
    token: &str,
) -> Result<PasswordResetTokenEntity, AppError> {
    sqlx::query_as::<_, PasswordResetTokenEntity>(
        "SELECT * FROM password_reset_tokens
             WHERE token = $1 AND used_at IS NULL AND expires_at > NOW()",
    )
    .bind(token)
    .fetch_one(executor)
    .await
    .map_err(|_| AppError::BadRequest("Invalid or expired password reset token".to_string()))
}

pub async fn mark_reset_token_used(
    tx: &mut PgConnection,
    token_id: PasswordTokenId,
) -> Result<(), AppError> {
    sqlx::query("UPDATE password_reset_tokens SET used_at = NOW() WHERE id = $1")
        .bind(token_id.value())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to mark reset token as used: {}", e))
        })?;

    Ok(())
}

pub async fn update_user_password(
    tx: &mut PgConnection,
    user_id: UserId,
    hashed_password: &str,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET password = $1, updated_at = NOW() WHERE id = $2")
        .bind(hashed_password)
        .bind(user_id.value())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            AppError::InternalServerError(format!("Failed to update user password: {}", e))
        })?;

    Ok(())
}
