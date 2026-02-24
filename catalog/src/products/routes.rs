use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use crate::products::dtos::{
    AddProductImageReq, CreateProductReq, CreateSkuReq, ProductDetailRes, ProductImageRes,
    ProductRes, SkuRes, UpdateProductReq, UpdateSkuReq,
};
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::errors::AppError;
use shared::responses;

pub fn product_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.jwt_service.clone());

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
    State(app_state): State<AppState>,
) -> Result<Json<Vec<ProductRes>>, AppError> {
    let products = app_state.service.list_active_products().await?;
    Ok(Json(products))
}

async fn get_product_detail(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<ProductDetailRes>, AppError> {
    let detail = app_state.service.get_product_detail(id).await?;
    Ok(Json(detail))
}

async fn get_product_by_slug(
    State(app_state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<ProductRes>, AppError> {
    let product = app_state.service.get_product_by_slug(&slug).await?;
    Ok(Json(product))
}

async fn create_product(
    State(app_state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<CreateProductReq>,
) -> Result<impl IntoResponse, AppError> {
    let product = app_state.service.create_product(&current_user, req).await?;
    Ok(responses::ok(product))
}

async fn list_my_products(
    State(app_state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Json<Vec<ProductRes>>, AppError> {
    let products = app_state
        .service
        .list_products_by_seller(current_user.id)
        .await?;
    Ok(Json(products))
}

async fn update_product(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateProductReq>,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .service
        .update_product(&current_user, id, req)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Product updated successfully",
    ))
}

async fn delete_product(
    State(app_state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    app_state.service.delete_product(&current_user, id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Product deleted successfully",
    ))
}

// ── SKU handlers ────────────────────────────────────────────

async fn list_skus(
    State(app_state): State<AppState>,
    Path(product_id): Path<Uuid>,
) -> Result<Json<Vec<SkuRes>>, AppError> {
    let skus = app_state.service.list_skus(product_id).await?;
    Ok(Json(skus))
}

async fn create_sku(
    State(app_state): State<AppState>,
    Path(product_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<CreateSkuReq>,
) -> Result<impl IntoResponse, AppError> {
    let sku = app_state
        .service
        .create_sku(&current_user, product_id, req)
        .await?;
    Ok(responses::ok(sku))
}

async fn update_sku(
    State(app_state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateSkuReq>,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .service
        .update_sku(&current_user, sku_id, req)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "SKU updated successfully",
    ))
}

async fn delete_sku(
    State(app_state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    app_state.service.delete_sku(&current_user, sku_id).await?;
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
    State(app_state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<AdjustStockReq>,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .service
        .adjust_stock(&current_user, sku_id, req.delta)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Stock adjusted successfully",
    ))
}

// ── Image handlers ──────────────────────────────────────────

async fn list_images(
    State(app_state): State<AppState>,
    Path(product_id): Path<Uuid>,
) -> Result<Json<Vec<ProductImageRes>>, AppError> {
    let images = app_state.service.list_images(product_id).await?;
    Ok(Json(images))
}

async fn add_image(
    State(app_state): State<AppState>,
    Path(product_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<AddProductImageReq>,
) -> Result<impl IntoResponse, AppError> {
    let image = app_state
        .service
        .add_image(&current_user, product_id, req)
        .await?;
    Ok(responses::ok(image))
}

async fn delete_image(
    State(app_state): State<AppState>,
    Path((product_id, image_id)): Path<(Uuid, Uuid)>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    app_state
        .service
        .delete_image(&current_user, product_id, image_id)
        .await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Image deleted successfully",
    ))
}
