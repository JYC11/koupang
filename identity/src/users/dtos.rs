use crate::users::entities::UserEntity;
use serde::{Deserialize, Serialize};
use shared::auth::jwt::Tokens;
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
}

impl UserRes {
    pub fn new(entity: UserEntity) -> Self {
        Self {
            id: entity.id.to_string(),
            created_at: entity.created_at.to_string(),
            updated_at: entity.updated_at.map(|dt| dt.to_string()),
            deleted_at: entity.deleted_at.map(|dt| dt.to_string()),
            username: entity.username,
            email: entity.email,
            phone: entity.phone,
            role: entity.role,
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
    Success(Tokens),
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
