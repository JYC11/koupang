use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use uuid::Uuid;

use crate::AppState;
use crate::categories::dtos::{CategoryRes, CreateCategoryReq, UpdateCategoryReq};
use crate::categories::value_objects::CategoryId;
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::errors::AppError;
use shared::responses;

pub fn category_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.jwt_service.clone());

    let public_routes = Router::new()
        .route("/", get(list_root_categories))
        .route("/{id}", get(get_category))
        .route("/slug/{slug}", get(get_category_by_slug))
        .route("/{id}/children", get(get_children))
        .route("/{id}/subtree", get(get_subtree))
        .route("/{id}/ancestors", get(get_ancestors));

    let protected_routes = Router::new()
        .route("/", post(create_category))
        .route("/{id}", put(update_category))
        .route("/{id}", delete(delete_category))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    Router::new()
        .nest("/api/v1/categories", public_routes.merge(protected_routes))
        .with_state(app_state)
}

// ── Handlers ───────────────────────────────────────────────

async fn list_root_categories(
    State(app_state): State<AppState>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let categories = app_state.category_service.list_root_categories().await?;
    Ok(Json(categories))
}

async fn get_category(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CategoryRes>, AppError> {
    let id = CategoryId::new(id);
    let category = app_state.category_service.get_category(id).await?;
    Ok(Json(category))
}

async fn get_category_by_slug(
    State(app_state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<CategoryRes>, AppError> {
    let category = app_state
        .category_service
        .get_category_by_slug(&slug)
        .await?;
    Ok(Json(category))
}

async fn get_children(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let id = CategoryId::new(id);
    let children = app_state.category_service.get_children(id).await?;
    Ok(Json(children))
}

async fn get_subtree(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let id = CategoryId::new(id);
    let subtree = app_state.category_service.get_subtree(id).await?;
    Ok(Json(subtree))
}

async fn get_ancestors(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let id = CategoryId::new(id);
    let ancestors = app_state.category_service.get_ancestors(id).await?;
    Ok(Json(ancestors))
}

async fn create_category(
    State(app_state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<CreateCategoryReq>,
) -> Result<impl IntoResponse, AppError> {
    let category = app_state
        .category_service
        .create_category(&current_user, req)
        .await?;
    Ok(responses::ok(category))
}

async fn update_category(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateCategoryReq>,
) -> Result<impl IntoResponse, AppError> {
    let id = CategoryId::new(id);
    app_state
        .category_service
        .update_category(&current_user, id, req)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Category updated successfully",
    ))
}

async fn delete_category(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    let id = CategoryId::new(id);
    app_state
        .category_service
        .delete_category(&current_user, id)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Category deleted successfully",
    ))
}
