// HTTP server module — only compiled with the `http` feature.

mod aether_client;
pub(crate) mod auth;
mod config;
mod envelope;
pub(crate) mod openapi;
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
use openapi::openapi_json;
use routes::{aether_invoke, health, invoke, ready};
use session_routes::{create_session, end_session, get_session, list_messages, send_message};
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

fn build_router(agent: Arc<Agent>) -> Router {
    if !api_key_configured(std::env::var("API_KEY")) {
        tracing::warn!(
            "API_KEY env var is not set — HTTP sidecar is unauthenticated. \
             Set API_KEY to require bearer token auth on all routes."
        );
    }

    let rate_limiter = Arc::new(RateLimiter::new(100, 60)); // 100 req/min per user

    let v1_session_router = Router::new()
        .route("/", post(create_session))
        .route(
            "/:session_id/messages",
            post(send_message).get(list_messages),
        )
        .route("/:session_id", get(get_session))
        .route("/:session_id", delete(end_session))
        .with_state(Arc::clone(&agent));

    let v1_router = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/invoke", post(invoke))
        .route("/aether/invoke", post(aether_invoke))
        .nest("/sessions", v1_session_router)
        .with_state(Arc::clone(&agent));

    Router::new()
        .nest("/v1", v1_router)
        // Backward-compat aliases (no version prefix)
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/invoke", post(invoke))
        .route("/aether/invoke", post(aether_invoke))
        .route("/openapi.json", get(openapi_json))
        .layer(Extension(rate_limiter))
        .layer(CorsLayer::permissive())
        .layer(middleware::from_fn(auth::auth_middleware))
        .with_state(agent)
}

/// An API key counts as configured only when present and non-empty after
/// trimming — `API_KEY=""` (e.g. compose's `${API_KEY:-}` default) is unset.
fn api_key_configured(var: Result<String, std::env::VarError>) -> bool {
    var.map(|v| !v.trim().is_empty()).unwrap_or(false)
}

fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false) // unparseable hostnames treated as non-loopback (conservative)
}

/// Secure-by-default guard: binding a non-loopback address without API_KEY
/// requires an explicit ALLOW_INSECURE=true opt-out.
fn validate_bind_security(
    host: &str,
    api_key_set: bool,
    allow_insecure: bool,
) -> Result<(), String> {
    if api_key_set || allow_insecure || is_loopback_host(host) {
        return Ok(());
    }
    Err(format!(
        "refusing to serve HTTP on non-loopback address '{host}' without authentication. \
         Set a non-empty API_KEY to require bearer-token auth on all routes, or set ALLOW_INSECURE=true \
         to accept an unauthenticated server (local development only)."
    ))
}

/// Reads HOST/PORT from env, builds the axum Router, and spawns the HTTP
/// listener as a tokio background task. Returns immediately.
pub fn spawn_server(agent: Arc<Agent>) {
    let cfg = HttpConfig::from_env();
    let api_key_set = api_key_configured(std::env::var("API_KEY"));
    let allow_insecure = std::env::var("ALLOW_INSECURE")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    // Deliberate startup panic: Agent::new has no error channel (until the
    // builder lands), and failing loudly beats silently serving unauthenticated.
    if let Err(reason) = validate_bind_security(&cfg.host, api_key_set, allow_insecure) {
        panic!("{reason}");
    }
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

#[cfg(test)]
mod bind_security_tests {
    use super::{api_key_configured, is_loopback_host, validate_bind_security};

    #[test]
    fn api_key_configured_requires_non_empty_value() {
        assert!(api_key_configured(Ok("k".to_string())));
        assert!(!api_key_configured(Ok("".to_string())));
        assert!(!api_key_configured(Ok("  ".to_string())));
        assert!(!api_key_configured(Err(std::env::VarError::NotPresent)));
    }

    #[test]
    fn loopback_hosts_are_recognized() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("example.com")); // unparseable → non-loopback
    }

    #[test]
    fn non_loopback_without_key_or_flag_is_rejected() {
        assert!(validate_bind_security("0.0.0.0", false, false).is_err());
    }

    #[test]
    fn non_loopback_with_key_is_allowed() {
        assert!(validate_bind_security("0.0.0.0", true, false).is_ok());
    }

    #[test]
    fn non_loopback_with_insecure_flag_is_allowed() {
        assert!(validate_bind_security("0.0.0.0", false, true).is_ok());
    }

    #[test]
    fn loopback_without_key_is_allowed() {
        assert!(validate_bind_security("127.0.0.1", false, false).is_ok());
    }
}
