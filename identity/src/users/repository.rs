use crate::users::dtos::{UserCreateReq, UserUpdateReq};
use crate::users::entities::UserEntity;
use chrono::Utc;
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

pub async fn create_user(tx: &mut PgConnection, req: UserCreateReq) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO users (username, password, email, phone, role) 
             VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(&req.username)
    .bind(&req.password)
    .bind(&req.email)
    .bind(&req.phone)
    .bind(&req.role)
    .execute(&mut *tx)
    .await
    .map_err(|e| AppError::InternalServerError(format!("Failed to create user: {}", e)))?;

    Ok(())
}

pub async fn update_user(
    tx: &mut PgConnection,
    id: Uuid,
    req: UserUpdateReq,
) -> Result<(), AppError> {
    let now = Utc::now();

    let result = sqlx::query(
        "UPDATE users 
             SET username = $1, email = $2, phone = $3, role = $4, updated_at = $5
             WHERE id = $6 AND deleted_at IS NULL",
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(&req.phone)
    .bind(&req.role)
    .bind(now)
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
    let now = Utc::now();

    let result =
        sqlx::query("UPDATE users SET deleted_at = $1 WHERE id = $2 AND deleted_at IS NULL")
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::InternalServerError(format!("Failed to delete user: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("User not found".to_string()));
    }

    Ok(())
}

mod tests {
    // TODO
}