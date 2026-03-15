use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;
use crate::users::dtos::{
    ChangePasswordReq, ForgotPasswordReq, ResetPasswordReq, UserCreateReq, UserLoginReq,
    UserLoginRes, UserRefreshReq, UserRefreshRes, UserRes, UserUpdateReq, VerifyEmailReq,
};
use crate::users::service;
use crate::users::value_objects::UserId;
use shared::auth::guards::require_access;
use shared::auth::jwt::{CurrentUser, JwtTokens};
use shared::auth::middleware::{AuthMiddleware, GetCurrentUser};
use shared::errors::AppError;
use shared::responses;

pub fn user_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new(
        app_state.auth_config.clone(),
        Arc::new(app_state.clone()) as Arc<dyn GetCurrentUser>,
    );

    let public_routes = Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh_token))
        .route("/verify-email", post(verify_email))
        .route("/forgot-password", post(forgot_password))
        .route("/reset-password", post(reset_password));

    let protected_routes = Router::new()
        .route("/{id}", get(get_one))
        .route("/{id}", put(update))
        .route("/{id}", delete(delete_user))
        .route("/change-password", post(change_password))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    Router::new()
        .nest("/api/v1/users", public_routes.merge(protected_routes))
        .with_state(app_state)
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<UserCreateReq>,
) -> Result<impl IntoResponse, AppError> {
    service::create_user_account(&state, req).await?;
    Ok(responses::created("User registered successfully"))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<UserLoginReq>,
) -> Result<Json<JwtTokens>, AppError> {
    let response = service::login_user(&state, req).await?;
    match response {
        UserLoginRes::Success(tokens) => Ok(Json(tokens)),
        UserLoginRes::Failure(_) => {
            Err(AppError::Unauthorized("Incorrect credentials".to_string()))
        }
    }
}

async fn refresh_token(
    State(state): State<AppState>,
    Json(req): Json<UserRefreshReq>,
) -> Result<Json<UserRefreshRes>, AppError> {
    let response = service::generate_refresh_token(&state, req).await?;
    Ok(Json(response))
}

async fn verify_email(
    State(state): State<AppState>,
    Json(req): Json<VerifyEmailReq>,
) -> Result<impl IntoResponse, AppError> {
    service::verify_email(&state, req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Email verified successfully",
    ))
}

async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<Json<UserRes>, AppError> {
    require_access(&current_user, &id)?;
    let user = service::get_user(&state, UserId::new(id)).await?;
    Ok(Json(user))
}

async fn update(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UserUpdateReq>,
) -> Result<impl IntoResponse, AppError> {
    require_access(&current_user, &id)?;
    service::update_user_account(&state, UserId::new(id), req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "User updated successfully",
    ))
}

async fn delete_user(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    require_access(&current_user, &id)?;
    service::delete_user_account(&state, UserId::new(id)).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "User deleted successfully",
    ))
}

async fn change_password(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<ChangePasswordReq>,
) -> Result<impl IntoResponse, AppError> {
    service::change_password(&state, UserId::new(current_user.id), req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Password changed successfully",
    ))
}

async fn forgot_password(
    State(state): State<AppState>,
    Json(req): Json<ForgotPasswordReq>,
) -> Result<impl IntoResponse, AppError> {
    service::forgot_password(&state, req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "If the email exists, a password reset link has been sent",
    ))
}

async fn reset_password(
    State(state): State<AppState>,
    Json(req): Json<ResetPasswordReq>,
) -> Result<impl IntoResponse, AppError> {
    service::reset_password(&state, req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Password has been reset successfully",
    ))
}
