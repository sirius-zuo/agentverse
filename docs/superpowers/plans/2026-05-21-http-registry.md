# HTTP Registry — agentverse (avs-server) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add aether self-registration to avs-server (on startup when `AETHER_REGISTRY_URL` is set), add a `/aether/invoke` endpoint for envelope-based workflow triggering, and remove the now-dead unix_adapter.

**Architecture:** `aether_client.rs` handles all aether communication (register, deregister, push_event) using plain `reqwest` HTTP calls. `main.rs` calls it on startup and installs a SIGTERM handler for deregistration. A new `/aether/invoke` route accepts `Envelope` JSON from aether, calls the agent, and returns an `Envelope` response. The existing `Envelope` type in `envelope.rs` is reused (its `write_envelope`/`read_envelope` async helpers become dead code and are removed).

**Tech Stack:** Rust, axum 0.7, reqwest 0.12 (already in agentverse workspace? check — if not, add it), tokio (signal handling), serde_json

**Parallelism note:** Tasks 1 and 2 are independent of the aether plan. Task 3 (integration testing the full registration flow) requires the aether registry server from the aether plan to be running.

**Prerequisite:** Check that `reqwest` is in the agentverse workspace. If not, add `reqwest = { version = "0.12", features = ["json"] }` to `Cargo.toml` workspace deps and to `avs-server/Cargo.toml` deps.

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `avs-server/src/unix_adapter.rs` | Delete | Unix socket mode (removed) |
| `avs-server/src/envelope.rs` | Modify | Remove async write/read helpers; keep Envelope + EnvelopeKind types |
| `avs-server/src/config.rs` | Modify | Add `AETHER_REGISTRY_URL`, `AGENT_NAME` env var reading |
| `avs-server/src/aether_client.rs` | Create | Register, deregister, push_event via HTTP |
| `avs-server/src/routes.rs` | Modify | Add `/aether/invoke` endpoint |
| `avs-server/src/main.rs` | Modify | Wire aether_client on startup, SIGTERM handler, remove unix_adapter |

---

### Task 1: Drop unix_adapter, slim envelope.rs, add config fields

**Files:**
- Delete: `avs-server/src/unix_adapter.rs`
- Modify: `avs-server/src/envelope.rs`
- Modify: `avs-server/src/config.rs`

- [ ] **Step 1: Delete unix_adapter.rs**

```bash
rm /Users/jinzuo/projects/agentverse/avs-server/src/unix_adapter.rs
```

- [ ] **Step 2: Remove dead async helpers from envelope.rs**

Replace `avs-server/src/envelope.rs` with the slimmed version (keep `Envelope` and `EnvelopeKind`, remove the async `write_envelope`/`read_envelope` and their tokio imports):

```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    Invoke,
    Result,
    Error,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub id: Uuid,
    pub kind: EnvelopeKind,
    pub payload: serde_json::Value,
    pub metadata: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&EnvelopeKind::Invoke).unwrap(),
            "\"invoke\""
        );
        assert_eq!(
            serde_json::to_string(&EnvelopeKind::Pong).unwrap(),
            "\"pong\""
        );
    }

    #[test]
    fn envelope_roundtrip() {
        let env = Envelope {
            id: Uuid::new_v4(),
            kind: EnvelopeKind::Result,
            payload: serde_json::json!({"output": "hello"}),
            metadata: HashMap::new(),
        };
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, env.id);
        assert_eq!(back.kind, EnvelopeKind::Result);
    }
}
```

- [ ] **Step 3: Add AETHER_REGISTRY_URL and AGENT_NAME to config**

In `avs-server/src/config.rs`, add two new fields to `ServerConfig`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub agent: AgentConfig,
    pub guardrails: GuardrailsConfig,
    pub aether_registry_url: Option<String>,  // None = standalone, Some = register with aether
    pub agent_name: String,                   // used as registry name
}
```

Update `Default for ServerConfig` to read from env:

```rust
impl Default for ServerConfig {
    fn default() -> Self {
        let api_key = std::env::var("MODEL_API_KEY").unwrap_or_default();
        if api_key.is_empty() {
            eprintln!(
                "WARNING: MODEL_API_KEY is not set. The server will start but model calls will fail."
            );
        }
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            agent: AgentConfig {
                provider: ProviderConfig::OpenAI {
                    model_name: std::env::var("MODEL_NAME").unwrap_or_else(|_| "gpt-4".to_string()),
                    api_key,
                    base_url: Some(
                        std::env::var("MODEL_BASE_URL")
                            .unwrap_or_else(|_| "http://localhost:9090/v1".to_string()),
                    ),
                },
                max_iterations: 10,
            },
            guardrails: GuardrailsConfig {
                enabled: true,
                max_requests_per_minute: 60,
            },
            aether_registry_url: std::env::var("AETHER_REGISTRY_URL").ok(),
            agent_name: std::env::var("AGENT_NAME")
                .unwrap_or_else(|_| "agentverse-agent".to_string()),
        }
    }
}
```

- [ ] **Step 4: Remove mod unix_adapter from main.rs**

In `avs-server/src/main.rs`, remove the line:

```rust
mod unix_adapter;
```

And remove the entire Unix socket mode check block:

```rust
if std::env::var("AETHER_SOCKET_PATH").is_ok() {
    info!(model = %model_name, provider = %provider_name, "Starting in Unix socket adapter mode");
    unix_adapter::run_unix(agent, model_name, provider_name).await;
    return;
}
```

- [ ] **Step 5: Verify project compiles**

```bash
cd /Users/jinzuo/projects/agentverse
cargo build -p agentverse-server 2>&1
```

Expected: compiles cleanly. No references to `unix_adapter` or `AETHER_SOCKET_PATH`.

- [ ] **Step 6: Run existing tests**

```bash
cd /Users/jinzuo/projects/agentverse
cargo test -p agentverse-server 2>&1
```

Expected: all existing tests pass.

- [ ] **Step 7: Commit**

```bash
git -C /Users/jinzuo/projects/agentverse add avs-server/src/envelope.rs avs-server/src/config.rs avs-server/src/main.rs
git -C /Users/jinzuo/projects/agentverse rm avs-server/src/unix_adapter.rs
git -C /Users/jinzuo/projects/agentverse commit -m "chore(server): drop unix adapter; add AETHER_REGISTRY_URL config"
```

---

### Task 2: AetherClient + /aether/invoke route

**Files:**
- Create: `avs-server/src/aether_client.rs`
- Modify: `avs-server/src/routes.rs`

- [ ] **Step 1: Check reqwest dependency**

```bash
grep -r "reqwest" /Users/jinzuo/projects/agentverse/Cargo.toml /Users/jinzuo/projects/agentverse/avs-server/Cargo.toml
```

If `reqwest` is missing from the workspace, add to `/Users/jinzuo/projects/agentverse/Cargo.toml`:

```toml
reqwest = { version = "0.12", features = ["json"] }
```

And to `avs-server/Cargo.toml` under `[dependencies]`:

```toml
reqwest = { workspace = true }
```

- [ ] **Step 2: Write failing tests for AetherClient**

Create `avs-server/src/aether_client.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterResponse {
    pub instance_id: String,
    pub poll_interval_secs: u64,
}

#[derive(Debug, Clone)]
pub struct AetherClient {
    registry_url: String,
    agent_name: String,
    agent_http_url: String,
    capabilities: Vec<String>,
    client: reqwest::Client,
}

#[derive(Debug)]
pub struct AetherRegistration {
    pub instance_id: String,
    pub poll_interval_secs: u64,
}

impl AetherClient {
    pub fn new(
        registry_url: impl Into<String>,
        agent_name: impl Into<String>,
        agent_http_url: impl Into<String>,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            registry_url: registry_url.into(),
            agent_name: agent_name.into(),
            agent_http_url: agent_http_url.into(),
            capabilities,
            client: reqwest::Client::new(),
        }
    }

    /// POST /registry/agents — returns instance_id on success.
    /// On network failure: logs warning and returns None (agent continues standalone).
    pub async fn register(&self) -> Option<AetherRegistration> {
        #[derive(Serialize)]
        struct Req<'a> {
            name: &'a str,
            http_url: &'a str,
            capabilities: &'a [String],
        }

        let url = format!("{}/registry/agents", self.registry_url.trim_end_matches('/'));
        match self
            .client
            .post(&url)
            .json(&Req {
                name: &self.agent_name,
                http_url: &self.agent_http_url,
                capabilities: &self.capabilities,
            })
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => {
                match res.json::<RegisterResponse>().await {
                    Ok(r) => {
                        tracing::info!(
                            instance_id = %r.instance_id,
                            poll_interval_secs = r.poll_interval_secs,
                            "Registered with aether registry"
                        );
                        Some(AetherRegistration {
                            instance_id: r.instance_id,
                            poll_interval_secs: r.poll_interval_secs,
                        })
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Failed to parse aether registration response");
                        None
                    }
                }
            }
            Ok(res) => {
                tracing::warn!(status = %res.status(), "Aether registration rejected");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "Aether registry unreachable; running standalone");
                None
            }
        }
    }

    /// DELETE /registry/instances/{instance_id} — best-effort, ignores errors.
    pub async fn deregister(&self, instance_id: &str) {
        let url = format!(
            "{}/registry/instances/{}",
            self.registry_url.trim_end_matches('/'),
            instance_id
        );
        if let Err(e) = self.client.delete(&url).send().await {
            tracing::warn!(error = %e, "Failed to deregister from aether (best-effort)");
        }
    }

    /// POST /registry/instances/{instance_id}/events — fire-and-forget.
    pub async fn push_event(&self, instance_id: &str, event_type: &str, payload: serde_json::Value) {
        let url = format!(
            "{}/registry/instances/{}/events",
            self.registry_url.trim_end_matches('/'),
            instance_id
        );
        let _ = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "event_type": event_type, "payload": payload }))
            .send()
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn make_client(base_url: &str) -> AetherClient {
        AetherClient::new(base_url, "test-agent", "http://127.0.0.1:8080", vec!["chat".to_string()])
    }

    #[tokio::test]
    async fn register_returns_instance_id_on_success() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method("POST").path("/registry/agents");
            then.status(200).json_body(serde_json::json!({
                "instance_id": "test-uuid",
                "poll_interval_secs": 30
            }));
        });

        let client = make_client(&server.base_url());
        let reg = client.register().await;
        assert!(reg.is_some());
        let r = reg.unwrap();
        assert_eq!(r.instance_id, "test-uuid");
        assert_eq!(r.poll_interval_secs, 30);
    }

    #[tokio::test]
    async fn register_returns_none_when_aether_unreachable() {
        // Port 1 is always closed
        let client = make_client("http://127.0.0.1:1");
        let reg = client.register().await;
        assert!(reg.is_none()); // standalone mode, no panic
    }

    #[tokio::test]
    async fn register_returns_none_on_server_error() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method("POST").path("/registry/agents");
            then.status(500).body("internal error");
        });

        let client = make_client(&server.base_url());
        let reg = client.register().await;
        assert!(reg.is_none());
    }

    #[tokio::test]
    async fn deregister_does_not_panic_on_error() {
        // Port 1 is always closed — deregister must not panic
        let client = make_client("http://127.0.0.1:1");
        client.deregister("any-id").await; // must not panic
    }

    #[tokio::test]
    async fn push_event_is_fire_and_forget() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method("POST").path("/registry/instances/inst-1/events");
            then.status(202);
        });

        let client = make_client(&server.base_url());
        client.push_event("inst-1", "error", serde_json::json!({"msg": "oops"})).await;
        mock.assert();
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd /Users/jinzuo/projects/agentverse
cargo test -p agentverse-server aether_client 2>&1 | head -20
```

Expected: compile error — module not declared.

- [ ] **Step 4: Add mod aether_client to main.rs**

In `avs-server/src/main.rs`, add at the top:

```rust
mod aether_client;
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd /Users/jinzuo/projects/agentverse
cargo test -p agentverse-server aether_client 2>&1
```

Expected: all 5 tests pass.

- [ ] **Step 6: Write failing test for /aether/invoke route**

In `avs-server/src/routes.rs`, add after the existing imports:

```rust
use crate::envelope::{Envelope, EnvelopeKind};
```

Add a test at the bottom of the existing `tests` module:

```rust
    #[tokio::test]
    async fn test_aether_invoke_with_envelope() {
        let state = make_state();
        let app = make_app(state);

        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "kind": "invoke",
            "payload": {"input": "hello from aether"},
            "metadata": {}
        });

        let res = post_json(app, "/aether/invoke", env).await;
        // Returns 200 (model ok) or 500 (model API unreachable with test key) — not 400
        assert_ne!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_aether_invoke_non_invoke_kind_returns_400() {
        let state = make_state();
        let app = make_app(state);

        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000002",
            "kind": "ping",
            "payload": {},
            "metadata": {}
        });

        let res = post_json(app, "/aether/invoke", env).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
```

Also add `/aether/invoke` to `make_app` in the test helper:

```rust
    fn make_app(state: AppState) -> Router {
        Router::new()
            .route("/health", get(health))
            .route("/ready", get(ready))
            .route("/invoke", post(invoke))
            .route("/aether/invoke", post(aether_invoke))
            .with_state(state)
    }
```

- [ ] **Step 7: Run tests to verify they fail**

```bash
cd /Users/jinzuo/projects/agentverse
cargo test -p agentverse-server test_aether_invoke 2>&1 | head -20
```

Expected: compile error — `aether_invoke` not defined.

- [ ] **Step 8: Implement /aether/invoke handler**

Add to `avs-server/src/routes.rs` (after the existing `ready` function):

```rust
pub async fn aether_invoke(
    State(state): State<AppState>,
    Json(env): Json<Envelope>,
) -> impl IntoResponse {
    if env.kind != EnvelopeKind::Invoke {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "expected envelope kind: invoke" })),
        );
    }

    let input = env.payload["input"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let agent = state.agent.lock().await;
    let result = agent.invoke("aether", &input).await;
    drop(agent);

    match result {
        Ok(output) => {
            let response = Envelope {
                id: env.id,
                kind: EnvelopeKind::Result,
                payload: serde_json::json!({ "output": output }),
                metadata: env.metadata,
            };
            (StatusCode::OK, Json(serde_json::to_value(response).unwrap()))
        }
        Err(e) => {
            let response = Envelope {
                id: env.id,
                kind: EnvelopeKind::Error,
                payload: serde_json::json!({ "error": e.to_string() }),
                metadata: env.metadata,
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::to_value(response).unwrap()))
        }
    }
}
```

`mod envelope;` is already declared in `main.rs` (it was there before unix_adapter was removed). No change needed to main.rs for this step. The import `use crate::envelope::{Envelope, EnvelopeKind};` goes at the top of `routes.rs` only.

- [ ] **Step 9: Run tests to verify they pass**

```bash
cd /Users/jinzuo/projects/agentverse
cargo test -p agentverse-server 2>&1
```

Expected: all tests pass including the two new aether_invoke tests.

- [ ] **Step 10: Commit**

```bash
git -C /Users/jinzuo/projects/agentverse add avs-server/src/aether_client.rs avs-server/src/routes.rs avs-server/src/main.rs
git -C /Users/jinzuo/projects/agentverse commit -m "feat(server): add AetherClient and /aether/invoke endpoint"
```

---

### Task 3: Wire AetherClient into main.rs startup and SIGTERM

**Files:**
- Modify: `avs-server/src/main.rs`
- Modify: `avs-server/src/routes.rs` (add `/aether/invoke` to the live router)

- [ ] **Step 1: Add /aether/invoke to the live axum router in main.rs**

In `avs-server/src/main.rs`, import `aether_invoke`:

```rust
use routes::{aether_invoke, health, invoke, ready, AppState};
```

Add the route to the router:

```rust
    let app = Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/invoke", post(invoke))
        .route("/aether/invoke", post(aether_invoke))
        .layer(cors)
        .layer(middleware::from_fn(auth::auth_middleware))
        .with_state(state);
```

- [ ] **Step 2: Wire AetherClient registration on startup**

In `avs-server/src/main.rs`, after building `state` and before starting the server, add:

```rust
    use aether_client::AetherClient;
    use std::sync::Arc;
    use tokio::sync::Mutex as TokioMutex;

    // Register with aether if AETHER_REGISTRY_URL is set
    let instance_id: Arc<TokioMutex<Option<String>>> = Arc::new(TokioMutex::new(None));

    if let Some(registry_url) = &server_config.aether_registry_url {
        let own_url = format!("http://{}:{}", server_config.host, server_config.port);
        let client = AetherClient::new(
            registry_url.clone(),
            server_config.agent_name.clone(),
            own_url,
            vec![],
        );
        if let Some(reg) = client.register().await {
            *instance_id.lock().await = Some(reg.instance_id.clone());
            info!(instance_id = %reg.instance_id, "Registered with aether");
        }
    }
```

- [ ] **Step 3: Wire SIGTERM handler for deregistration**

Add a SIGTERM handler that deregisters before shutdown. Add this block after the registration block (before `axum::serve`):

```rust
    // Deregister on SIGTERM
    {
        let registry_url = server_config.aether_registry_url.clone();
        let agent_name = server_config.agent_name.clone();
        let agent_url = format!("http://{}:{}", server_config.host, server_config.port);
        let instance_id_clone = Arc::clone(&instance_id);

        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate())
                    .expect("failed to install SIGTERM handler");
                sigterm.recv().await;
            }
            #[cfg(not(unix))]
            {
                tokio::signal::ctrl_c().await.ok();
            }

            if let Some(url) = registry_url {
                if let Some(id) = instance_id_clone.lock().await.as_deref() {
                    let client = AetherClient::new(&url, &agent_name, &agent_url, vec![]);
                    client.deregister(id).await;
                    info!("Deregistered from aether");
                }
            }
            std::process::exit(0);
        });
    }
```

- [ ] **Step 4: Verify final build**

```bash
cd /Users/jinzuo/projects/agentverse
cargo build -p agentverse-server 2>&1
```

Expected: builds cleanly.

- [ ] **Step 5: Run all tests**

```bash
cd /Users/jinzuo/projects/agentverse
cargo test -p agentverse-server 2>&1
```

Expected: all tests pass.

- [ ] **Step 6: Manual smoke test (optional — requires running aether registry)**

If the aether plan's `aether` binary is built:

```bash
# Terminal 1: start aether registry
AETHER_PORT=7070 /Users/jinzuo/projects/aether/target/debug/aether

# Terminal 2: start agentverse server with registration
AETHER_REGISTRY_URL=http://127.0.0.1:7070 \
AGENT_NAME=my-agent \
MODEL_API_KEY=sk-test \
MODEL_BASE_URL=http://localhost:9090/v1 \
cargo run -p agentverse-server

# Terminal 3: verify registration
curl -s http://127.0.0.1:7070/registry/agents | jq .
# Expected: [{"name":"my-agent","instance_count":1,"status":"unknown"}]
```

- [ ] **Step 7: Commit**

```bash
git -C /Users/jinzuo/projects/agentverse add avs-server/src/main.rs avs-server/src/routes.rs
git -C /Users/jinzuo/projects/agentverse commit -m "feat(server): wire aether registration on startup and SIGTERM deregister"
```
