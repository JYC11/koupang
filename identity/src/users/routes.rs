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
    UserCreateReq, UserLoginReq, UserLoginRes, UserRefreshReq, UserRefreshRes, UserRes,
    UserUpdateReq, VerifyEmailReq,
};
use shared::auth::guards::require_access;
use shared::auth::jwt::{CurrentUser, JwtTokens};
use shared::auth::middleware::{AuthMiddleware, GetCurrentUser};
use shared::errors::AppError;
use shared::responses;

pub fn user_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new(
        Arc::new(app_state.service.jwt_service.clone()),
        app_state.service.clone() as Arc<dyn GetCurrentUser>,
    );

    let public_routes = Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh_token))
        .route("/verify-email", post(verify_email));

    let protected_routes = Router::new()
        .route("/{id}", get(get_one))
        .route("/{id}", put(update))
        .route("/{id}", delete(delete_user))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    // TODO replace with GRPC!!!
    let internal_routes = Router::new().route("/{id}", get(get_one_for_auth));

    Router::new()
        .nest("/api/v1/users", public_routes.merge(protected_routes))
        .nest("/internal/users", internal_routes)
        .with_state(app_state)
}

async fn register(
    State(app_state): State<AppState>,
    Json(req): Json<UserCreateReq>,
) -> Result<impl IntoResponse, AppError> {
    app_state.service.create_user(req).await?;
    Ok(responses::created("User registered successfully"))
}

async fn login(
    State(app_state): State<AppState>,
    Json(req): Json<UserLoginReq>,
) -> Result<Json<JwtTokens>, AppError> {
    let response = app_state.service.login_user(req).await?;
    match response {
        UserLoginRes::Success(tokens) => Ok(Json(tokens)),
        UserLoginRes::Failure(_) => {
            Err(AppError::Unauthorized("Incorrect credentials".to_string()))
        }
    }
}

async fn refresh_token(
    State(app_state): State<AppState>,
    Json(req): Json<UserRefreshReq>,
) -> Result<Json<UserRefreshRes>, AppError> {
    let response = app_state.service.generate_refresh_token(req).await?;
    Ok(Json(response))
}

async fn verify_email(
    State(app_state): State<AppState>,
    Json(req): Json<VerifyEmailReq>,
) -> Result<impl IntoResponse, AppError> {
    app_state.service.verify_email(req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Email verified successfully",
    ))
}

async fn get_one(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<Json<UserRes>, AppError> {
    require_access(&current_user, &id)?;
    let user = app_state.service.get_user(id).await?;
    Ok(Json(user))
}

async fn update(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UserUpdateReq>,
) -> Result<impl IntoResponse, AppError> {
    require_access(&current_user, &id)?;
    app_state.service.update_user(id, req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "User updated successfully",
    ))
}

async fn delete_user(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    require_access(&current_user, &id)?;
    app_state.service.delete_user(id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "User deleted successfully",
    ))
}

// TODO replace with GRPC!!!
// TODO add caching!!!
async fn get_one_for_auth(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<UserRes>, AppError> {
    let user = app_state.service.get_user(id).await?;
    Ok(Json(user))
}
