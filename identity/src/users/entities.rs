use sqlx::FromRow;
use sqlx::types::Uuid;
use sqlx::types::chrono::{DateTime, Utc};

#[derive(Debug, Clone, Eq, PartialEq, Hash, FromRow)]
pub struct UserEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub username: String,
    pub password: String,
    pub email: String,
    pub phone: String,
    pub role: String,
    pub email_verified: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct EmailVerificationTokenEntity {
    pub id: Uuid,
    pub created_at: DateTime<Utc>,
    pub user_id: Uuid,
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}
