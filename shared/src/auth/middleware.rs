use async_trait::async_trait;
use axum::{extract::FromRequestParts, http::request::Parts};
use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::jwt::{self, AccessTokenClaims, AuthError, CurrentUser};
use crate::config::auth_config::AuthConfig;
use crate::errors::AppError;

#[async_trait]
pub trait GetCurrentUser: Send + Sync {
    async fn get_by_id(&self, id: Uuid) -> Result<CurrentUser, AppError>;
}

#[derive(Clone)]
pub struct AuthMiddleware {
    auth_config: AuthConfig,
    current_user_getter: Option<Arc<dyn GetCurrentUser>>,
}

impl AuthMiddleware {
    /// Full auth: validates JWT then fetches user from DB via GetCurrentUser.
    /// Used by Identity service which owns the user table.
    pub fn new(auth_config: AuthConfig, current_user_getter: Arc<dyn GetCurrentUser>) -> Self {
        Self {
            auth_config,
            current_user_getter: Some(current_user_getter),
        }
    }

    /// Claims-based auth: validates JWT and trusts the embedded claims to build CurrentUser.
    /// Used by downstream services (Catalog, Order, etc.) that don't have direct DB access to users.
    pub fn new_claims_based(auth_config: AuthConfig) -> Self {
        Self {
            auth_config,
            current_user_getter: None,
        }
    }

    /// Middleware handler function
    pub async fn handle(
        self,
        mut req: Request,
        next: Next,
    ) -> Result<Response, AuthMiddlewareError> {
        // 1. Extract Authorization header
        let headers = req.headers();
        let token = extract_bearer_token(headers)?;

        // 2. Validate and decode JWT
        let claims = jwt::validate_access_token(&self.auth_config, token).map_err(|e| match e {
            AuthError::TokenExpired => AuthMiddlewareError::TokenExpired,
            AuthError::InvalidToken => AuthMiddlewareError::InvalidToken,
            _ => AuthMiddlewareError::InvalidToken,
        })?;

        // 3. Build CurrentUser — either from DB or from JWT claims
        let current_user = match &self.current_user_getter {
            Some(getter) => getter
                .get_by_id(claims.sub)
                .await
                .map_err(|_| AuthMiddlewareError::UserNotFound)?,
            None => CurrentUser {
                id: claims.sub,
                role: claims.role.clone(),
            },
        };

        // 4. Store CurrentUser in request extensions
        req.extensions_mut().insert(current_user);
        req.extensions_mut().insert(claims);

        // 5. Continue to next handler
        Ok(next.run(req).await)
    }
}

/// Extract Bearer token from Authorization header
fn extract_bearer_token(headers: &HeaderMap) -> Result<&str, AuthMiddlewareError> {
    let auth_header = headers
        .get("authorization")
        .ok_or(AuthMiddlewareError::MissingAuthHeader)?
        .to_str()
        .map_err(|_| AuthMiddlewareError::InvalidAuthHeader)?;

    if !auth_header.starts_with("Bearer ") {
        return Err(AuthMiddlewareError::InvalidAuthHeader);
    }

    Ok(auth_header.trim_start_matches("Bearer ").trim())
}

#[derive(Debug)]
pub enum AuthMiddlewareError {
    InternalError,
    MissingAuthHeader,
    InvalidAuthHeader,
    InvalidToken,
    TokenExpired,
    UserNotFound,
}

impl std::fmt::Display for AuthMiddlewareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthMiddlewareError::InternalError => write!(f, "Internal error"),
            AuthMiddlewareError::MissingAuthHeader => write!(f, "Missing Authorization header"),
            AuthMiddlewareError::InvalidAuthHeader => {
                write!(f, "Invalid Authorization header format")
            }
            AuthMiddlewareError::InvalidToken => write!(f, "Invalid token"),
            AuthMiddlewareError::TokenExpired => write!(f, "Token has expired"),
            AuthMiddlewareError::UserNotFound => write!(f, "User not found"),
        }
    }
}

impl std::error::Error for AuthMiddlewareError {}

impl IntoResponse for AuthMiddlewareError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            AuthMiddlewareError::InternalError => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Internal error")
            }
            AuthMiddlewareError::MissingAuthHeader => {
                (StatusCode::UNAUTHORIZED, "Missing Authorization header")
            }
            AuthMiddlewareError::InvalidAuthHeader => (
                StatusCode::UNAUTHORIZED,
                "Invalid Authorization header format",
            ),
            AuthMiddlewareError::InvalidToken => (StatusCode::UNAUTHORIZED, "Invalid token"),
            AuthMiddlewareError::TokenExpired => (StatusCode::UNAUTHORIZED, "Token has expired"),
            AuthMiddlewareError::UserNotFound => {
                (StatusCode::FORBIDDEN, "User not found or access denied")
            }
        };

        (status, message).into_response()
    }
}

// Extractor for handlers to easily get CurrentUser from request

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<CurrentUser>().cloned().ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "CurrentUser not found in request extensions",
        ))
    }
}

impl<S> FromRequestParts<S> for AccessTokenClaims
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts.extensions.get::<AccessTokenClaims>().cloned().ok_or((
            StatusCode::INTERNAL_SERVER_ERROR,
            "AccessTokenClaims not found in request extensions",
        ))
    }
}
