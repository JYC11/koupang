use crate::users::entities::UserEntity;
use crate::users::value_objects::{Email, Password, Phone, Username};
use serde::{Deserialize, Serialize};
use shared::auth::Role;
use shared::auth::jwt::JwtTokens;
use shared::dto_helpers::{fmt_datetime, fmt_datetime_opt, fmt_id};
use shared::errors::AppError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserCreateReq {
    pub username: String,
    pub password: String,
    pub email: String,
    pub phone: String,
    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUpdateReq {
    pub username: String,
    pub email: String,
    pub phone: String,
    pub role: Role,
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
    pub role: Role,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgotPasswordReq {
    pub email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResetPasswordReq {
    pub token: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePasswordReq {
    pub current_password: String,
    pub new_password: String,
}

// ── Validated DTOs ──────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ValidUserCreateReq {
    pub username: Username,
    pub password: Password,
    pub email: Email,
    pub phone: Phone,
    pub role: Role,
}

impl TryFrom<UserCreateReq> for ValidUserCreateReq {
    type Error = AppError;

    fn try_from(req: UserCreateReq) -> Result<Self, Self::Error> {
        Ok(Self {
            username: Username::new(&req.username)?,
            password: Password::new(&req.password)?,
            email: Email::new(&req.email)?,
            phone: Phone::new(&req.phone)?,
            role: req.role,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ValidUserUpdateReq {
    pub username: Username,
    pub email: Email,
    pub phone: Phone,
    pub role: Role,
}

impl TryFrom<UserUpdateReq> for ValidUserUpdateReq {
    type Error = AppError;

    fn try_from(req: UserUpdateReq) -> Result<Self, Self::Error> {
        Ok(Self {
            username: Username::new(&req.username)?,
            email: Email::new(&req.email)?,
            phone: Phone::new(&req.phone)?,
            role: req.role,
        })
    }
}
