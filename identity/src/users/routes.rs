use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post, put},
};
use std::sync::Arc;
use uuid::Uuid;

use crate::AppState;
use crate::users::dtos::{
    UserCreateReq, UserLoginReq, UserLoginRes, UserRefreshReq, UserRefreshRes, UserRes,
    UserUpdateReq,
};
use shared::auth::jwt::{CurrentUser, JwtTokens};
use shared::auth::middleware::{AuthMiddleware, GetCurrentUser};
use shared::errors::AppError;

pub fn routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new(
        Arc::new(app_state.service.jwt_service.clone()),
        app_state.service.clone() as Arc<dyn GetCurrentUser>,
    );

    let public_routes = Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh_token));

    let protected_routes = Router::new()
        .route("/:id", get(get_one))
        .route("/:id", put(update))
        .route("/:id", delete(delete_user))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    // TODO replace with GRPC!!!
    let internal_routes = Router::new().route("/:id", get(get_one_for_auth));

    Router::new()
        .nest("/api/v1/users", public_routes.merge(protected_routes))
        .nest("/internal/users", internal_routes)
        .with_state(app_state)
}

async fn register(
    State(app_state): State<AppState>,
    Json(req): Json<UserCreateReq>,
) -> Result<(StatusCode, &'static str), AppError> {
    app_state.service.create_user(req).await?;
    Ok((StatusCode::CREATED, "User registered successfully"))
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

async fn get_one(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<Json<UserRes>, AppError> {
    // Check authorization: user can access their own data or admin can access any
    if current_user.id != id && current_user.role != "ADMIN" {
        return Err(AppError::Forbidden(
            "You don't have permission to access this resource".to_string(),
        ));
    }

    let user = app_state.service.get_user(id).await?;
    Ok(Json(user))
}

async fn update(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UserUpdateReq>,
) -> Result<(StatusCode, &'static str), AppError> {
    if current_user.id != id && current_user.role != "ADMIN" {
        return Err(AppError::Forbidden(
            "You don't have permission to update this resource".to_string(),
        ));
    }

    app_state.service.update_user(id, req).await?;
    Ok((StatusCode::OK, "User updated successfully"))
}

async fn delete_user(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<(StatusCode, &'static str), AppError> {
    if current_user.id != id && current_user.role != "ADMIN" {
        return Err(AppError::Forbidden(
            "You don't have permission to delete this resource".to_string(),
        ));
    }

    app_state.service.delete_user(id).await?;
    Ok((StatusCode::OK, "User deleted successfully"))
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
