// avs-server/src/main.rs
mod auth;
mod config;
mod envelope;
mod routes;

use agentverse::{Agent, Config};
use agentverse_guardrails::RateLimiter;
use agentverse_tools::{Calculator, DateTimeTool, FileSearch, HttpClient, ToolRegistry};
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use config::ServerConfig;
use routes::{health, invoke, ready, AppState};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::info;
use tracing_subscriber::{
    fmt::Layer, prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

#[tokio::main]
async fn main() {
    // Initialize logging
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(Layer::new().with_writer(std::io::stderr))
        .with(env_filter)
        .init();

    // Load configuration
    let server_config = if let Ok(path) = std::env::var("CONFIG_PATH") {
        ServerConfig::from_file(&path).unwrap_or_else(|e| {
            eprintln!("Failed to load config from {}: {}", path, e);
            ServerConfig::from_env()
        })
    } else {
        ServerConfig::from_env()
    };

    info!(
        host = %server_config.host,
        port = server_config.port,
        provider = ?server_config.agent.provider,
        "Starting AgentVerse server"
    );

    // Build agent
    let agent_config = Config {
        provider: server_config.agent.provider.clone(),
        max_messages: 100,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    };

    let agent = Agent::from_config(agent_config).unwrap_or_else(|e| {
        eprintln!("Failed to build agent: {}", e);
        std::process::exit(1);
    });

    // Build tool registry — wired for future tool use
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(FileSearch);
    tool_registry.register(HttpClient);
    tool_registry.register(Calculator);
    tool_registry.register(DateTimeTool);

    // Build rate limiter
    let rate_limiter = Arc::new(RateLimiter::new(
        server_config.guardrails.max_requests_per_minute as usize,
        60,
    ));

    // Build app state
    let model_name = match &server_config.agent.provider {
        agentverse::ProviderConfig::OpenAI { model_name, .. } => model_name.clone(),
        agentverse::ProviderConfig::Anthropic { model_name, .. } => model_name.clone(),
        agentverse::ProviderConfig::Gemini { model_name, .. } => model_name.clone(),
    };
    let state = AppState {
        agent: Arc::new(Mutex::new(agent)),
        rate_limiter,
        guardrails_enabled: server_config.guardrails.enabled,
        model_name,
        tools: Arc::new(Mutex::new(tool_registry)),
    };

    // Build routes
    let cors = CorsLayer::permissive();
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/invoke", post(invoke))
        .layer(cors)
        .layer(middleware::from_fn(auth::auth_middleware))
        .with_state(state);

    // Start server
    let addr = format!("{}:{}", server_config.host, server_config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        });

    info!("Listening on {}", addr);
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("Server error: {}", e));
}
