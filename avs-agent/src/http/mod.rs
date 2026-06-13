// HTTP server module — only compiled with the `http` feature.

mod aether_client;
pub(crate) mod auth;
mod config;
mod envelope;
mod routes;
mod session_routes;

use crate::Agent;
use agentverse_guardrails::RateLimiter;
use axum::{
    middleware,
    routing::{delete, get, post},
    Extension, Router,
};
use config::HttpConfig;
use routes::{aether_invoke, health, invoke, ready};
use session_routes::{create_session, end_session, get_session, send_message};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

fn build_router(agent: Arc<Agent>) -> Router {
    if std::env::var("API_KEY").is_err() {
        tracing::warn!(
            "API_KEY env var is not set — HTTP sidecar is unauthenticated. \
             Set API_KEY to require bearer token auth on all routes."
        );
    }

    let rate_limiter = Arc::new(RateLimiter::new(100, 60)); // 100 req/min per user

    let session_router = Router::new()
        .route("/", post(create_session))
        .route("/:session_id/messages", post(send_message))
        .route("/:session_id", get(get_session))
        .route("/:session_id", delete(end_session))
        .with_state(Arc::clone(&agent));

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/invoke", post(invoke))
        .route("/aether/invoke", post(aether_invoke))
        .nest("/sessions", session_router)
        .layer(Extension(rate_limiter))
        .layer(CorsLayer::permissive())
        .layer(middleware::from_fn(auth::auth_middleware))
        .with_state(agent)
}

/// Reads HOST/PORT from env, builds the axum Router, and spawns the HTTP
/// listener as a tokio background task. Returns immediately.
pub fn spawn_server(agent: Arc<Agent>) {
    let cfg = HttpConfig::from_env();
    let addr = format!("{}:{}", cfg.host, cfg.port);
    let router = build_router(agent);

    tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(&addr)
            .await
            .unwrap_or_else(|e| panic!("HTTP server failed to bind {}: {}", addr, e));
        info!("HTTP server listening on {}", addr);
        axum::serve(listener, router)
            .await
            .unwrap_or_else(|e| tracing::error!("HTTP server error: {}", e));
    });
}
