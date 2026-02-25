use axum::body::Body;
use axum::http::Request;
use http_body_util::BodyExt;

pub async fn body_bytes(response: axum::http::Response<Body>) -> Vec<u8> {
    response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec()
}

pub async fn body_json(response: axum::http::Response<Body>) -> serde_json::Value {
    let bytes = body_bytes(response).await;
    serde_json::from_slice(&bytes).unwrap()
}

// ── Request builders ───────────────────────────────────────

pub fn json_request(method: &str, uri: &str, body: &impl serde::Serialize) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method(method)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

pub fn authed_json_request(
    method: &str,
    uri: &str,
    token: &str,
    body: &impl serde::Serialize,
) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method(method)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

pub fn authed_get(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("GET")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap()
}

pub fn authed_delete(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .uri(uri)
        .method("DELETE")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap()
}
