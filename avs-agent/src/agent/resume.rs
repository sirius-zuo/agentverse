use super::{Agent, AgentError, AgentOutput, PhaseTransition};
use agentverse_hitl::{ApprovalDecision, HitlContext, InterruptKind};
use agentverse_session::{InterruptedState, SessionId, SessionMemoryError};
use agentverse_skill::{SkillContext, SkillError};
use uuid::Uuid;

impl Agent {
    pub(super) async fn handle_tool_interrupt(
        &self,
        _user_id: &str,
        session_id: SessionId,
        interrupt: agentverse::hitl::HitlInterrupt,
        skill_ctx: &Option<SkillContext>,
    ) -> Result<AgentOutput, AgentError> {
        let agentverse::hitl::HitlInterrupt {
            approval_id,
            kind_json,
            history,
            pending_calls,
            active_tool_names,
        } = interrupt;

        // Determine variant from kind_json
        let kind: InterruptKind = serde_json::from_str(&kind_json).map_err(|e| {
            AgentError::Llm(agentverse::AgentError::Memory(format!(
                "bad HITL kind_json: {e}"
            )))
        })?;

        let skill_ctx_json = skill_ctx
            .as_ref()
            .and_then(|c| serde_json::to_string(c).ok());
        let history_json = serde_json::to_string(&history)?;
        let pending_calls_json = serde_json::to_string(&pending_calls)?;

        let state = match kind {
            InterruptKind::SkillCheckpoint { .. } => InterruptedState::PendingCheckpoint {
                approval_id: approval_id.to_string(),
                kind_json,
                history_json,
                active_tool_names,
                skill_context_json: skill_ctx_json,
            },
            _ => InterruptedState::PendingToolCall {
                approval_id: approval_id.to_string(),
                kind_json,
                history_json,
                pending_calls_json,
                active_tool_names,
                skill_context_json: skill_ctx_json,
            },
        };

        let state_json = serde_json::to_string(&state)?;
        self.sessions
            .set_interrupted_state(session_id, Some(&state_json))
            .await?;
        self.sessions
            .update_status(session_id, agentverse_session::SessionStatus::Interrupted)
            .await?;

        agentverse::metrics::record_hitl_transition(
            agentverse::metrics::HitlTransition::Interrupted,
        );
        Ok(AgentOutput::Interrupted { approval_id, kind })
    }

    pub async fn resume(
        &self,
        user_id: &str,
        session_id: SessionId,
        approval_id: Uuid,
        decision: ApprovalDecision,
    ) -> Result<AgentOutput, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;

        let state_json = self
            .sessions
            .get_interrupted_state(session_id)
            .await?
            .ok_or(AgentError::Session(SessionMemoryError::NotFound(
                session_id,
            )))?;
        let state: InterruptedState = serde_json::from_str(&state_json)?;

        // Validate the caller is resolving the correct approval.
        if state.approval_id_str() != approval_id.to_string() {
            return Err(AgentError::Llm(agentverse::AgentError::Memory(format!(
                "approval_id mismatch: expected {}, got {}",
                state.approval_id_str(),
                approval_id
            ))));
        }

        self.sessions
            .set_interrupted_state(session_id, None)
            .await?;
        self.sessions
            .update_status(session_id, agentverse_session::SessionStatus::Active)
            .await?;

        agentverse::metrics::record_hitl_transition(agentverse::metrics::HitlTransition::Resumed);

        match state {
            InterruptedState::PendingPhaseGate {
                transition_json, ..
            } => {
                self.resume_phase_gate(session_id, transition_json, decision)
                    .await
            }
            other => {
                self.resume_tool_call_or_checkpoint(user_id, session_id, other, decision)
                    .await
            }
        }
    }

    async fn resume_phase_gate(
        &self,
        session_id: SessionId,
        transition_json: String,
        decision: ApprovalDecision,
    ) -> Result<AgentOutput, AgentError> {
        match decision {
            ApprovalDecision::Approved | ApprovalDecision::Modified { .. } => {
                let transition: PhaseTransition = serde_json::from_str(&transition_json)?;
                let skills = self.skills.as_ref().ok_or_else(|| {
                    AgentError::Skill(SkillError::NotConfigured("no skills configured".into()))
                })?;
                let new_ctx = {
                    let reg = skills.registry.read().await;
                    reg.compile_context(&transition.next_skill)
                        .map_err(AgentError::Skill)?
                };
                let new_ctx_json = serde_json::to_string(&new_ctx)?;
                let phase_ctx_str = format!("Context from previous phase: {}", transition.summary);
                self.sessions
                    .apply_phase_transition(session_id, &new_ctx_json, &phase_ctx_str)
                    .await?;
                Ok(AgentOutput::Done(format!(
                    "Phase transition to {} approved.",
                    transition.next_skill
                )))
            }
            ApprovalDecision::Rejected { reason } => Ok(AgentOutput::Done(format!(
                "Phase transition rejected: {reason}"
            ))),
        }
    }

    async fn resume_tool_call_or_checkpoint(
        &self,
        user_id: &str,
        session_id: SessionId,
        state: InterruptedState,
        decision: ApprovalDecision,
    ) -> Result<AgentOutput, AgentError> {
        let (history_json, pending_calls_json, active_tool_names, skill_ctx_json) = match state {
            InterruptedState::PendingToolCall {
                history_json,
                pending_calls_json,
                active_tool_names,
                skill_context_json,
                ..
            } => (
                history_json,
                pending_calls_json,
                active_tool_names,
                skill_context_json,
            ),
            InterruptedState::PendingCheckpoint {
                history_json,
                active_tool_names,
                skill_context_json,
                ..
            } => (
                history_json,
                String::from("[]"),
                active_tool_names,
                skill_context_json,
            ),
            InterruptedState::PendingPhaseGate { .. } => unreachable!(),
        };

        let history: Vec<agentverse::memory::Message> = serde_json::from_str(&history_json)?;
        let pending: Vec<agentverse::ToolCall> = serde_json::from_str(&pending_calls_json)?;
        let execution_tools = if self.strategy_router.is_some() {
            self.tools.restricted_to_names(&active_tool_names)
        } else {
            std::sync::Arc::clone(&self.tools)
        };

        let observation = match &decision {
            ApprovalDecision::Approved => {
                if pending.is_empty() {
                    "Checkpoint approved. Continue.".to_string()
                } else {
                    // Execute directly: these calls already went through HITL and were
                    // approved. Re-running them through execute_many_hitl would re-check
                    // them against the same policy and immediately re-intercept them
                    // (the hook has no notion of "already approved"), turning every
                    // approval into an infinite interrupt loop. Modified/Rejected below
                    // execute directly for the same reason.
                    let results = execution_tools.execute_many(pending).await;
                    results
                        .iter()
                        .map(|r| {
                            let v = match &r.result {
                                Ok(v) => v.to_string(),
                                Err(e) => format!("Error: {e}"),
                            };
                            format!("Tool: {}\nResult: {}", r.name, v)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                }
            }
            ApprovalDecision::Modified { new_args } => {
                if let Some(first) = pending.first() {
                    let modified = agentverse::ToolCall {
                        name: first.name.clone(),
                        args: new_args.clone(),
                    };
                    let results = execution_tools.execute_many(vec![modified]).await;
                    results
                        .iter()
                        .map(|r| {
                            let v = match &r.result {
                                Ok(v) => v.to_string(),
                                Err(e) => format!("Error: {e}"),
                            };
                            format!("Tool: {}\nResult: {}", r.name, v)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                } else {
                    "No tool calls to modify.".to_string()
                }
            }
            ApprovalDecision::Rejected { reason } => {
                if pending.is_empty() {
                    format!("Checkpoint rejected: {reason}. Revise and resubmit.")
                } else {
                    pending
                        .iter()
                        .map(|c| {
                            format!("Tool: {}\nResult: Rejected by approver: {}", c.name, reason)
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n")
                }
            }
        };

        let mut augmented = history;
        augmented.push(agentverse::memory::Message {
            role: agentverse::memory::MessageRole::User,
            content: observation,
        });

        let skill_ctx: Option<SkillContext> = skill_ctx_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok());
        let strategy = self.strategy_for_resume(&active_tool_names);

        let run_result = if let Some(ref hitl_cfg) = self.hitl {
            let hook = std::sync::Arc::new(HitlContext::new(
                session_id,
                skill_ctx.as_ref().map(|c| c.skill_id.clone()),
                hitl_cfg.policy.clone(),
                std::sync::Arc::clone(&hitl_cfg.queue),
            ));
            strategy.run_hitl(augmented, &active_tool_names, hook).await
        } else {
            strategy
                .run_with_active_tools(augmented, &active_tool_names)
                .await
        };

        match run_result {
            Ok(agentverse::StrategyOutcome::Done(text)) => Ok(AgentOutput::Done(text)),
            Ok(agentverse::StrategyOutcome::Interrupted(interrupt)) => {
                self.handle_tool_interrupt(user_id, session_id, interrupt, &skill_ctx)
                    .await
            }
            Err(e) => Err(AgentError::Llm(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

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
        let session_memory = Arc::new(
            agentverse_session::SqliteSessionMemory::new("sqlite::memory:")
                .await
                .unwrap(),
        );
        Agent::builder(runner, tools, prompts, session_memory, strategy).build()
    }

    // Directly exercises handle_tool_interrupt with a hand-constructed
    // HitlInterrupt (no strategy round-trip) — this is what the design spec
    // calls "resume.rs tests assert direct enum consumption": the function
    // must consume the typed enum's fields as-is, not via a serialized wire
    // format.
    #[tokio::test]
    async fn handle_tool_interrupt_persists_state_and_returns_interrupted_output() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();

        let approval_id = Uuid::new_v4();
        let interrupt = agentverse::hitl::HitlInterrupt {
            approval_id,
            kind_json: serde_json::json!({
                "ToolApproval": {"tool_name": "echo", "args": {"text": "hi"}}
            })
            .to_string(),
            history: vec![agentverse::memory::Message {
                role: agentverse::memory::MessageRole::User,
                content: "please call echo".to_string(),
            }],
            pending_calls: vec![agentverse::ToolCall {
                name: "echo".to_string(),
                args: serde_json::json!({"text": "hi"}),
            }],
            active_tool_names: vec!["echo".to_string()],
        };

        let output = agent
            .handle_tool_interrupt("alice", session_id, interrupt, &None)
            .await
            .unwrap();

        match output {
            AgentOutput::Interrupted {
                approval_id: got_id,
                kind,
            } => {
                assert_eq!(got_id, approval_id);
                assert!(matches!(kind, InterruptKind::ToolApproval { .. }));
            }
            AgentOutput::Done(text) => panic!("expected Interrupted, got Done({text})"),
        }

        let state_json = agent
            .sessions
            .get_interrupted_state(session_id)
            .await
            .unwrap()
            .expect("interrupted state must be persisted");
        let state: InterruptedState = serde_json::from_str(&state_json).unwrap();
        match state {
            InterruptedState::PendingToolCall {
                approval_id: stored_id,
                pending_calls_json,
                history_json,
                ..
            } => {
                assert_eq!(stored_id, approval_id.to_string());
                let pending: Vec<agentverse::ToolCall> =
                    serde_json::from_str(&pending_calls_json).unwrap();
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].name, "echo");
                let history: Vec<agentverse::memory::Message> =
                    serde_json::from_str(&history_json).unwrap();
                assert_eq!(history.len(), 1);
                assert_eq!(history[0].content, "please call echo");
            }
            other => panic!("expected PendingToolCall, got {other:?}"),
        }
    }
}
