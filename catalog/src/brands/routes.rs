use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use uuid::Uuid;

use crate::AppState;
use crate::brands::dtos::{AssociateCategoryReq, BrandRes, CreateBrandReq, UpdateBrandReq};
use crate::categories::dtos::CategoryRes;
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::errors::AppError;
use shared::responses;

pub fn brand_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.jwt_service.clone());

    let public_routes = Router::new()
        .route("/", get(list_brands))
        .route("/{id}", get(get_brand))
        .route("/slug/{slug}", get(get_brand_by_slug))
        .route("/{id}/categories", get(list_categories_for_brand));

    let protected_routes = Router::new()
        .route("/", post(create_brand))
        .route("/{id}", put(update_brand))
        .route("/{id}", delete(delete_brand))
        .route("/{id}/categories", post(associate_category))
        .route(
            "/{brand_id}/categories/{category_id}",
            delete(disassociate_category),
        )
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    Router::new()
        .nest("/api/v1/brands", public_routes.merge(protected_routes))
        .with_state(app_state)
}

// ── Handlers ───────────────────────────────────────────────

async fn list_brands(State(app_state): State<AppState>) -> Result<Json<Vec<BrandRes>>, AppError> {
    let brands = app_state.brand_service.list_brands().await?;
    Ok(Json(brands))
}

async fn get_brand(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<BrandRes>, AppError> {
    let brand = app_state.brand_service.get_brand(id).await?;
    Ok(Json(brand))
}

async fn get_brand_by_slug(
    State(app_state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<BrandRes>, AppError> {
    let brand = app_state.brand_service.get_brand_by_slug(&slug).await?;
    Ok(Json(brand))
}

async fn create_brand(
    State(app_state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<CreateBrandReq>,
) -> Result<impl IntoResponse, AppError> {
    let brand = app_state
        .brand_service
        .create_brand(&current_user, req)
        .await?;
    Ok(responses::ok(brand))
}

async fn update_brand(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateBrandReq>,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .brand_service
        .update_brand(&current_user, id, req)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Brand updated successfully",
    ))
}

async fn delete_brand(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .brand_service
        .delete_brand(&current_user, id)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Brand deleted successfully",
    ))
}

// ── Brand-Category handlers ────────────────────────────────

async fn list_categories_for_brand(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<CategoryRes>>, AppError> {
    let categories = app_state
        .brand_service
        .list_categories_for_brand(id)
        .await?;
    Ok(Json(categories))
}

async fn associate_category(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<AssociateCategoryReq>,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .brand_service
        .associate_category(&current_user, id, req.category_id)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Category associated with brand",
    ))
}

async fn disassociate_category(
    State(app_state): State<AppState>,
    Path((brand_id, category_id)): Path<(Uuid, Uuid)>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .brand_service
        .disassociate_category(&current_user, brand_id, category_id)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Category disassociated from brand",
    ))
}
