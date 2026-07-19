use crate::Agent;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info};

use super::envelope::{
    AetherApprovalDecision, Envelope, EnvelopeKind, ResumeRequest, SuspendPayload,
};
use crate::AgentOutput;
use agentverse_hitl::{ApprovalDecision as HitlDecision, InterruptKind};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct InvokeRequest {
    pub user_id: String,
    pub message: String,
}

pub async fn invoke(
    State(agent): State<Arc<Agent>>,
    Extension(limiter): Extension<Arc<agentverse_guardrails::RateLimiter>>,
    Json(request): Json<InvokeRequest>,
) -> impl IntoResponse {
    if let Err(e) = limiter.check(&request.user_id) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({ "error": e.to_string() })),
        );
    }
    if request.message.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "message must not be empty" })),
        );
    }

    match agent.invoke_stateless(&request.message).await {
        Ok(reply) => {
            info!(user_id = %request.user_id, "Invoke completed");
            (
                StatusCode::OK,
                Json(serde_json::json!({ "message": reply, "user_id": request.user_id })),
            )
        }
        Err(e) => {
            error!(error = %e, user_id = %request.user_id, "Invoke failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "healthy" })),
    )
}

pub async fn ready() -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "ready" })),
    )
}

fn interrupt_to_kind_and_prompt(kind: &InterruptKind) -> (String, String) {
    match kind {
        InterruptKind::ToolApproval { tool_name, args } => (
            "tool_approval".to_string(),
            format!("Approve tool call `{tool_name}` with args {args}?"),
        ),
        InterruptKind::PhaseGate {
            from_skill,
            to_skill,
            deliverable,
        } => (
            "phase_gate".to_string(),
            format!(
                "Approve phase transition {from_skill} → {to_skill} (deliverable: {deliverable})?"
            ),
        ),
        InterruptKind::SkillCheckpoint {
            checkpoint_name, ..
        } => (
            "skill_checkpoint".to_string(),
            format!("Approve checkpoint `{checkpoint_name}`."),
        ),
    }
}

fn map_decision(d: AetherApprovalDecision) -> HitlDecision {
    match d {
        AetherApprovalDecision::Approved => HitlDecision::Approved,
        AetherApprovalDecision::Rejected { reason } => HitlDecision::Rejected {
            reason: reason.unwrap_or_default(),
        },
        AetherApprovalDecision::Modified { payload } => {
            HitlDecision::Modified { new_args: payload }
        }
    }
}

/// Map an `AgentOutput` to a response envelope. Ends the session on `Done`;
/// leaves it alive on `Interrupted` so a later `/aether/resume` can continue it.
async fn finish(
    agent: &Agent,
    owner: &str,
    session_id: Uuid,
    req_id: Uuid,
    out: AgentOutput,
) -> Envelope {
    match out {
        AgentOutput::Done(text) => {
            let _ = agent.end_session(owner, session_id).await; // best-effort
            Envelope {
                id: req_id,
                kind: EnvelopeKind::Result,
                payload: serde_json::json!({ "output": text }),
                metadata: HashMap::new(),
            }
        }
        AgentOutput::Interrupted { approval_id, kind } => {
            let (kind_tag, prompt) = interrupt_to_kind_and_prompt(&kind);
            let payload = SuspendPayload {
                session_id: session_id.to_string(),
                approval_id: approval_id.to_string(),
                kind: kind_tag,
                prompt,
            };
            Envelope {
                id: req_id,
                kind: EnvelopeKind::Suspended,
                payload: serde_json::to_value(payload).unwrap(),
                metadata: HashMap::new(),
            }
        }
    }
}

fn error_envelope(req_id: Uuid, msg: String) -> (StatusCode, Json<serde_json::Value>) {
    let env = Envelope {
        id: req_id,
        kind: EnvelopeKind::Error,
        payload: serde_json::json!({ "error": msg }),
        metadata: HashMap::new(),
    };
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::to_value(env).unwrap()),
    )
}

pub async fn aether_invoke(
    State(agent): State<Arc<Agent>>,
    Json(env): Json<Envelope>,
) -> impl IntoResponse {
    if env.kind != EnvelopeKind::Invoke {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "expected envelope kind: invoke" })),
        );
    }

    let owner = env
        .metadata
        .get("user_id")
        .cloned()
        .unwrap_or_else(|| "aether".to_string());
    let input = env.payload["input"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    let session_id = match agent.create_session(&owner).await {
        Ok(id) => id,
        Err(e) => return error_envelope(env.id, e.to_string()),
    };

    match agent.invoke(&owner, session_id, &input).await {
        Ok(out) => {
            let resp = finish(&agent, &owner, session_id, env.id, out).await;
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        Err(e) => {
            let _ = agent.end_session(&owner, session_id).await; // best-effort cleanup
            error!(error = %e, "aether_invoke failed");
            error_envelope(env.id, e.to_string())
        }
    }
}

pub async fn aether_resume(
    State(agent): State<Arc<Agent>>,
    Json(req): Json<ResumeRequest>,
) -> impl IntoResponse {
    let session_id = match Uuid::parse_str(&req.session_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid session_id" })),
            )
        }
    };
    let approval_id = match Uuid::parse_str(&req.approval_id) {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid approval_id" })),
            )
        }
    };

    let owner = match agent.session_owner(session_id).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "session not found" })),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    };

    let decision = map_decision(req.decision);

    match agent
        .resume(&owner, session_id, approval_id, decision)
        .await
    {
        Ok(out) => {
            let resp = finish(&agent, &owner, session_id, Uuid::new_v4(), out).await;
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        Err(e) => {
            error!(error = %e, "aether_resume failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;
    use uuid::Uuid;

    async fn make_agent() -> Arc<Agent> {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::openai(
                    "test".to_string(),
                    "sk-test".to_string(),
                    Some("http://127.0.0.1:1/v1".to_string()),
                ),
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        let tools = ToolRegistry::new();
        let prompts = Arc::new(PromptRegistry::new());
        let strategy = build(
            StrategyKind::React,
            Arc::clone(&runner),
            Arc::clone(&prompts),
            Arc::clone(&tools),
            3,
        );
        let session_memory = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
        Agent::builder(runner, tools, prompts, session_memory, strategy).build()
    }

    fn make_app(agent: Arc<Agent>) -> Router {
        let limiter = Arc::new(agentverse_guardrails::RateLimiter::new(1000, 60));
        Router::new()
            .route("/health", get(health))
            .route("/ready", get(ready))
            .route("/invoke", post(invoke))
            .route("/aether/invoke", post(aether_invoke))
            .layer(axum::Extension(limiter))
            .with_state(agent)
    }

    async fn post_json(
        app: Router,
        path: &str,
        body: serde_json::Value,
    ) -> axum::http::Response<Body> {
        let req = Request::post(path)
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();
        app.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn test_health_returns_200() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let req = Request::get("/health").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["status"], "healthy");
    }

    #[tokio::test]
    async fn test_ready_returns_200() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let req = Request::get("/ready").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["status"], "ready");
    }

    #[tokio::test]
    async fn test_invoke_empty_message_returns_400() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let res = post_json(
            app,
            "/invoke",
            serde_json::json!({"user_id": "test", "message": ""}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert!(body["error"].as_str().unwrap().contains("empty"));
    }

    #[tokio::test]
    async fn test_invoke_with_message_fails_gracefully() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let res = post_json(
            app,
            "/invoke",
            serde_json::json!({"user_id": "test", "message": "Hello, agent!"}),
        )
        .await;
        // 200 if model provider accepts, 500 if unreachable (expected with test key)
        assert!(
            res.status() == StatusCode::OK || res.status() == StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[tokio::test]
    async fn test_aether_invoke_non_invoke_kind_returns_400() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000002",
            "kind": "ping",
            "payload": {},
            "metadata": {}
        });
        let res = post_json(app, "/aether/invoke", env).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_aether_invoke_with_invoke_kind() {
        let agent = make_agent().await;
        let app = make_app(agent);
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "kind": "invoke",
            "payload": {"input": "hello from aether"},
            "metadata": {}
        });
        let res = post_json(app, "/aether/invoke", env).await;
        // 200 (model ok) or 500 (API unreachable with test key) — not 400
        assert_ne!(res.status(), StatusCode::BAD_REQUEST);
    }

    fn make_versioned_app(agent: Arc<Agent>) -> Router {
        let limiter = Arc::new(agentverse_guardrails::RateLimiter::new(1000, 60));
        Router::new()
            .route("/v1/health", get(health))
            .route("/v1/ready", get(ready))
            .route("/v1/invoke", post(invoke))
            .route("/openapi.json", get(super::super::openapi::openapi_json))
            .layer(axum::Extension(limiter))
            .with_state(agent)
    }

    #[tokio::test]
    async fn v1_health_returns_200() {
        let agent = make_agent().await;
        let app = make_versioned_app(agent);
        let req = Request::get("/v1/health").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn openapi_json_returns_200() {
        let agent = make_agent().await;
        let app = make_versioned_app(agent);
        let req = Request::get("/openapi.json").body(Body::empty()).unwrap();
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["openapi"], "3.1.0");
    }

    #[tokio::test]
    async fn test_rate_limited_invoke_returns_429() {
        let agent = make_agent().await;
        let limiter = Arc::new(agentverse_guardrails::RateLimiter::new(0, 60));
        let app = Router::new()
            .route("/invoke", post(invoke))
            .layer(axum::Extension(limiter))
            .with_state(agent);
        let res = post_json(
            app,
            "/invoke",
            serde_json::json!({"user_id": "test", "message": "Hello"}),
        )
        .await;
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn map_decision_covers_all_variants() {
        use super::super::envelope::AetherApprovalDecision as A;
        use agentverse_hitl::ApprovalDecision as H;
        assert!(matches!(super::map_decision(A::Approved), H::Approved));
        assert!(matches!(
            super::map_decision(A::Rejected { reason: None }),
            H::Rejected { reason } if reason.is_empty()
        ));
        assert!(matches!(
            super::map_decision(A::Rejected { reason: Some("no".into()) }),
            H::Rejected { reason } if reason == "no"
        ));
        assert!(matches!(
            super::map_decision(A::Modified { payload: serde_json::json!({"a":1}) }),
            H::Modified { new_args } if new_args == serde_json::json!({"a":1})
        ));
    }

    #[test]
    fn interrupt_maps_to_kind_tag_and_prompt() {
        use agentverse_hitl::InterruptKind as K;
        let (kind, prompt) = super::interrupt_to_kind_and_prompt(&K::ToolApproval {
            tool_name: "echo".into(),
            args: serde_json::json!({"t": "hi"}),
        });
        assert_eq!(kind, "tool_approval");
        assert!(prompt.contains("echo"));

        let (kind, _) = super::interrupt_to_kind_and_prompt(&K::PhaseGate {
            from_skill: "a".into(),
            to_skill: "b".into(),
            deliverable: "d".into(),
        });
        assert_eq!(kind, "phase_gate");

        let (kind, _) = super::interrupt_to_kind_and_prompt(&K::SkillCheckpoint {
            checkpoint_name: "cp".into(),
            payload: serde_json::json!({}),
        });
        assert_eq!(kind, "skill_checkpoint");
    }

    #[tokio::test]
    async fn finish_done_returns_result_and_ends_session() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let req_id = Uuid::new_v4();
        let env = super::finish(
            &agent,
            "alice",
            session_id,
            req_id,
            crate::AgentOutput::Done("hello".into()),
        )
        .await;
        assert_eq!(env.kind, super::super::envelope::EnvelopeKind::Result);
        assert_eq!(env.id, req_id);
        assert_eq!(env.payload["output"], "hello");
        // session was ended (status Completed), so no longer active
        let s = agent
            .get_session("alice", session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(s.status.to_string(), "completed");
    }

    #[tokio::test]
    async fn finish_interrupted_returns_suspended_payload_and_keeps_session() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let approval_id = Uuid::new_v4();
        let env = super::finish(
            &agent,
            "alice",
            session_id,
            Uuid::new_v4(),
            crate::AgentOutput::Interrupted {
                approval_id,
                kind: agentverse_hitl::InterruptKind::ToolApproval {
                    tool_name: "echo".into(),
                    args: serde_json::json!({"t": "hi"}),
                },
            },
        )
        .await;
        assert_eq!(env.kind, super::super::envelope::EnvelopeKind::Suspended);
        assert_eq!(env.payload["session_id"], session_id.to_string());
        assert_eq!(env.payload["approval_id"], approval_id.to_string());
        assert_eq!(env.payload["kind"], "tool_approval");
        // session kept alive for resume (still active)
        let s = agent
            .get_session("alice", session_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(s.status.to_string(), "active");
    }

    #[tokio::test]
    async fn aether_resume_unknown_session_returns_404() {
        let agent = make_agent().await;
        let app = Router::new()
            .route("/aether/resume", post(super::aether_resume))
            .with_state(agent);
        let body = serde_json::json!({
            "session_id": Uuid::new_v4().to_string(),
            "approval_id": Uuid::new_v4().to_string(),
            "decision": { "type": "approved" }
        });
        let res = post_json(app, "/aether/resume", body).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn aether_resume_bad_uuid_returns_400() {
        let agent = make_agent().await;
        let app = Router::new()
            .route("/aether/resume", post(super::aether_resume))
            .with_state(agent);
        let body = serde_json::json!({
            "session_id": "not-a-uuid",
            "approval_id": Uuid::new_v4().to_string(),
            "decision": { "type": "approved" }
        });
        let res = post_json(app, "/aether/resume", body).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
