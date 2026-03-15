use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use crate::products::dtos::{
    AddProductImageReq, CreateProductReq, CreateSkuReq, ProductDetailRes, ProductFilterQuery,
    ProductImageRes, ProductListRes, ProductRes, SkuRes, UpdateProductReq, UpdateSkuReq,
};
use crate::products::service;
use crate::products::value_objects::{ProductId, ProductImageId, SkuId};
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::db::pagination_support::PaginatedResponse;
use shared::errors::AppError;
use shared::responses;

pub fn product_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.auth_config.clone());

    let public_routes = Router::new()
        .route("/", get(list_active_products))
        .route("/{id}", get(get_product_detail))
        .route("/slug/{slug}", get(get_product_by_slug));

    let protected_routes = Router::new()
        .route("/", post(create_product))
        .route("/seller/me", get(list_my_products))
        .route("/{id}", put(update_product))
        .route("/{id}", delete(delete_product))
        // SKU routes
        .route("/{product_id}/skus", get(list_skus))
        .route("/{product_id}/skus", post(create_sku))
        .route("/skus/{sku_id}", put(update_sku))
        .route("/skus/{sku_id}", delete(delete_sku))
        .route("/skus/{sku_id}/stock", post(adjust_stock))
        // Image routes
        .route("/{product_id}/images", get(list_images))
        .route("/{product_id}/images", post(add_image))
        .route("/{product_id}/images/{image_id}", delete(delete_image))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    Router::new()
        .nest("/api/v1/products", public_routes.merge(protected_routes))
        .with_state(app_state)
}

// ── Product handlers ────────────────────────────────────────

async fn list_active_products(
    State(state): State<AppState>,
    Query(query): Query<ProductFilterQuery>,
) -> Result<Json<PaginatedResponse<ProductListRes>>, AppError> {
    let (params, mut filter) = query.into_parts();
    filter.status = None; // public endpoint always filters to active
    let result = service::list_active_products(&state, params, filter).await?;
    Ok(Json(PaginatedResponse::new(result)))
}

async fn get_product_detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProductDetailRes>, AppError> {
    let id = ProductId::new(id);
    let detail = service::get_product_detail(&state, id).await?;
    Ok(Json(detail))
}

async fn get_product_by_slug(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ProductRes>, AppError> {
    let product = service::get_product_by_slug(&state, &slug).await?;
    Ok(Json(product))
}

async fn create_product(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<CreateProductReq>,
) -> Result<impl IntoResponse, AppError> {
    let product = service::create_product(&state, &current_user, req).await?;
    Ok(responses::ok(product))
}

async fn list_my_products(
    State(state): State<AppState>,
    Query(query): Query<ProductFilterQuery>,
    current_user: CurrentUser,
) -> Result<Json<PaginatedResponse<ProductListRes>>, AppError> {
    let (params, filter) = query.into_parts();
    let result = service::list_products_by_seller(&state, current_user.id, params, filter).await?;
    Ok(Json(PaginatedResponse::new(result)))
}

async fn update_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateProductReq>,
) -> Result<impl IntoResponse, AppError> {
    let id = ProductId::new(id);
    service::update_product(&state, &current_user, id, req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Product updated successfully",
    ))
}

async fn delete_product(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    let id = ProductId::new(id);
    service::delete_product(&state, &current_user, id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Product deleted successfully",
    ))
}

// ── SKU handlers ────────────────────────────────────────────

async fn list_skus(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
) -> Result<Json<Vec<SkuRes>>, AppError> {
    let product_id = ProductId::new(product_id);
    let skus = service::list_skus(&state, product_id).await?;
    Ok(Json(skus))
}

async fn create_sku(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<CreateSkuReq>,
) -> Result<impl IntoResponse, AppError> {
    let product_id = ProductId::new(product_id);
    let sku = service::create_sku(&state, &current_user, product_id, req).await?;
    Ok(responses::ok(sku))
}

async fn update_sku(
    State(state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateSkuReq>,
) -> Result<impl IntoResponse, AppError> {
    let sku_id = SkuId::new(sku_id);
    service::update_sku(&state, &current_user, sku_id, req).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "SKU updated successfully",
    ))
}

async fn delete_sku(
    State(state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    let sku_id = SkuId::new(sku_id);
    service::delete_sku(&state, &current_user, sku_id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "SKU deleted successfully",
    ))
}

#[derive(Deserialize)]
pub struct AdjustStockReq {
    pub delta: i32,
}

async fn adjust_stock(
    State(state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<AdjustStockReq>,
) -> Result<impl IntoResponse, AppError> {
    let sku_id = SkuId::new(sku_id);
    service::adjust_stock(&state, &current_user, sku_id, req.delta).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Stock adjusted successfully",
    ))
}

// ── Image handlers ──────────────────────────────────────────

async fn list_images(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
) -> Result<Json<Vec<ProductImageRes>>, AppError> {
    let product_id = ProductId::new(product_id);
    let images = service::list_images(&state, product_id).await?;
    Ok(Json(images))
}

async fn add_image(
    State(state): State<AppState>,
    Path(product_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<AddProductImageReq>,
) -> Result<impl IntoResponse, AppError> {
    let product_id = ProductId::new(product_id);
    let image = service::add_image(&state, &current_user, product_id, req).await?;
    Ok(responses::ok(image))
}

async fn delete_image(
    State(state): State<AppState>,
    Path((product_id, image_id)): Path<(Uuid, Uuid)>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    let product_id = ProductId::new(product_id);
    let image_id = ProductImageId::new(image_id);
    service::delete_image(&state, &current_user, product_id, image_id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Image deleted successfully",
    ))
}
