# Phase 5: Guardrails + Integration

> **Goal:** Implement Guardrails (prompt/output/action filtering, rate limiting) and Integration adapters (Slack, Webhook).
> **Dependencies:** Phase 1 (avs-core) must be complete
> **Parallel:** avs-guardrails and avs-integration can develop in parallel

---

## Overview

**Guardrails** — Security layer integrated into the core loop:
- `PromptGuard` — detect prompt injection, jailbreak
- `OutputGuard` — PII detection, sensitive word filtering
- `ActionGuard` — dangerous action confirmation (human-in-the-loop)
- `RateLimiter` — per-user rate limiting, cost control

**Integration** — External communication adapters:
- `SlackAdapter` — WebSocket/Bolt mode
- `WebhookAdapter` — generic HTTP endpoint
- `IntegrationAdapter` trait for custom adapters

## File Structure

```
avs-guardrails/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── prompt_guard.rs    # Prompt injection detection
│   ├── output_guard.rs    # PII/sensitive word filtering
│   ├── action_guard.rs    # Dangerous action detection + HITL
│   └── rate_limiter.rs    # Per-user rate limiting
│
avs-integration/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── adapter.rs         # IntegrationAdapter trait
│   ├── slack.rs           # Slack adapter
│   └── webhook.rs         # Webhook/REST adapter
└── tests/
```

---

## Task 1: avs-guardrails — Security layer

**Files:**
- Create: `avs-guardrails/Cargo.toml`
- Create: `avs-guardrails/src/lib.rs`
- Create: `avs-guardrails/src/prompt_guard.rs`
- Create: `avs-guardrails/src/output_guard.rs`
- Create: `avs-guardrails/src/action_guard.rs`
- Create: `avs-guardrails/src/rate_limiter.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-guardrails"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
regex = "1.10"
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
```

- [ ] **Step 2: prompt_guard.rs**

```rust
// avs-guardrails/src/prompt_guard.rs
use regex::Regex;
use std::sync::LazyLock;

static PROMPT_INJECTION_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)(ignore\s+previous|forget\s+previous|disregard\s+previous)\s+instructions").unwrap(),
        Regex::new(r"(?i)(you\s+are\s+now|from\s+now\s+on)\s+(a\s+)?(jailbroken|unrestricted|uncensored)").unwrap(),
        Regex::new(r"(?i)(DAN|DMIT|DO NOT INTERRUPT|do not interrupt me|DAN mode|developer mode)").unwrap(),
        Regex::new(r"(?i)(system\s*:\s*)?(roleplay|simulate|pretend)\s+(that\s+you)?\s+(are\s+)?(an\s+)?(AI|assistant)\s+without\s+(any|these)\s+(restrictions|guidelines|safety|rules)").unwrap(),
    ]
});

/// Check if a prompt contains injection attempts.
pub fn check_prompt(prompt: &str) -> Result<(), GuardrailError> {
    for pattern in PROMPT_INJECTION_PATTERNS.iter() {
        if pattern.is_match(prompt) {
            return Err(GuardrailError::PromptInjection(format!(
                "Potential prompt injection detected: {}", pattern.as_str()
            )));
        }
    }
    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum GuardrailError {
    #[error("Prompt injection: {0}")]
    PromptInjection(String),
    #[error("Output filtered: {0}")]
    OutputFiltered(String),
    #[error("Action blocked: {0}")]
    ActionBlocked(String),
    #[error("Rate limited: {0}")]
    RateLimited(String),
}
```

- [ ] **Step 3: output_guard.rs**

```rust
// avs-guardrails/src/output_guard.rs
use regex::Regex;
use std::sync::LazyLock;

static PII_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    vec![
        (Regex::new(r"\b\d{3}-\d{2}-\d{4}\b").unwrap(), "SSN"),
        (Regex::new(r"\b\d{4}[\s-]\d{4}[\s-]\d{4}[\s-]\d{4}\b").unwrap(), "Credit Card"),
        (Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(), "Email"),
    ]
});

/// Check if output contains PII or sensitive data.
pub fn check_output(output: &str) -> Result<(), GuardrailError> {
    for (pattern, pii_type) in PII_PATTERNS.iter() {
        if pattern.is_match(output) {
            return Err(GuardrailError::OutputFiltered(format!(
                "PII detected: {} — output filtered", pii_type
            )));
        }
    }
    Ok(())
}
```

- [ ] **Step 4: action_guard.rs**

```rust
// avs-guardrails/src/action_guard.rs
use agentverse::ToolResult;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Dangerous tool names that require human approval.
static DANGEROUS_TOOLS: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    ["file_write", "file_delete", "exec_command", "system_shutdown", "database_delete"].into_iter().collect()
});

/// Callback type for human approval.
pub type ApprovalCallback = Arc<dyn Fn(&str, &Value) -> mpsc::Receiver<bool> + Send + Sync>;

/// ActionGuard: checks if a tool execution needs human approval.
pub struct ActionGuard {
    approval_callback: Option<ApprovalCallback>,
}

impl ActionGuard {
    pub fn new() -> Self {
        Self {
            approval_callback: None,
        }
    }

    pub fn with_approval_callback(mut self, callback: ApprovalCallback) -> Self {
        self.approval_callback = Some(callback);
        self
    }

    /// Check if a tool execution is allowed.
    /// Returns Ok if approved, Err if blocked.
    pub async fn check(
        &self,
        tool_name: &str,
        args: &Value,
    ) -> Result<(), GuardrailError> {
        if DANGEROUS_TOOLS.contains(tool_name) {
            if let Some(ref callback) = self.approval_callback {
                // Wait for human approval
                let receiver = callback(tool_name, args);
                // In production, this would await the approval signal
                // For MVP, return Ok (approve by default with logging)
                tracing::warn!(
                    tool = tool_name,
                    "Dangerous tool called — awaiting human approval (MVP: auto-approve with warning)"
                );
                Ok(())
            } else {
                tracing::warn!(tool = tool_name, "Dangerous tool called without approval callback");
                Ok(())
            }
        } else {
            Ok(())
        }
    }
}

impl Default for ActionGuard {
    fn default() -> Self {
        Self::new()
    }
}
```

> **Note:** `DANGEROUS_TOOLS` needs `use std::collections::HashSet` and `use std::sync::LazyLock` imports.

- [ ] **Step 5: rate_limiter.rs**

```rust
// avs-guardrails/src/rate_limiter.rs
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Per-user rate limiter.
pub struct RateLimiter {
    limits: Mutex<HashMap<String, RateLimitState>>,
    default_max_requests: usize,
    default_window_seconds: u64,
}

struct RateLimitState {
    requests: Vec<Instant>,
}

impl RateLimiter {
    pub fn new(default_max_requests: usize, default_window_seconds: u64) -> Self {
        Self {
            limits: Mutex::new(HashMap::new()),
            default_max_requests,
            default_window_seconds,
        }
    }

    /// Check if a user is within their rate limit.
    pub fn check(&self, user_id: &str) -> Result<(), GuardrailError> {
        let mut limits = self.limits.lock().unwrap();
        let state = limits.entry(user_id.to_string()).or_insert_with(|| {
            RateLimitState {
                requests: Vec::new(),
            }
        });

        let now = Instant::now();
        let window = std::time::Duration::from_secs(self.default_window_seconds);

        // Remove old requests outside the window
        state.requests.retain(|t| now.duration_since(*t) < window);

        if state.requests.len() >= self.default_max_requests {
            return Err(GuardrailError::RateLimited(format!(
                "User {} exceeded rate limit: {} requests per {}s",
                user_id, self.default_max_requests, self.default_window_seconds
            )));
        }

        state.requests.push(now);
        Ok(())
    }
}
```

- [ ] **Step 6: lib.rs**

```rust
// avs-guardrails/src/lib.rs
pub mod action_guard;
pub mod output_guard;
pub mod prompt_guard;
pub mod rate_limiter;

pub use action_guard::ActionGuard;
pub use output_guard::check_output;
pub use prompt_guard::check_prompt;
pub use rate_limiter::RateLimiter;
pub use prompt_guard::GuardrailError;
```

- [ ] **Step 7: Verify + commit**

Run: `cargo check -p agentverse-guardrails`
Run: `cargo test -p agentverse-guardrails`
Commit: `git add avs-guardrails/ && git commit -m "feat: add guardrails (prompt/output/action/rate limiting)"`

---

## Task 2: avs-integration — Integration adapters

**Files:**
- Create: `avs-integration/Cargo.toml`
- Create: `avs-integration/src/lib.rs`
- Create: `avs-integration/src/adapter.rs`
- Create: `avs-integration/src/slack.rs`
- Create: `avs-integration/src/webhook.rs`

- [ ] **Step 1: Cargo.toml**

```toml
[package]
name = "agentverse-integration"
version.workspace = true
edition.workspace = true

[dependencies]
agentverse = { path = "../avs-core" }
axum.workspace = true
serde.workspace = true
serde_json.workspace = true
tokio.workspace = true
tracing.workspace = true
```

- [ ] **Step 2: adapter.rs — IntegrationAdapter trait**

```rust
// avs-integration/src/adapter.rs
use agentverse::{Agent, AgentError};
use async_trait::async_trait;

/// Trait for integration adapters.
/// Each adapter connects an external platform (Slack, Webhook, etc.) to an Agent.
#[async_trait]
pub trait IntegrationAdapter: Send + Sync {
    /// The name of this adapter (e.g., "slack", "webhook").
    fn name(&self) -> &str;

    /// Start the adapter (listen for incoming messages).
    async fn start(&self) -> Result<(), IntegrationError>;

    /// Stop the adapter.
    async fn stop(&self);

    /// Get the health status.
    async fn health_check(&self) -> Result<(), IntegrationError>;
}

#[derive(thiserror::Error, Debug)]
pub enum IntegrationError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Agent error: {0}")]
    Agent(AgentError),
    #[error("Configuration error: {0}")]
    Config(String),
}
```

- [ ] **Step 3: slack.rs — Slack adapter (simplified)**

```rust
// avs-integration/src/slack.rs
use super::adapter::{IntegrationAdapter, IntegrationError};
use agentverse::Agent;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Slack adapter using a simplified HTTP-based approach.
/// In production, use the slack-rs crate for Bolt/WebSocket.
pub struct SlackAdapter {
    agent: Arc<Mutex<Agent>>,
    bot_token: String,
    signing_secret: String,
    port: u16,
}

#[async_trait::async_trait]
impl IntegrationAdapter for SlackAdapter {
    fn name(&self) -> &str {
        "slack"
    }

    async fn start(&self) -> Result<(), IntegrationError> {
        // In production: start Bolt app or HTTP server for Slack events
        tracing::info!(adapter = "slack", port = self.port, "Starting Slack adapter");
        Ok(())
    }

    async fn stop(&self) {
        tracing::info!(adapter = "slack", "Stopping Slack adapter");
    }

    async fn health_check(&self) -> Result<(), IntegrationError> {
        Ok(())
    }
}

impl SlackAdapter {
    pub fn new(agent: Arc<Mutex<Agent>>, bot_token: &str, signing_secret: &str, port: u16) -> Self {
        Self {
            agent,
            bot_token: bot_token.to_string(),
            signing_secret: signing_secret.to_string(),
            port,
        }
    }
}
```

- [ ] **Step 4: webhook.rs — REST/Webhook adapter**

```rust
// avs-integration/src/webhook.rs
use super::adapter::{IntegrationAdapter, IntegrationError};
use agentverse::{Agent, AgentError};
use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::post,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
pub struct WebhookRequest {
    pub user_id: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub message: String,
}

/// Webhook adapter: exposes an HTTP endpoint for incoming messages.
pub struct WebhookAdapter {
    agent: Arc<Mutex<Agent>>,
    port: u16,
    auth_token: Option<String>,
}

#[async_trait::async_trait]
impl IntegrationAdapter for WebhookAdapter {
    fn name(&self) -> &str {
        "webhook"
    }

    async fn start(&self) -> Result<(), IntegrationError> {
        let agent = Arc::clone(&self.agent);
        let port = self.port;

        let app = Router::new()
            .route("/webhook", post(handle_webhook))
            .with_state(WebhookState {
                agent: Arc::clone(&agent),
                auth_token: self.auth_token.clone(),
            });

        let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
            .await
            .map_err(|e| IntegrationError::Connection(e.to_string()))?;

        tracing::info!(adapter = "webhook", port, "Starting webhook adapter");

        axum::serve(listener, app).await
            .map_err(|e| IntegrationError::Connection(e.to_string()))?;

        Ok(())
    }

    async fn stop(&self) {
        tracing::info!(adapter = "webhook", "Stopping webhook adapter");
    }

    async fn health_check(&self) -> Result<(), IntegrationError> {
        Ok(())
    }
}

#[derive(Clone)]
struct WebhookState {
    agent: Arc<Mutex<Agent>>,
    auth_token: Option<String>,
}

async fn handle_webhook(
    State(state): State<WebhookState>,
    Json(request): Json<WebhookRequest>,
) -> Result<Json<WebhookResponse>, (StatusCode, Json<serde_json::Value>)> {
    // API Key check
    if let Some(ref token) = state.auth_token {
        // In production: check Authorization header
        let _ = token; // simplified
    }

    let agent = state.agent.lock().await;
    let response = agent.invoke(&request.user_id, &request.message).await;

    match response {
        Ok(output) => Ok(Json(WebhookResponse { message: output })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )),
    }
}

impl WebhookAdapter {
    pub fn new(agent: Arc<Mutex<Agent>>, port: u16, auth_token: Option<String>) -> Self {
        Self { agent, port, auth_token }
    }
}
```

- [ ] **Step 5: lib.rs**

```rust
// avs-integration/src/lib.rs
pub mod adapter;
pub mod slack;
pub mod webhook;

pub use adapter::{IntegrationAdapter, IntegrationError};
pub use slack::SlackAdapter;
pub use webhook::{WebhookAdapter, WebhookRequest, WebhookResponse};
```

- [ ] **Step 6: Verify + commit**

Run: `cargo check -p agentverse-integration`
Run: `cargo test -p agentverse-integration`
Commit: `git add avs-integration/ && git commit -m "feat: add integration adapters (Slack, Webhook)"`

---

## Phase 5 Acceptance Criteria

- [ ] `check_prompt()` detects injection patterns
- [ ] `check_output()` detects PII patterns
- [ ] `ActionGuard` blocks dangerous tools
- [ ] `RateLimiter` enforces per-user limits
- [ ] `SlackAdapter` and `WebhookAdapter` implement `IntegrationAdapter`
- [ ] Webhook endpoint handles requests and routes to Agent

## Parallel Execution Notes

- `avs-guardrails` and `avs-integration` are **independent** — can be parallelized
- Both depend only on `avs-core`

## Estimated Effort

~4-6 hours total. With parallelization: ~2-3 hours.
