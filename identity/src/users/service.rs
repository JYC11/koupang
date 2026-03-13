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
    Email, EmailTokenId, Password, PasswordTokenId, UserId, Username,
};
use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use chrono::{Duration, Utc};
use shared::auth::jwt::{CurrentUser, JwtService, JwtTokens};
use shared::auth::middleware::GetCurrentUser;
use shared::cache::RedisCache;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::email::{EmailMessage, EmailService};
use shared::errors::AppError;
use std::sync::Arc;
use uuid::Uuid;

const USER_CACHE_TTL_SECS: u64 = 300; // 5 minutes
const USER_CACHE_PREFIX: &str = "user:";

pub struct UserService {
    pool: PgPool,
    pub jwt_service: JwtService,
    email_service: Arc<dyn EmailService>,
    cache: RedisCache,
}

impl UserService {
    pub fn new(
        pool: PgPool,
        email_service: Arc<dyn EmailService>,
        redis_conn: Option<redis::aio::ConnectionManager>,
    ) -> Self {
        Self {
            pool: pool.clone(),
            jwt_service: JwtService::new(AuthConfig::new()),
            email_service,
            cache: RedisCache::new(redis_conn, USER_CACHE_TTL_SECS),
        }
    }

    pub fn new_with_config(
        pool: PgPool,
        auth_config: AuthConfig,
        email_service: Arc<dyn EmailService>,
        redis_conn: Option<redis::aio::ConnectionManager>,
    ) -> Self {
        Self {
            pool: pool.clone(),
            jwt_service: JwtService::new(auth_config),
            email_service,
            cache: RedisCache::new(redis_conn, USER_CACHE_TTL_SECS),
        }
    }

    fn user_cache_key(id: UserId) -> String {
        format!("{USER_CACHE_PREFIX}{id}")
    }

    pub async fn create_user(&self, req: UserCreateReq) -> Result<(), AppError> {
        let validated: ValidUserCreateReq = req.try_into()?;
        let hashed_password = hash_password(validated.password.as_str())?;
        let email = validated.email.as_str().to_string();

        let token = generate_verification_token();
        let token_clone = token.clone();
        let expires_at = Utc::now() + Duration::hours(24);

        with_transaction(&self.pool, |tx| {
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
        if let Err(e) = self.email_service.send_email(email_message).await {
            tracing::error!(error = %e, "Failed to send verification email");
        }

        Ok(())
    }

    pub async fn verify_email(&self, req: VerifyEmailReq) -> Result<(), AppError> {
        let token_entity = get_valid_verification_token(&self.pool, &req.token).await?;

        with_transaction(&self.pool, |tx| {
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

    pub async fn get_user(&self, id: UserId) -> Result<UserRes, AppError> {
        let user = get_user_by_id(&self.pool, id).await?;
        Ok(UserRes::new(user))
    }

    pub async fn update_user(&self, id: UserId, req: UserUpdateReq) -> Result<(), AppError> {
        let validated: ValidUserUpdateReq = req.try_into()?;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                update_user(tx.as_executor(), id, validated)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))?;
                Ok(())
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update user: {}", e)))?;

        self.cache.evict(&Self::user_cache_key(id)).await;

        Ok(())
    }

    pub async fn delete_user(&self, id: UserId) -> Result<(), AppError> {
        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                delete_user(tx.as_executor(), id)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))?;
                Ok(())
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to delete user: {}", e)))?;

        self.cache.evict(&Self::user_cache_key(id)).await;

        Ok(())
    }

    pub async fn login_user(&self, req: UserLoginReq) -> Result<UserLoginRes, AppError> {
        // Find user by username/email
        let user = get_user_by_username(&self.pool, Username::new(&req.username)?).await?;

        // Check email verification
        if !user.email_verified {
            return Err(AppError::Forbidden(
                "Please verify your email before logging in".to_string(),
            ));
        }

        // Verify password
        verify_password(&req.password, &user.password)?;

        // Generate tokens
        let access_token = self
            .jwt_service
            .generate_access_token(&user.id, &user.username, user.role)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;

        let refresh_token = self
            .jwt_service
            .generate_refresh_token(&user.id)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;

        Ok(UserLoginRes::Success(JwtTokens {
            access_token,
            refresh_token,
        }))
    }

    pub async fn generate_refresh_token(
        &self,
        req: UserRefreshReq,
    ) -> Result<UserRefreshRes, AppError> {
        // Validate refresh token and extract user ID
        let claims = self
            .jwt_service
            .validate_refresh_token(&req.refresh_token)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;

        let user = get_user_by_id(&self.pool.clone(), UserId::new(claims.sub)).await?;

        // Generate new access token
        let access_token = self
            .jwt_service
            .generate_access_token(&user.id, &user.username, user.role)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;

        Ok(UserRefreshRes { access_token })
    }

    pub async fn forgot_password(&self, req: ForgotPasswordReq) -> Result<(), AppError> {
        // Look up user by email — if not found, return Ok silently (don't leak email existence)
        let user = match get_user_by_email(&self.pool, Email::new(&req.email)?).await {
            Ok(user) => user,
            Err(_) => return Ok(()),
        };

        let token = generate_verification_token();
        let token_clone = token.clone();
        let user_id = UserId::new(user.id);
        let expires_at = Utc::now() + Duration::hours(24);

        with_transaction(&self.pool, |tx| {
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
        if let Err(e) = self.email_service.send_email(email_message).await {
            tracing::error!(error = %e, "Failed to send password reset email");
        }

        Ok(())
    }

    pub async fn reset_password(&self, req: ResetPasswordReq) -> Result<(), AppError> {
        let token_entity = get_valid_password_reset_token(&self.pool, &req.token).await?;
        let validated_password = Password::new(&req.new_password)?;
        let hashed_password = hash_password(validated_password.as_str())?;

        let token_id = PasswordTokenId::new(token_entity.id);
        let user_id = UserId::new(token_entity.user_id);

        with_transaction(&self.pool, |tx| {
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
        &self,
        user_id: UserId,
        req: ChangePasswordReq,
    ) -> Result<(), AppError> {
        let user = get_user_by_id(&self.pool, user_id).await?;

        // Verify current password
        verify_password(&req.current_password, &user.password)?;

        // Ensure new password is different
        if req.current_password == req.new_password {
            return Err(AppError::BadRequest(
                "New password must be different from the current password".to_string(),
            ));
        }

        let validated_password = Password::new(&req.new_password)?;
        let hashed_password = hash_password(validated_password.as_str())?;

        with_transaction(&self.pool, |tx| {
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

        self.cache.evict(&Self::user_cache_key(user_id)).await;

        Ok(())
    }
}

#[async_trait::async_trait]
impl GetCurrentUser for UserService {
    async fn get_by_id(&self, id: Uuid) -> Result<CurrentUser, AppError> {
        let user_id = UserId::new(id);
        let cache_key = UserService::user_cache_key(user_id);

        if let Some(cached) = self.cache.get::<CurrentUser>(&cache_key).await {
            return Ok(cached);
        }

        let user = get_user_by_id(&self.pool, user_id).await?;
        let current_user = CurrentUser {
            id: user.id,
            role: user.role,
        };
        self.cache.set(&cache_key, &current_user).await;

        Ok(current_user)
    }
}

fn generate_verification_token() -> String {
    let bytes: [u8; 32] = rand::random();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Hash a plaintext password using Argon2
fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    argon2
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| AppError::InternalServerError(format!("Failed to hash password: {}", e)))
}

/// Verify a plaintext password against a hash
fn verify_password(password: &str, hash: &str) -> Result<(), AppError> {
    let parsed_hash = PasswordHash::new(hash)
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
