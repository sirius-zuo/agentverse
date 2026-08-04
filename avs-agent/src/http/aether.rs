// Aether orchestrator HTTP surface: the `/aether/invoke` and `/aether/resume`
// envelope routes that expose the agent's session-based suspend/resume machinery.

use super::envelope::{
    AetherApprovalDecision, Envelope, EnvelopeKind, ResumeRequest, SuspendPayload,
};
use crate::{Agent, AgentError, AgentOutput};
use agentverse_hitl::{ApprovalDecision as HitlDecision, InterruptKind};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

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

/// Map an `AgentOutput` to a response envelope, echoing the caller's `metadata`
/// (e.g. trace context) back onto the response. Ends the session on `Done`;
/// leaves it alive on `Interrupted` so a later `/aether/resume` can continue it.
async fn finish(
    agent: &Agent,
    owner: &str,
    session_id: Uuid,
    req_id: Uuid,
    metadata: HashMap<String, String>,
    out: AgentOutput,
) -> Envelope {
    match out {
        AgentOutput::Done(text) => {
            let _ = agent.end_session(owner, session_id).await; // best-effort
            Envelope {
                id: req_id,
                kind: EnvelopeKind::Result,
                payload: serde_json::json!({ "output": text }),
                metadata,
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
                metadata,
            }
        }
    }
}

fn error_envelope(
    req_id: Uuid,
    metadata: HashMap<String, String>,
    msg: String,
) -> (StatusCode, Json<serde_json::Value>) {
    let env = Envelope {
        id: req_id,
        kind: EnvelopeKind::Error,
        payload: serde_json::json!({ "error": msg }),
        metadata,
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
    let req_id = env.id;
    let metadata = env.metadata;

    let session_id = match agent.create_session(&owner).await {
        Ok(id) => id,
        Err(e) => return error_envelope(req_id, metadata, e.to_string()),
    };

    match agent.invoke(&owner, session_id, &input).await {
        Ok(out) => {
            let resp = finish(&agent, &owner, session_id, req_id, metadata, out).await;
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        Err(e) => {
            let _ = agent.end_session(&owner, session_id).await; // best-effort cleanup
            error!(error = %e, "aether_invoke failed");
            error_envelope(req_id, metadata, e.to_string())
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

    // Resume carries no request envelope, so there is no caller metadata to echo.
    match agent
        .resume(&owner, session_id, approval_id, decision)
        .await
    {
        Ok(out) => {
            let resp = finish(
                &agent,
                &owner,
                session_id,
                Uuid::new_v4(),
                HashMap::new(),
                out,
            )
            .await;
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        Err(e) => {
            error!(error = %e, "aether_resume failed");
            let status = if matches!(e, AgentError::IncompatiblePersistedInterrupt) {
                StatusCode::CONFLICT
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, Json(serde_json::json!({ "error": e.to_string() })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::memory::Message;
    use agentverse::{
        AgentError as CoreAgentError, Config, LlmRunner, PromptRegistry, RunStrategy,
        StrategyOutcome,
    };
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::post;
    use axum::Router;
    use tower::ServiceExt;

    enum StubOutcome {
        Success,
        Failure,
    }

    struct StubStrategy {
        outcome: StubOutcome,
    }

    #[async_trait]
    impl RunStrategy for StubStrategy {
        async fn run(&self, _messages: Vec<Message>) -> Result<StrategyOutcome, CoreAgentError> {
            match self.outcome {
                StubOutcome::Success => Ok(StrategyOutcome::Done("aether reply".to_string())),
                StubOutcome::Failure => Err(CoreAgentError::Model(
                    agentverse::ModelError::ApiError("aether test failure".to_string()),
                )),
            }
        }
    }

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

    async fn make_agent_with_stub(outcome: StubOutcome) -> Arc<Agent> {
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
        let session_memory = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
        let strategy = Arc::new(StubStrategy { outcome });
        Agent::builder(runner, tools, prompts, session_memory, strategy).build()
    }

    fn aether_app(agent: Arc<Agent>) -> Router {
        Router::new()
            .route("/aether/invoke", post(aether_invoke))
            .route("/v1/aether/invoke", post(aether_invoke))
            .route("/aether/resume", post(aether_resume))
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

    #[test]
    fn map_decision_covers_all_variants() {
        use super::super::envelope::AetherApprovalDecision as A;
        use agentverse_hitl::ApprovalDecision as H;
        assert!(matches!(map_decision(A::Approved), H::Approved));
        assert!(matches!(
            map_decision(A::Rejected { reason: None }),
            H::Rejected { reason } if reason.is_empty()
        ));
        assert!(matches!(
            map_decision(A::Rejected { reason: Some("no".into()) }),
            H::Rejected { reason } if reason == "no"
        ));
        assert!(matches!(
            map_decision(A::Modified { payload: serde_json::json!({"a":1}) }),
            H::Modified { new_args } if new_args == serde_json::json!({"a":1})
        ));
    }

    #[test]
    fn interrupt_maps_to_kind_tag_and_prompt() {
        use agentverse_hitl::InterruptKind as K;
        let (kind, prompt) = interrupt_to_kind_and_prompt(&K::ToolApproval {
            tool_name: "echo".into(),
            args: serde_json::json!({"t": "hi"}),
        });
        assert_eq!(kind, "tool_approval");
        assert!(prompt.contains("echo"));

        let (kind, _) = interrupt_to_kind_and_prompt(&K::PhaseGate {
            from_skill: "a".into(),
            to_skill: "b".into(),
            deliverable: "d".into(),
        });
        assert_eq!(kind, "phase_gate");

        let (kind, _) = interrupt_to_kind_and_prompt(&K::SkillCheckpoint {
            checkpoint_name: "cp".into(),
            payload: serde_json::json!({}),
        });
        assert_eq!(kind, "skill_checkpoint");
    }

    #[tokio::test]
    async fn aether_invoke_non_invoke_kind_returns_400() {
        let agent = make_agent().await;
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000002",
            "kind": "ping",
            "payload": {},
            "metadata": {}
        });
        let res = post_json(aether_app(agent), "/aether/invoke", env).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn aether_invoke_returns_result_envelope_on_both_aliases() {
        let agent = make_agent_with_stub(StubOutcome::Success).await;
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "kind": "invoke",
            "payload": {"input": "hello from aether"},
            "metadata": {"trace_id": "aether-trace"}
        });
        for path in ["/aether/invoke", "/v1/aether/invoke"] {
            let res = post_json(aether_app(Arc::clone(&agent)), path, env.clone()).await;
            assert_eq!(res.status(), StatusCode::OK, "path: {path}");
            let body: serde_json::Value =
                serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                    .unwrap();
            assert_eq!(body["id"], env["id"]);
            assert_eq!(body["kind"], "result");
            assert_eq!(body["payload"]["output"], "aether reply");
            assert_eq!(body["metadata"], env["metadata"]);
        }
    }

    #[tokio::test]
    async fn aether_invoke_returns_error_envelope_when_agent_fails() {
        let agent = make_agent_with_stub(StubOutcome::Failure).await;
        let env = serde_json::json!({
            "id": "00000000-0000-0000-0000-000000000003",
            "kind": "invoke",
            "payload": {"input": "hello from aether"},
            "metadata": {"trace_id": "aether-trace"}
        });
        let res = post_json(aether_app(agent), "/aether/invoke", env.clone()).await;
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body: serde_json::Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["id"], env["id"]);
        assert_eq!(body["kind"], "error");
        assert!(body["payload"]["error"]
            .as_str()
            .unwrap()
            .contains("aether test failure"));
        assert_eq!(body["metadata"], env["metadata"]);
    }

    #[tokio::test]
    async fn finish_done_returns_result_and_ends_session() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let req_id = Uuid::new_v4();
        let env = finish(
            &agent,
            "alice",
            session_id,
            req_id,
            HashMap::new(),
            AgentOutput::Done("hello".into()),
        )
        .await;
        assert_eq!(env.kind, EnvelopeKind::Result);
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
        let env = finish(
            &agent,
            "alice",
            session_id,
            Uuid::new_v4(),
            HashMap::new(),
            AgentOutput::Interrupted {
                approval_id,
                kind: InterruptKind::ToolApproval {
                    tool_name: "echo".into(),
                    args: serde_json::json!({"t": "hi"}),
                },
            },
        )
        .await;
        assert_eq!(env.kind, EnvelopeKind::Suspended);
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
        let body = serde_json::json!({
            "session_id": Uuid::new_v4().to_string(),
            "approval_id": Uuid::new_v4().to_string(),
            "decision": { "type": "approved" }
        });
        let res = post_json(aether_app(agent), "/aether/resume", body).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn aether_resume_bad_uuid_returns_400() {
        let agent = make_agent().await;
        let body = serde_json::json!({
            "session_id": "not-a-uuid",
            "approval_id": Uuid::new_v4().to_string(),
            "decision": { "type": "approved" }
        });
        let res = post_json(aether_app(agent), "/aether/resume", body).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn aether_resume_on_pre_phase4_interrupt_row_returns_409_not_500() {
        // Mirrors avs-agent/tests/agent_test.rs's
        // resume_on_pre_phase4_interrupt_row_fails_with_actionable_error_not_raw_json_error,
        // but through the actual HTTP handler to confirm the status-code mapping.
        let sessions = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
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
        let agent = Agent::builder(runner, tools, prompts, sessions.clone(), strategy).build();
        let session_id = agent.create_session("alice").await.unwrap();
        let approval_id = Uuid::new_v4();
        let old_shape_state = serde_json::json!({
            "PendingToolCall": {
                "approval_id": approval_id.to_string(),
                "kind_json": serde_json::json!({"ToolApproval": {"tool_name": "wire_transfer", "args": {"amount": 100}}}).to_string(),
                "history_json": serde_json::json!([{"role": "User", "content": "Transfer $100"}]).to_string(),
                "pending_calls_json": serde_json::json!([{"name": "wire_transfer", "args": {"amount": 100}}]).to_string(),
                "active_tool_names": ["wire_transfer"],
                "skill_context_json": null
            }
        })
        .to_string();
        let manager = agentverse_session::SessionManager::new(sessions);
        manager
            .set_interrupted_state(session_id, Some(&old_shape_state))
            .await
            .unwrap();

        let body = serde_json::json!({
            "session_id": session_id.to_string(),
            "approval_id": approval_id.to_string(),
            "decision": { "type": "approved" }
        });
        let res = post_json(aether_app(agent), "/aether/resume", body).await;
        assert_eq!(res.status(), StatusCode::CONFLICT);
    }
}
