// avs-server/src/auth.rs
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

static API_KEY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

fn get_api_key() -> &'static Option<String> {
    // Empty/whitespace API_KEY counts as unset — otherwise `API_KEY=""`
    // (e.g. compose's `${API_KEY:-}` default) would enforce auth against
    // an empty bearer token, which is trivially bypassable.
    API_KEY.get_or_init(|| {
        std::env::var("API_KEY")
            .ok()
            .filter(|k| !k.trim().is_empty())
    })
}

/// API key authentication middleware.
pub async fn auth_middleware(req: Request<Body>, next: Next) -> Response {
    if let Some(ref key) = *get_api_key() {
        let auth_header = req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer ").map(str::trim));

        if auth_header != Some(key.as_str()) {
            return StatusCode::UNAUTHORIZED.into_response();
        }
    }

    next.run(req).await
}
