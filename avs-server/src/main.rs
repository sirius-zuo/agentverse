// avs-server/src/main.rs
mod aether_client;
mod auth;
mod config;
mod envelope;
mod routes;
mod session_routes;

use agentverse::{Config, LlmRunner};
use agentverse_guardrails::RateLimiter;
use agentverse_logging as avs_logging;
use agentverse_session::{Agent as SessionAgent, SqliteSessionStore};
use agentverse_tools::{Calculator, DateTimeTool, FileSearch, HttpClient, ToolRegistry};
use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use config::ServerConfig;
use routes::{aether_invoke, health, invoke, ready, AppState};
use session_routes::{create_session, end_session, get_session, send_message, SessionState};
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_http::cors::CorsLayer;
use tracing::info;

#[tokio::main]
async fn main() {
    // Initialize logging
    avs_logging::init();

    // Load configuration
    let server_config = if let Ok(path) = std::env::var("CONFIG_PATH") {
        ServerConfig::from_file(&path).unwrap_or_else(|e| {
            eprintln!("Failed to load config from {}: {}", path, e);
            ServerConfig::from_env()
        })
    } else {
        ServerConfig::from_env()
    };

    let (model_name, provider_name) = match &server_config.agent.provider {
        agentverse::ProviderConfig::OpenAI { model_name, .. } => {
            (model_name.clone(), "openai".to_string())
        }
        agentverse::ProviderConfig::Anthropic { model_name, .. } => {
            (model_name.clone(), "anthropic".to_string())
        }
        agentverse::ProviderConfig::Gemini { model_name, .. } => {
            (model_name.clone(), "gemini".to_string())
        }
    };

    info!(
        host = %server_config.host,
        port = server_config.port,
        model = %model_name,
        provider = %provider_name,
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

    let agent = LlmRunner::from_config(agent_config).unwrap_or_else(|e| {
        eprintln!("Failed to build agent: {}", e);
        std::process::exit(1);
    });

    // Initialize session store (SQLite by default; override with SESSION_DB_URL env var)
    let session_db_url =
        std::env::var("SESSION_DB_URL").unwrap_or_else(|_| "sqlite:sessions.db".to_string());
    let session_store = Arc::new(
        SqliteSessionStore::new(&session_db_url)
            .await
            .unwrap_or_else(|e| {
                eprintln!("Failed to initialize session store: {}", e);
                std::process::exit(1);
            }),
    );
    let session_agent = Arc::new(SessionAgent::new(
        Arc::new(
            LlmRunner::from_config(Config {
                provider: server_config.agent.provider.clone(),
                max_messages: 100,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap_or_else(|e| {
                eprintln!("Failed to build session agent: {}", e);
                std::process::exit(1);
            }),
        ),
        session_store,
    ));
    let session_state = SessionState {
        agent: session_agent,
    };

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
    let state = AppState {
        agent: Arc::new(agent),
        rate_limiter,
        guardrails_enabled: server_config.guardrails.enabled,
        model_name,
        tools: Arc::new(Mutex::new(tool_registry)),
    };

    // Register with aether if AETHER_REGISTRY_URL is set
    let aether_instance_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    if let Some(registry_url) = &server_config.aether_registry_url {
        let own_url = format!("http://{}:{}", server_config.host, server_config.port);
        let aether = aether_client::AetherClient::new(
            registry_url.clone(),
            server_config.agent_name.clone(),
            own_url,
            vec![],
        );
        if let Some(reg) = aether.register().await {
            *aether_instance_id.lock().await = Some(reg.instance_id.clone());
            info!(instance_id = %reg.instance_id, "Registered with aether");
        }
    }

    // Deregister on SIGTERM (or Ctrl-C on non-Unix)
    {
        let registry_url = server_config.aether_registry_url.clone();
        let agent_name = server_config.agent_name.clone();
        let agent_url = format!("http://{}:{}", server_config.host, server_config.port);
        let instance_id_clone = Arc::clone(&aether_instance_id);

        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm =
                    signal(SignalKind::terminate()).expect("failed to install SIGTERM handler");
                sigterm.recv().await;
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await.ok();
            }

            if let Some(url) = registry_url {
                if let Some(id) = instance_id_clone.lock().await.as_deref() {
                    let client =
                        aether_client::AetherClient::new(&url, &agent_name, &agent_url, vec![]);
                    client.deregister(id).await;
                    info!("Deregistered from aether");
                }
            }
            std::process::exit(0);
        });
    }

    // Build routes
    let session_router = Router::new()
        .route("/", post(create_session))
        .route("/:session_id/messages", post(send_message))
        .route("/:session_id", get(get_session))
        .route("/:session_id", delete(end_session))
        .with_state(session_state);

    let cors = CorsLayer::permissive();
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/invoke", post(invoke))
        .route("/aether/invoke", post(aether_invoke))
        .nest("/sessions", session_router)
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
