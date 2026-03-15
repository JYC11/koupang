use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use uuid::Uuid;

use crate::AppState;
use crate::categories::dtos::{CategoryRes, CreateCategoryReq, UpdateCategoryReq};
use crate::categories::service;
use crate::categories::value_objects::CategoryId;
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::errors::AppError;
use shared::responses;

pub fn category_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.auth_config.clone());

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
    State(state): State<AppState>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let categories = service::list_root_categories(&state).await?;
    Ok(Json(categories))
}

async fn get_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<CategoryRes>, AppError> {
    let id = CategoryId::new(id);
    let category = service::get_category(&state, id).await?;
    Ok(Json(category))
}

async fn get_category_by_slug(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<CategoryRes>, AppError> {
    let category = service::get_category_by_slug(&state, &slug).await?;
    Ok(Json(category))
}

async fn get_children(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let id = CategoryId::new(id);
    let children = service::get_children(&state, id).await?;
    Ok(Json(children))
}

async fn get_subtree(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let id = CategoryId::new(id);
    let subtree = service::get_subtree(&state, id).await?;
    Ok(Json(subtree))
}

async fn get_ancestors(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let id = CategoryId::new(id);
    let ancestors = service::get_ancestors(&state, id).await?;
    Ok(Json(ancestors))
}

async fn create_category(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<CreateCategoryReq>,
) -> Result<impl IntoResponse, AppError> {
    let category = service::create_category(&state, &current_user, req).await?;
    Ok(responses::ok(category))
}

async fn update_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateCategoryReq>,
) -> Result<impl IntoResponse, AppError> {
    let id = CategoryId::new(id);
    service::update_category(&state, &current_user, id, req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Category updated successfully",
    ))
}

async fn delete_category(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    let id = CategoryId::new(id);
    service::delete_category(&state, &current_user, id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Category deleted successfully",
    ))
}
