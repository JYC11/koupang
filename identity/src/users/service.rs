use crate::users::dtos::{
    UserCreateReq, UserLoginReq, UserLoginRes, UserRefreshReq, UserRefreshRes, UserRes,
    UserUpdateReq,
};
use crate::users::repository::{
    create_user, delete_user, get_user_by_id, get_user_by_username, update_user,
};
use argon2::password_hash::SaltString;
use argon2::password_hash::rand_core::OsRng;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use shared::auth::jwt::{CurrentUser, JwtService, JwtTokens};
use shared::auth::middleware::GetCurrentUser;
use shared::config::auth_config::AuthConfig;
use shared::db::PgPool;
use shared::db::transaction_support::{TxError, with_transaction};
use shared::errors::AppError;
use uuid::Uuid;

pub struct UserService {
    pool: PgPool,
    pub jwt_service: JwtService,
}

impl UserService {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: pool.clone(),
            jwt_service: JwtService::new(AuthConfig::new()),
        }
    }

    pub fn new_with_config(pool: PgPool, auth_config: AuthConfig) -> Self {
        Self {
            pool: pool.clone(),
            jwt_service: JwtService::new(auth_config),
        }
    }

    pub async fn create_user(&self, req: UserCreateReq) -> Result<(), AppError> {
        let hashed_password = hash_password(&req.password)?;
        let mut user_req = req;
        user_req.password = hashed_password;

        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                create_user(tx.as_executor(), user_req)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))?;
                Ok(())
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to create user: {}", e)))?;

        Ok(())
    }

    pub async fn get_user(&self, id: Uuid) -> Result<UserRes, AppError> {
        let user = get_user_by_id(&self.pool, id).await?;
        Ok(UserRes::new(user))
    }

    pub async fn update_user(&self, id: Uuid, req: UserUpdateReq) -> Result<(), AppError> {
        with_transaction(&self.pool, |tx| {
            Box::pin(async move {
                update_user(tx.as_executor(), id, req)
                    .await
                    .map_err(|e| TxError::Other(e.to_string()))?;
                Ok(())
            })
        })
        .await
        .map_err(|e| AppError::InternalServerError(format!("Failed to update user: {}", e)))?;

        Ok(())
    }

    pub async fn delete_user(&self, id: Uuid) -> Result<(), AppError> {
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

        Ok(())
    }

    pub async fn login_user(&self, req: UserLoginReq) -> Result<UserLoginRes, AppError> {
        // Find user by username/email
        let user = get_user_by_username(&self.pool, &req.username).await?;

        // Verify password
        verify_password(&req.password, &user.password)?;

        // Generate tokens
        let access_token = self
            .jwt_service
            .generate_access_token(&user.id, &user.username, &user.role)
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

        let user = get_user_by_id(&self.pool.clone(), claims.sub).await?;

        // Generate new access token
        let access_token = self
            .jwt_service
            .generate_access_token(&user.id, &user.username, &user.role)
            .map_err(|e| AppError::Unauthorized(e.to_string()))?;

        Ok(UserRefreshRes { access_token })
    }
}

impl GetCurrentUser for UserService {
    // todo make this cacheable in redis
    fn get_by_id(&self, id: Uuid) -> Result<CurrentUser, AppError> {
        let pool = self.pool.clone();
        let handle = tokio::runtime::Handle::current();

        handle.block_on(async move {
            let user = get_user_by_id(&pool, id).await?;

            Ok(CurrentUser {
                id: user.id,
                role: user.role,
            })
        })
    }
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

mod tests {
    // TODO
}
