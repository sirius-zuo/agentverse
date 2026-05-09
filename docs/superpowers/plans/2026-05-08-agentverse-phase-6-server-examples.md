# Phase 6: Server + Examples + CI

> **Goal:** Build the standalone server binary and 5 example agents. Final CI integration.
> **Dependencies:** All previous phases must be complete
> **Parallel:** Server and examples can partially parallelize, but server is the main integration point

---

## Overview

This phase integrates all crates into a deployable server and ships 5 example agents that demonstrate the framework's capabilities.

```
avs-server/
├── Cargo.toml
├── src/
│   ├── main.rs           # Server entry point
│   ├── config.rs         # Server configuration (env vars, config file)
│   ├── routes.rs         # HTTP routes (/health, /invoke, /webhook)
│   └── auth.rs           # API key authentication middleware
│
examples/
├── hello-agent/          # Simple ReAct agent
├── slack-hr-assistant/   # Slack + HR tools
├── rag-qa/              # Vector DB + knowledge base
├── web-search-agent/    # Search + Plan-and-Execute
└── code-review-agent/   # Code analysis + Hierarchical Planning
```

## File Structure

```
AgentVerse/
├── avs-server/
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs
│       ├── config.rs
│       ├── routes.rs
│       └── auth.rs
├── examples/
│   ├── hello-agent/
│   ├── slack-hr-assistant/
│   ├── rag-qa/
│   ├── web-search-agent/
│   └── code-review-agent/
```

---

## Task 1: avs-server — Standalone HTTP Server

**Files:**
- Create: `avs-server/Cargo.toml`
- Create: `avs-server/src/main.rs`
- Create: `avs-server/src/config.rs`
- Create: `avs-server/src/routes.rs`
- Create: `avs-server/src/auth.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-server"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[[bin]]
name = "agentverse"
path = "src/main.rs"

[dependencies]
agentverse = { path = "../avs-core" }
agentverse-react = { path = "../avs-react" }
agentverse-plan = { path = "../avs-plan" }
agentverse-guardrails = { path = "../avs-guardrails" }
agentverse-integration = { path = "../avs-integration" }
agentverse-tools = { path = "../avs-tools" }
agentverse-memory = { path = "../avs-memory" }
agentverse-memory-lancedb = { path = "../avs-memory-lancedb" }
axum.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
serde_yaml.workspace = true
utoipa = { version = "4", features = ["axum_extras"] }
utoipa-swagger-ui = { version = "6", features = ["axum"] }
```

- [ ] **Step 2: config.rs — Server configuration**

```rust
// avs-server/src/config.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub agent: AgentConfig,
    pub guardrails: GuardrailsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub model_api_key: String,
    pub model_name: String,
    pub strategy: StrategyConfig,
    pub max_iterations: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StrategyConfig {
    ReAct,
    PlanAndExecute,
    Hierarchical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardrailsConfig {
    pub enabled: bool,
    pub max_requests_per_minute: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
            agent: AgentConfig {
                model_api_key: std::env::var("MODEL_API_KEY").unwrap_or_default(),
                model_name: std::env::var("MODEL_NAME").unwrap_or_else(|_| "gpt-4".to_string()),
                strategy: StrategyConfig::ReAct,
                max_iterations: 10,
            },
            guardrails: GuardrailsConfig {
                enabled: true,
                max_requests_per_minute: 60,
            },
        }
    }
}

impl ServerConfig {
    pub fn from_file(path: &str) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;
        let config: ServerConfig = serde_yaml::from_str(&content)
            .map_err(|e| format!("Failed to parse config: {}", e))?;
        Ok(config)
    }

    pub fn from_env() -> Self {
        Self::default()
    }
}
```

- [ ] **Step 3: auth.rs — API key authentication**

```rust
// avs-server/src/auth.rs
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::Response;
use std::sync::LazyLock;

static API_KEY: LazyLock<Option<String>> = LazyLock::new(|| {
    std::env::var("API_KEY").ok()
});

/// API key authentication middleware.
pub async fn auth_middleware(
    mut parts: Parts,
    next: axum::middleware::Next,
) -> Response {
    let api_key = API_KEY.as_ref().map(|key| {
        parts
            .headers
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|h| {
                if h.starts_with("Bearer ") {
                    Some(&h[7..])
                } else {
                    None
                }
            })
            .filter(|&key| key == key)
    });

    if api_key.is_none() && API_KEY.as_ref().is_some() {
        return axum::http::Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(axum::body::Body::from("Unauthorized"))
            .unwrap();
    }

    next.run(parts).await
}
```

- [ ] **Step 4: routes.rs — HTTP routes**

```rust
// avs-server/src/routes.rs
use agentverse::{Agent, Config};
use agentverse_guardrails::{check_prompt, check_output, RateLimiter};
use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tracing::{error, info};

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub user_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct InvokeResponse {
    pub message: String,
    pub user_id: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub model: String,
}

pub struct AppState {
    pub agent: Arc<Mutex<Agent>>,
    pub rate_limiter: Arc<RateLimiter>,
    pub guardrails_enabled: bool,
}

pub async fn invoke(
    State(state): State<AppState>,
    Json(request): Json<InvokeRequest>,
) -> impl IntoResponse {
    // Rate limiting
    if let Err(e) = state.rate_limiter.check(&request.user_id) {
        return (StatusCode::TOO_MANY_REQUESTS, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response();
    }

    // Prompt guardrail
    if state.guardrails_enabled {
        if let Err(e) = check_prompt(&request.message) {
            error!(error = %e, user_id = %request.user_id, "Prompt guardrail triggered");
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": e.to_string()
            }))).into_response();
        }
    }

    info!(user_id = %request.user_id, message = %request.message, "Processing request");

    let agent = state.agent.lock().unwrap();
    let result = agent.invoke(&request.user_id, &request.message).await;

    match result {
        Ok(response) => {
            // Output guardrail
            if state.guardrails_enabled {
                if let Err(e) = check_output(&response) {
                    error!(error = %e, user_id = %request.user_id, "Output guardrail triggered");
                    return (StatusCode::FORBIDDEN, Json(serde_json::json!({
                        "error": e.to_string()
                    }))).into_response();
                }
            }

            info!(user_id = %request.user_id, "Request completed");
            (StatusCode::OK, Json(InvokeResponse {
                message: response,
                user_id: request.user_id,
            })).into_response()
        }
        Err(e) => {
            error!(error = %e, user_id = %request.user_id, "Request failed");
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": e.to_string()
            }))).into_response()
        }
    }
}

pub async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(HealthResponse {
        status: "healthy".to_string(),
        model: "gpt-4".to_string(),
    }))
}

pub async fn ready() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ready" })))
}
```

- [ ] **Step 5: main.rs — Server entry point**

```rust
// avs-server/src/main.rs
mod auth;
mod config;
mod routes;

use agentverse::{Agent, Config, PromptRegistry, ToolResult};
use agentverse_guardrails::RateLimiter;
use agentverse_tools::{Calculator, DateTimeTool, FileSearch, HttpClient, ToolRegistry};
use axum::{
    routing::{get, post},
    Router,
};
use config::ServerConfig;
use routes::{AppState, health, invoke, ready};
use std::sync::{Arc, Mutex};
use tracing::{info, Level};
use tracing_subscriber::{fmt::Layer, EnvFilter};

#[tokio::main]
async fn main() {
    // Initialize logging
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(Layer::new().with_writer(std::io::stderr))
        .with(env_filter)
        .init();

    // Load configuration
    let config = if let Ok(path) = std::env::var("CONFIG_PATH") {
        ServerConfig::from_file(&path).unwrap_or_else(|e| {
            eprintln!("Failed to load config from {}: {}", path, e);
            ServerConfig::from_env()
        })
    } else {
        ServerConfig::from_env()
    };

    info!(
        host = %config.host,
        port = config.port,
        model = %config.agent.model_name,
        strategy = ?config.agent.strategy,
        "Starting AgentVerse server"
    );

    // Build agent
    let config = Config {
        model_api_key: config.agent.model_api_key.clone(),
        model_name: config.agent.model_name.clone(),
        max_messages: 100,
        tools: vec![],
    };

    let agent = Agent::from_config(config).unwrap_or_else(|e| {
        panic!("Failed to build agent: {}", e);
    });

    // Build tool registry
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(FileSearch);
    tool_registry.register(HttpClient);
    tool_registry.register(Calculator);
    tool_registry.register(DateTimeTool);

    // Build rate limiter
    let rate_limiter = Arc::new(RateLimiter::new(
        config.guardrails.max_requests_per_minute as usize,
        60,
    ));

    // Build app state
    let state = AppState {
        agent: Arc::new(Mutex::new(agent)),
        rate_limiter,
        guardrails_enabled: config.guardrails.enabled,
    };

    // Build routes
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/invoke", post(invoke))
        .with_state(state);

    // Start server
    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind to {}: {}", addr, e));

    info!("Listening on {}", addr);
    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| panic!("Server error: {}", e));
}
```

- [ ] **Step 6: Verify + commit**

Run: `cargo check -p agentverse-server`
Run: `cargo build -p agentverse-server`
Commit: `git add avs-server/ && git commit -m "feat: add standalone server"`

---

## Task 2: Examples (5 shipped examples)

**Files:**
- Create: 5 example directories under `examples/`

Each example is a simple binary crate that demonstrates a use case. They all share the same pattern:

```rust
// examples/hello-agent/src/main.rs
use agentverse::{Agent, Config};

#[tokio::main]
async fn main() {
    let config = Config {
        model_api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
    };

    let agent = Agent::from_config(config).unwrap();

    let result = agent.invoke("user1", "Hello, what can you do?").await;
    println!("Response: {}", result.unwrap());
}
```

- [ ] **Step 1: hello-agent — Simplest agent**

```toml
# examples/hello-agent/Cargo.toml
[package]
name = "example-hello-agent"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../../avs-core" }
tokio.workspace = true
```

```rust
// examples/hello-agent/src/main.rs
use agentverse::{Agent, Config};

#[tokio::main]
async fn main() {
    let config = Config {
        model_api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
    };

    let agent = Agent::from_config(config).unwrap();

    println!("Ask the agent anything:");
    println!("> Hello, what can you do?");
    let result = agent.invoke("user1", "Hello, what can you do?").await;
    println!("Agent: {}", result.unwrap());
}
```

- [ ] **Step 2: slack-hr-assistant**

```toml
# examples/slack-hr-assistant/Cargo.toml
[package]
name = "example-slack-hr-assistant"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../../avs-core" }
agentverse-tools = { path = "../../avs-tools" }
agentverse-integration = { path = "../../avs-integration" }
tokio.workspace = true
```

```rust
// examples/slack-hr-assistant/src/main.rs
use agentverse::{Agent, Config};
use agentverse_tools::ToolRegistry;
use agentverse_integration::SlackAdapter;
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::main]
async fn main() {
    let config = Config {
        model_api_key: std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set"),
        model_name: "gpt-4".to_string(),
        max_messages: 50,
        tools: vec![],
    };

    let agent = Agent::from_config(config).unwrap();
    let agent = Arc::new(Mutex::new(agent));

    let adapter = SlackAdapter::new(
        agent,
        &std::env::var("SLACK_BOT_TOKEN").expect("SLACK_BOT_TOKEN not set"),
        &std::env::var("SLACK_SIGNING_SECRET").expect("SLACK_SIGNING_SECRET not set"),
        3000,
    );

    adapter.start().await.expect("Failed to start Slack adapter");
}
```

- [ ] **Step 3-5: rag-qa, web-search-agent, code-review-agent**

Similar pattern — each example shows a different strategy + tool combination. See the spec for which strategy/tools each uses:

| Example | Strategy | Tools |
|---|---|---|
| hello-agent | ReAct | None (basic) |
| slack-hr-assistant | ReAct | Slack adapter + built-in tools |
| rag-qa | ReAct | LanceDB memory + HttpClient |
| web-search-agent | Plan-and-Execute | HttpClient + FileSearch |
| code-review-agent | Hierarchical | FileSearch + Calculator |

- [ ] **Step 6: Verify + commit**

Run: `cargo check --examples`
Run: `cargo build --examples`
Commit: `git add examples/ && git commit -m "feat: add 5 example agents"`

---

## Task 3: Final CI Integration

- [ ] **Step 1: Update CI to build examples**

Update `.github/workflows/ci.yml` — already done in Phase 1.

- [ ] **Step 2: Add README.md**

```markdown
# AgentVerse

Lightweight, extensible AI Agent framework in Rust.

## Quick Start

```bash
cargo build --release
OPENAI_API_KEY=sk-xxx ./target/release/agentverse
```

## Crates

- `agentverse` — Core framework
- `agentverse-react` — ReAct strategy
- `agentverse-plan` — Plan-and-Execute + Hierarchical strategies
- `agentverse-router` — Dynamic strategy routing
- `agentverse-memory` — Layered memory system
- `agentverse-tools` — Built-in tools
- `agentverse-mcp` — MCP client
- `agentverse-guardrails` — Security layer
- `agentverse-integration` — Slack, Webhook adapters
- `agentverse-server` — Standalone server

## Examples

See `examples/` for: hello-agent, slack-hr-assistant, rag-qa, web-search-agent, code-review-agent
```

- [ ] **Step 3: Final workspace check**

Run: `cargo check --workspace`
Run: `cargo test --workspace`
Run: `cargo clippy --workspace -- -D warnings`
Run: `cargo fmt --all --check`
Run: `cargo doc --workspace --no-deps`

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat: add README and finalize workspace"
```

---

## Phase 6 Acceptance Criteria

- [ ] Server builds: `cargo build -p agentverse-server`
- [ ] Server runs: `OPENAI_API_KEY=sk-xxx cargo run -p agentverse-server`
- [ ] Health endpoint: `curl http://localhost:8080/health` returns `{"status":"healthy"}`
- [ ] Invoke endpoint: `curl -X POST http://localhost:8080/invoke -H "Content-Type: application/json" -d '{"user_id":"test","message":"hello"}'` returns a response
- [ ] All 5 examples build: `cargo build --examples`
- [ ] Full workspace passes CI: `cargo check/test/clippy/fmt/doc --workspace`
- [ ] README.md exists and is accurate

## Parallel Execution Notes

- Phase 6 is **sequential** — it depends on all previous phases
- Within Phase 6: server and examples can partially parallelize, but server needs stable APIs from all crates

## Estimated Effort

~6-8 hours (sequential, integration-heavy).
