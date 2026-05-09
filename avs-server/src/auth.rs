// avs-server/src/auth.rs
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::http::Request;
use axum::body::Body;

static API_KEY: std::sync::LazyLock<Option<String>> = std::sync::LazyLock::new(|| {
    std::env::var("API_KEY").ok()
});

/// API key authentication middleware.
pub async fn auth_middleware(
    req: Request<Body>,
    next: Next,
) -> Response {
    if let Some(ref key) = *API_KEY {
        let auth_header = req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| {
                if h.starts_with("Bearer ") {
                    Some(&h[7..])
                } else {
                    None
                }
            });

        if auth_header != Some(key) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    next.run(req).await
}
