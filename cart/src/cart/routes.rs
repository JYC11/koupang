use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
};
use uuid::Uuid;

use crate::AppState;
use crate::cart::dtos::{AddToCartReq, CartRes, CartValidationRes, UpdateCartItemReq};
use crate::cart::service;
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::errors::AppError;
use shared::responses;

pub fn cart_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.auth_config.clone());

    let protected_routes = Router::new()
        .route("/", get(get_cart))
        .route("/", delete(clear_cart))
        .route("/items", post(add_item))
        .route("/items/{sku_id}", put(update_item))
        .route("/items/{sku_id}", delete(remove_item))
        .route("/validate", post(validate_cart))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    Router::new()
        .nest("/api/v1/cart", protected_routes)
        .with_state(app_state)
}

async fn get_cart(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Json<CartRes>, AppError> {
    let mut conn = state.redis_conn()?;
    let cart = service::get_cart(&mut conn, current_user.id).await?;
    Ok(Json(cart))
}

async fn add_item(
    State(state): State<AppState>,
    current_user: CurrentUser,
    Json(req): Json<AddToCartReq>,
) -> Result<Json<CartRes>, AppError> {
    let mut conn = state.redis_conn()?;
    let cart = service::add_item(&mut conn, current_user.id, req).await?;
    Ok(Json(cart))
}

async fn update_item(
    State(state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<UpdateCartItemReq>,
) -> Result<Json<CartRes>, AppError> {
    let mut conn = state.redis_conn()?;
    let cart = service::update_item_quantity(&mut conn, current_user.id, sku_id, req).await?;
    Ok(Json(cart))
}

async fn remove_item(
    State(state): State<AppState>,
    Path(sku_id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    let mut conn = state.redis_conn()?;
    service::remove_item(&mut conn, current_user.id, sku_id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Item removed from cart",
    ))
}

async fn clear_cart(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<impl IntoResponse, AppError> {
    let mut conn = state.redis_conn()?;
    service::clear_cart(&mut conn, current_user.id).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Cart cleared",
    ))
}

async fn validate_cart(
    State(state): State<AppState>,
    current_user: CurrentUser,
) -> Result<Json<CartValidationRes>, AppError> {
    let mut conn = state.redis_conn()?;
    let result = service::validate_cart(&mut conn, current_user.id).await?;
    Ok(Json(result))
}
