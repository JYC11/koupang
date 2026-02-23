use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub data: T,
}

#[derive(Serialize)]
pub struct MessageResponse {
    pub message: &'static str,
}

pub fn ok<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse { data })
}

pub fn success(status: StatusCode, message: &'static str) -> impl IntoResponse {
    (status, Json(MessageResponse { message }))
}

pub fn created(message: &'static str) -> impl IntoResponse {
    success(StatusCode::CREATED, message)
}
