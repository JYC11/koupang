use axum::http::HeaderMap;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use crate::orders::dtos::{CreateOrderReq, OrderDetailRes, OrderFilterQuery, OrderListRes};
use crate::orders::service;
use crate::orders::value_objects::OrderId;
use shared::auth::jwt::CurrentUser;
use shared::auth::middleware::AuthMiddleware;
use shared::db::pagination_support::PaginatedResponse;
use shared::errors::AppError;
use shared::responses;

pub fn order_routes(app_state: AppState) -> Router {
    let auth_middleware = AuthMiddleware::new_claims_based(app_state.auth_config.clone());

    let protected_routes = Router::new()
        .route("/", post(create_order))
        .route("/", get(list_my_orders))
        .route("/{id}", get(get_order_detail))
        .route("/{id}/cancel", post(cancel_order))
        .route("/seller/me", get(list_seller_orders))
        .layer(axum::middleware::from_fn(move |req, next| {
            auth_middleware.clone().handle(req, next)
        }));

    Router::new()
        .nest("/api/v1/orders", protected_routes)
        .with_state(app_state)
}

// ── Handlers ────────────────────────────────────────────────

async fn create_order(
    State(state): State<AppState>,
    current_user: CurrentUser,
    headers: HeaderMap,
    Json(req): Json<CreateOrderReq>,
) -> Result<impl IntoResponse, AppError> {
    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AppError::BadRequest("Idempotency-Key header is required".to_string()))?;

    let order = service::create_order(&state, &current_user, idempotency_key, req).await?;
    Ok((axum::http::StatusCode::ACCEPTED, axum::Json(order)))
}

async fn get_order_detail(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
) -> Result<Json<OrderDetailRes>, AppError> {
    let order_id = OrderId::new(id);
    let detail = service::get_order_detail(&state, &current_user, order_id).await?;
    Ok(Json(detail))
}

async fn list_my_orders(
    State(state): State<AppState>,
    Query(query): Query<OrderFilterQuery>,
    current_user: CurrentUser,
) -> Result<Json<PaginatedResponse<OrderListRes>>, AppError> {
    let (params, filter) = query.into_parts();
    let result = service::list_my_orders(&state, current_user.id, params, filter).await?;
    Ok(Json(PaginatedResponse::new(result)))
}

async fn list_seller_orders(
    State(state): State<AppState>,
    Query(query): Query<OrderFilterQuery>,
    current_user: CurrentUser,
) -> Result<Json<PaginatedResponse<OrderListRes>>, AppError> {
    let (params, filter) = query.into_parts();
    let result = service::list_seller_orders(&state, current_user.id, params, filter).await?;
    Ok(Json(PaginatedResponse::new(result)))
}

#[derive(Deserialize)]
pub struct CancelOrderReq {
    pub reason: Option<String>,
}

async fn cancel_order(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    current_user: CurrentUser,
    Json(req): Json<CancelOrderReq>,
) -> Result<impl IntoResponse, AppError> {
    let order_id = OrderId::new(id);
    service::cancel_order(&state, &current_user, order_id, req.reason).await?;
    Ok(responses::success(
        axum::http::StatusCode::OK,
        "Order cancelled successfully",
    ))
}
