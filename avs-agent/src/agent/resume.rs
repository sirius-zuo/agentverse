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
        hitl_msg: &str,
        active_tool_names: &[String],
        skill_ctx: &Option<SkillContext>,
    ) -> Result<AgentOutput, AgentError> {
        // Wire format: "HITL:{uuid}:{kind_b64}:{history_b64}:{calls_b64}" — see agentverse::hitl::HitlWire
        let wire = agentverse::hitl::HitlWire::parse(hitl_msg).map_err(|e| {
            AgentError::Llm(agentverse::AgentError::Memory(format!(
                "malformed HITL wire message: {e}"
            )))
        })?;
        let approval_id: Uuid = wire.approval_id;
        let kind_json = wire.kind_json;
        let history_json = wire.history_json;
        let pending_calls_json = wire.pending_calls_json;

        // Determine variant from kind_json
        let kind: InterruptKind = serde_json::from_str(&kind_json).map_err(|e| {
            AgentError::Llm(agentverse::AgentError::Memory(format!(
                "bad HITL kind_json: {e}"
            )))
        })?;

        let skill_ctx_json = skill_ctx
            .as_ref()
            .and_then(|c| serde_json::to_string(c).ok());

        let state = match kind {
            InterruptKind::SkillCheckpoint { .. } => InterruptedState::PendingCheckpoint {
                approval_id: approval_id.to_string(),
                kind_json,
                history_json,
                active_tool_names: active_tool_names.to_vec(),
                skill_context_json: skill_ctx_json,
            },
            _ => InterruptedState::PendingToolCall {
                approval_id: approval_id.to_string(),
                kind_json,
                history_json,
                pending_calls_json,
                active_tool_names: active_tool_names.to_vec(),
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

        let observation = match &decision {
            ApprovalDecision::Approved => {
                if pending.is_empty() {
                    "Checkpoint approved. Continue.".to_string()
                } else {
                    // Execute approved calls using execute_many_hitl so any further
                    // dangerous calls in the batch are re-checked.
                    if let Some(ref hitl_cfg) = self.hitl {
                        let skill_ctx: Option<SkillContext> = skill_ctx_json
                            .as_deref()
                            .and_then(|j| serde_json::from_str(j).ok());
                        let hook = std::sync::Arc::new(HitlContext::new(
                            session_id,
                            skill_ctx.as_ref().map(|c| c.skill_id.clone()),
                            hitl_cfg.policy.clone(),
                            std::sync::Arc::clone(&hitl_cfg.queue),
                        ));
                        let hook_arc: std::sync::Arc<dyn agentverse::hitl::HitlHook> = hook;
                        match self
                            .tools
                            .execute_many_hitl(pending.clone(), &hook_arc)
                            .await
                        {
                            Ok(results) => results
                                .iter()
                                .map(|r| {
                                    let v = match &r.result {
                                        Ok(v) => v.to_string(),
                                        Err(e) => format!("Error: {e}"),
                                    };
                                    format!("Tool: {}\nResult: {}", r.name, v)
                                })
                                .collect::<Vec<_>>()
                                .join("\n\n"),
                            Err(agentverse_tools::HitlInterruptResult {
                                approval_id,
                                kind_json,
                            }) => {
                                // Another call in the batch needs approval.
                                let msg = agentverse::hitl::HitlWire {
                                    approval_id,
                                    kind_json,
                                    history_json: serde_json::to_string(&history)
                                        .unwrap_or_default(),
                                    pending_calls_json: serde_json::to_string(&pending)
                                        .unwrap_or_default(),
                                }
                                .encode();
                                let active_tool_names_vec: Vec<String> = active_tool_names;
                                let skill_ctx_val: Option<SkillContext> = skill_ctx_json
                                    .as_deref()
                                    .and_then(|j| serde_json::from_str(j).ok());
                                return self
                                    .handle_tool_interrupt(
                                        user_id,
                                        session_id,
                                        &msg,
                                        &active_tool_names_vec,
                                        &skill_ctx_val,
                                    )
                                    .await;
                            }
                        }
                    } else {
                        let results = self.tools.execute_many(pending).await;
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
            }
            ApprovalDecision::Modified { new_args } => {
                if let Some(first) = pending.first() {
                    let modified = agentverse::ToolCall {
                        name: first.name.clone(),
                        args: new_args.clone(),
                    };
                    let results = self.tools.execute_many(vec![modified]).await;
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

        let run_result = if let Some(ref hitl_cfg) = self.hitl {
            let hook = std::sync::Arc::new(HitlContext::new(
                session_id,
                skill_ctx.as_ref().map(|c| c.skill_id.clone()),
                hitl_cfg.policy.clone(),
                std::sync::Arc::clone(&hitl_cfg.queue),
            ));
            self.strategy
                .run_hitl(augmented, &active_tool_names, hook)
                .await
        } else {
            self.strategy
                .run_with_active_tools(augmented, &active_tool_names)
                .await
        };

        match run_result {
            Ok(text) => Ok(AgentOutput::Done(text)),
            Err(agentverse::AgentError::Memory(ref msg))
                if agentverse::hitl::HitlWire::is_wire(msg) =>
            {
                self.handle_tool_interrupt(user_id, session_id, msg, &active_tool_names, &skill_ctx)
                    .await
            }
            Err(e) => Err(AgentError::Llm(e)),
        }
    }
}
