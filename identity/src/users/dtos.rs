use crate::users::entities::UserEntity;
use serde::{Deserialize, Serialize};
use shared::auth::jwt::JwtTokens;
use shared::dto_helpers::{fmt_datetime, fmt_datetime_opt, fmt_id};
use shared::errors::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreateReq {
    pub username: String,
    pub password: String,
    pub email: String,
    pub phone: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdateReq {
    pub username: String,
    pub email: String,
    pub phone: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRes {
    pub id: String,
    pub created_at: String,
    pub updated_at: Option<String>,
    pub deleted_at: Option<String>,
    pub username: String,
    pub email: String,
    pub phone: String,
    pub role: String,
    pub email_verified: bool,
}

impl UserRes {
    pub fn new(entity: UserEntity) -> Self {
        Self {
            id: fmt_id(&entity.id),
            created_at: fmt_datetime(&entity.created_at),
            updated_at: fmt_datetime_opt(&entity.updated_at),
            deleted_at: fmt_datetime_opt(&entity.deleted_at),
            username: entity.username,
            email: entity.email,
            phone: entity.phone,
            role: entity.role,
            email_verified: entity.email_verified,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserLoginReq {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserLoginRes {
    Success(JwtTokens),
    Failure(AppError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRefreshReq {
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRefreshRes {
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyEmailReq {
    pub token: String,
}
