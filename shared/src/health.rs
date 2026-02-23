use axum::{Json, Router, routing::get};
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
}

pub fn health_routes(service_name: &'static str) -> Router {
    Router::new().route(
        "/health",
        get(move || async move {
            Json(HealthResponse {
                status: "ok",
                service: service_name,
            })
        }),
    )
}
