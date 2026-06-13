use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use agentverse::memory::{LongtermMemory, LongtermRecord, Message, MessageRole};
use agentverse::{LlmRunner, PromptRegistry, RunStrategy};
use agentverse_hitl::{ApprovalDecision, HitlContext, InterruptKind};
use agentverse_session::{
    InterruptedState, Session, SessionId, SessionManager, SessionMemory, SessionMemoryError,
};
use agentverse_skill::{RouteSkills, SkillConfig, SkillContext, SkillError, SkillRegistry, SkillRouter};
use agentverse_tools::ToolRegistry;
use serde::{Deserialize, Serialize};
use serde_json;
use tokio::sync::Mutex;
use uuid::Uuid;

struct CacheMemory {
    messages: Vec<Message>,
    last_used: Instant,
}


/// Result of a skill phase transition parsed from an agent's output.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct PhaseTransition {
    pub next_skill: String,
    pub summary: String,
    pub deliverable: String,
}

/// Output of a single agent invocation.
/// The output of an agent run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOutput {
    /// The agent completed successfully with this string response.
    Done(String),
    /// The agent was interrupted by HITL.
    /// The `kind` field matches what was submitted to the ApprovalQueue, so callers
    /// can display which interrupt type fired without re-querying the queue.
    Interrupted {
        approval_id: Uuid,
        kind: InterruptKind,
    },
}

impl std::fmt::Display for AgentOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentOutput::Done(text) => write!(f, "{text}"),
            AgentOutput::Interrupted { approval_id, kind } => {
                write!(
                    f,
                    "HITL interruption: approval_id={approval_id}, kind={kind:?}"
                )
            }
        }
    }
}

/// HITL configuration attached to an agent at construction time.
pub struct HitlConfig {
    pub policy: agentverse_hitl::HitlPolicy,
    pub queue: std::sync::Arc<dyn agentverse_hitl::ApprovalQueue>,
}

/// Result of advance_phase when HITL is configured.
pub enum PhaseAdvanceResult {
    /// Transition was applied immediately (no phase gate for this skill).
    Advanced(PhaseTransition),
    /// Transition is pending human approval; approval_id matches the queue entry.
    Pending { approval_id: Uuid },
}

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("session error: {0}")]
    Session(#[from] SessionMemoryError),
    #[error("llm error: {0}")]
    Llm(#[from] agentverse::AgentError),
    #[error("skill error: {0}")]
    Skill(#[from] SkillError),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct Agent {
    // Held for ownership and future use (e.g. HTTP server, direct tool calls)
    #[allow(dead_code)]
    runner: Arc<LlmRunner>,
    #[allow(dead_code)]
    tools: Arc<ToolRegistry>,
    prompts: Arc<PromptRegistry>,
    sessions: Arc<SessionManager>,
    strategy: Arc<dyn RunStrategy>,
    cache_memory: Mutex<HashMap<(String, SessionId), CacheMemory>>,
    buffer_ttl: Duration,
    longterm_memory: Option<Arc<dyn LongtermMemory>>,
    skills: Option<SkillConfig>,
    hitl: Option<HitlConfig>,
}

/// Parse `NEXT_SKILL: <id>` and `SUMMARY: <text>` from the last non-empty lines of output.
/// Both directives must be present (in order) for a transition to be returned.
/// Returns `None` if either directive is missing.
pub fn parse_phase_transition(output: &str) -> Option<PhaseTransition> {
    let trimmed = output.trim_end();
    let mut lines: Vec<&str> = trimmed.lines().collect();

    // Strip trailing blank lines
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }

    if lines.len() < 2 {
        return None;
    }

    let last = lines.last()?.trim();
    let summary = last.strip_prefix("SUMMARY:")?.trim().to_string();
    if summary.is_empty() {
        return None;
    }
    lines.pop();

    // Strip blank lines between NEXT_SKILL and SUMMARY
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }

    let next_line = lines.last()?.trim();
    let next_skill = next_line.strip_prefix("NEXT_SKILL:")?.trim().to_string();
    if next_skill.is_empty() {
        return None;
    }
    lines.pop();

    // Strip trailing blank lines from deliverable body
    while lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.pop();
    }

    let deliverable = lines.join("\n");
    if deliverable.trim().is_empty() {
        tracing::warn!(
            "parse_phase_transition: NEXT_SKILL/SUMMARY directives found but deliverable \
            body is empty; treating as terminal output"
        );
        return None;
    }

    Some(PhaseTransition {
        next_skill,
        summary,
        deliverable,
    })
}

impl Agent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        runner: Arc<LlmRunner>,
        tools: Arc<ToolRegistry>,
        prompts: Arc<PromptRegistry>,
        session_memory: Arc<dyn SessionMemory>,
        strategy: Arc<dyn RunStrategy>,
        enable_http_server: bool,
        longterm_memory: Option<Arc<dyn LongtermMemory>>,
        skills: Option<SkillConfig>,
        hitl: Option<HitlConfig>,
    ) -> Arc<Self> {
        let sm_for_workers = Arc::clone(&session_memory);
        let agent = Arc::new(Self {
            runner,
            tools,
            prompts,
            sessions: Arc::new(SessionManager::new(session_memory)),
            strategy,
            cache_memory: Mutex::new(HashMap::new()),
            buffer_ttl: Duration::from_secs(300),
            longterm_memory,
            skills,
            hitl,
        });

        // Auto-spawn CleanupWorker — purges stale messages from consolidated sessions
        tokio::spawn(
            crate::workers::CleanupWorker::new(
                Arc::clone(&sm_for_workers),
                crate::workers::CleanupConfig::default(),
            )
            .run(),
        );

        // Spawn HitlSweepWorker when HITL is configured
        if let Some(ref hitl_cfg) = agent.hitl {
            tokio::spawn(
                crate::workers::HitlSweepWorker::new(
                    Arc::clone(&hitl_cfg.queue),
                    crate::workers::HitlSweepConfig::default(),
                )
                .run(),
            );
        }

        // Spawn ConsolidationWorker when longterm memory is configured
        if let Some(ref ltm) = agent.longterm_memory {
            tokio::spawn(
                crate::workers::ConsolidationWorker::new(
                    Arc::clone(&sm_for_workers),
                    Arc::clone(ltm),
                    crate::workers::ConsolidationConfig::default(),
                )
                .run(),
            );
        }

        #[cfg(feature = "http")]
        if enable_http_server {
            crate::http::spawn_server(Arc::clone(&agent));
        }

        // Suppress unused variable warning when http feature is disabled
        let _ = enable_http_server;
        agent
    }

    fn assemble_system(
        &self,
        skill_ctx: Option<&SkillContext>,
        summaries_block: Option<&str>,
    ) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();

        match skill_ctx {
            Some(ctx) => {
                // Skill active: full instructions + supporting documents
                parts.push(ctx.instructions.clone());
                parts.extend(ctx.documents.iter().cloned());
            }
            None => {
                // Discovery phase: skill summaries (if any)
                if let Some(block) = summaries_block {
                    if !block.is_empty() {
                        parts.push(block.to_string());
                    }
                }
            }
        }

        // Agent base system prompt from system.j2 template
        if let Ok(base) = self
            .prompts
            .render("system", std::collections::HashMap::new())
        {
            if !base.trim().is_empty() {
                parts.push(base);
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n\n"))
        }
    }

    fn assemble_messages(
        &self,
        system: Option<String>,
        history: Vec<Message>,
        input: &str,
    ) -> Vec<Message> {
        let mut msgs = Vec::new();
        if let Some(sys) = system {
            msgs.push(Message {
                role: MessageRole::System,
                content: sys,
            });
        }
        msgs.extend(history);
        msgs.push(Message {
            role: MessageRole::User,
            content: input.to_string(),
        });
        msgs
    }

    fn assemble_messages_with_context(
        &self,
        system: Option<String>,
        long_term_text: Option<String>,
        history: Vec<Message>,
        input: &str,
    ) -> Vec<Message> {
        let mut msgs = Vec::new();
        let sys_content = match (system, long_term_text) {
            (Some(sys), Some(lt)) => Some(format!("{sys}\n\n{lt}")),
            (Some(sys), None) => Some(sys),
            (None, Some(lt)) => Some(lt),
            (None, None) => None,
        };
        if let Some(content) = sys_content {
            msgs.push(Message {
                role: MessageRole::System,
                content,
            });
        }
        msgs.extend(history);
        msgs.push(Message {
            role: MessageRole::User,
            content: input.to_string(),
        });
        msgs
    }

    async fn get_cache_memory(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Vec<Message>, AgentError> {
        let key = (user_id.to_string(), session_id);
        {
            let cache = self.cache_memory.lock().await;
            if let Some(buf) = cache.get(&key) {
                if buf.last_used.elapsed() <= self.buffer_ttl {
                    return Ok(buf.messages.clone());
                }
            }
        }
        // Miss or TTL expired: sweep expired entries, then rehydrate from Layer 2
        let history = self.sessions.load_messages(session_id).await?;
        let mut cache = self.cache_memory.lock().await;
        let ttl = self.buffer_ttl;
        cache.retain(|_, buf| buf.last_used.elapsed() <= ttl);
        cache.insert(
            key,
            CacheMemory {
                messages: history.clone(),
                last_used: Instant::now(),
            },
        );
        Ok(history)
    }

    async fn update_cache_memory(
        &self,
        user_id: &str,
        session_id: SessionId,
        user_msg: Message,
        assistant_msg: Message,
    ) {
        let key = (user_id.to_string(), session_id);
        let mut cache = self.cache_memory.lock().await;
        if let Some(buf) = cache.get_mut(&key) {
            buf.messages.push(user_msg);
            buf.messages.push(assistant_msg);
            buf.last_used = Instant::now();
        } else {
            // Key was TTL-evicted during the LLM call; insert a minimal buffer
            // with just this turn so the next invoke avoids a cold DB read.
            cache.insert(
                key,
                CacheMemory {
                    messages: vec![user_msg, assistant_msg],
                    last_used: Instant::now(),
                },
            );
        }
    }

    /// Single-turn stateless invocation with no session, history, or skill context.
    /// Skill sessions (created via `create_session_with_skill`) must use the
    /// session-aware `invoke` path, not this method.
    pub async fn invoke_stateless(&self, input: &str) -> Result<String, AgentError> {
        // Stateless: no session, no memory context — always a fresh single-turn call.
        let messages = self.assemble_messages(self.assemble_system(None, None), vec![], input);
        let response = self.strategy.run(messages).await?;
        Ok(response)
    }

    pub async fn invoke(
        &self,
        user_id: &str,
        session_id: SessionId,
        input: &str,
    ) -> Result<AgentOutput, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;

        // Check for a phase opening context set by advance_phase.
        // If present: clear stale history from cache, inject context as the sole prior context,
        // and clear the stored context so subsequent invokes accumulate normally.
        let phase_ctx = if self.skills.is_some() {
            self.sessions.get_phase_opening_context(session_id).await?
        } else {
            None
        };

        let (history, effective_input) = if let Some(ctx_str) = phase_ctx {
            // Phase transition: clear stale cache from the previous phase.
            let cache_key = (user_id.to_string(), session_id);
            self.cache_memory.lock().await.remove(&cache_key);

            // Clear the stored context — subsequent invokes in this phase accumulate normally.
            self.sessions
                .set_phase_opening_context(session_id, None)
                .await?;

            // ctx_str is the summary only; input is the deliverable passed by the caller.
            // Combine into one coherent user message: no duplication, no separator needed.
            let combined = format!("{ctx_str}\n\n{input}");
            (vec![], combined)
        } else {
            let history = self.get_cache_memory(user_id, session_id).await?;
            (history, input.to_string())
        };

        // Resolve skill context. On first invoke with no context, attempt routing.
        // Each read lock is scoped to a single synchronous operation and released
        // before any .await so that reload_skills write-lock is never blocked by I/O.
        let (skill_ctx, summaries_block): (Option<SkillContext>, Option<String>) = {
            let existing = self.sessions.get_skill_context(session_id).await?;

            if let Some(json) = existing {
                // Already bound — deserialize and use as-is.
                let ctx = serde_json::from_str::<SkillContext>(&json)?;
                (Some(ctx), None)
            } else if let Some(ref skills) = self.skills {
                let router = match skills.routing_threshold {
                    Some(t) => SkillRouter::with_threshold(t),
                    None => SkillRouter::for_mode(&skills.mode),
                };

                // Lock scope 1: route only. Released before any await.
                let routed_id: Option<String> = {
                    let reg = skills.registry.read().await;
                    let candidates = reg.eligible(&skills.mode);
                    router.route(input, &candidates)
                    // candidates drops, then reg drops here
                };

                if let Some(skill_id) = routed_id {
                    tracing::debug!(
                        skill_id = %skill_id,
                        session_id = %session_id,
                        "skill activated via automatic routing"
                    );
                    // Lock scope 2: compile context only. Released before set_skill_context await.
                    let ctx = {
                        let reg = skills.registry.read().await;
                        reg.compile_context(&skill_id).map_err(AgentError::Skill)?
                        // reg drops here
                    };
                    let json = serde_json::to_string(&ctx)?;
                    self.sessions
                        .set_skill_context(session_id, Some(&json))
                        .await?;
                    (Some(ctx), None)
                } else {
                    tracing::debug!(session_id = %session_id, "no skill matched, running base agent");
                    let text = skills.summaries();
                    (None, if text.is_empty() { None } else { Some(text) })
                }
            } else {
                (None, None)
            }
        };

        // Active tool names: skill tools ∩ registry, or all if no skill.
        // A skill with tools:[] restricts to zero tools; only None (no skill) means all tools.
        let active_tool_names: Vec<String> = match &skill_ctx {
            None => self.tools.tool_names(),
            Some(ctx) => ctx
                .tools
                .iter()
                .filter(|name| self.tools.has_tool(name))
                .cloned()
                .collect(),
        };

        // Layer 3: retrieve scored memories
        let long_term_text = if let Some(ref ms) = self.longterm_memory {
            let memories = ms
                .retrieve(user_id, &effective_input, 5)
                .await
                .unwrap_or_else(|e| {
                    tracing::warn!(error = %e, "layer-3 memory retrieve failed, proceeding without context");
                    vec![]
                });
            if memories.is_empty() {
                None
            } else {
                Some(
                    memories
                        .into_iter()
                        .map(|sm| format!("[Memory] {}", sm.content))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
        } else {
            None
        };

        let user_msg = Message {
            role: MessageRole::User,
            content: effective_input.clone(),
        };
        let messages = self.assemble_messages_with_context(
            self.assemble_system(skill_ctx.as_ref(), summaries_block.as_deref()),
            long_term_text,
            history,
            &effective_input,
        );
        // Extract active skill_id for HitlContext per-skill gate lookup.
        let active_skill_id = skill_ctx.as_ref().map(|ctx| ctx.skill_id.clone());

        let run_result = if let Some(ref hitl_cfg) = self.hitl {
            let hook = std::sync::Arc::new(HitlContext::new(
                session_id,
                active_skill_id,
                hitl_cfg.policy.clone(),
                std::sync::Arc::clone(&hitl_cfg.queue),
            ));
            self.strategy
                .run_hitl(messages, &active_tool_names, hook)
                .await
        } else {
            self.strategy
                .run_with_active_tools(messages, &active_tool_names)
                .await
        };

        let response = match run_result {
            Ok(text) => text,
            Err(agentverse::AgentError::Memory(ref msg)) if msg.starts_with("HITL:") => {
                return self
                    .handle_tool_interrupt(user_id, session_id, msg, &active_tool_names, &skill_ctx)
                    .await;
            }
            Err(e) => return Err(AgentError::Llm(e)),
        };

        let assistant_msg = Message {
            role: MessageRole::Assistant,
            content: response.clone(),
        };
        self.sessions
            .append_turn(session_id, user_msg.clone(), assistant_msg.clone())
            .await?;
        self.update_cache_memory(user_id, session_id, user_msg, assistant_msg)
            .await;

        if let Some(ms) = self.longterm_memory.clone() {
            let uid = user_id.to_string();
            let record = LongtermRecord::now(
                format!("User: {effective_input}\nAssistant: {response}"),
                0.5,
            );
            tokio::spawn(async move {
                let _ = ms.write(&uid, record).await;
            });
        }

        Ok(AgentOutput::Done(response))
    }

    pub async fn create_session(&self, user_id: &str) -> Result<SessionId, AgentError> {
        Ok(self.sessions.create_session(user_id).await?)
    }

    pub async fn create_session_with_skill(
        &self,
        user_id: &str,
        skill_id: &str,
    ) -> Result<SessionId, AgentError> {
        let skills = self.skills.as_ref().ok_or_else(|| {
            SkillError::NotConfigured("no skill registry configured on this agent".into())
        })?;
        let ctx = skills.registry.read().await.compile_context(skill_id)?;
        let ctx_json = serde_json::to_string(&ctx)?;
        Ok(self
            .sessions
            .create_session_with_skill_context(user_id, &ctx_json)
            .await?)
    }

    /// Parse a skill transition from `output`. If `NEXT_SKILL:` + `SUMMARY:` are found:
    /// rebinds the active skill on the session and stores the phase opening context.
    /// Returns `None` if the output contains no transition directives (terminal skill).
    pub async fn advance_phase(
        &self,
        user_id: &str,
        session_id: SessionId,
        output: &str,
    ) -> Result<Option<PhaseAdvanceResult>, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;

        let transition = match parse_phase_transition(output) {
            Some(t) => t,
            None => {
                // Warn if NEXT_SKILL: appears near the end but SUMMARY: is absent —
                // this likely means the skill emitted only one of the two required directives.
                let near_end_has_next_skill = output
                    .trim_end()
                    .lines()
                    .rev()
                    .take(5)
                    .filter(|l| !l.trim().is_empty())
                    .any(|l| l.trim().starts_with("NEXT_SKILL:"));
                let has_summary = output.lines().any(|l| l.trim().starts_with("SUMMARY:"));
                if near_end_has_next_skill && !has_summary {
                    tracing::warn!(
                        session_id = %session_id,
                        "advance_phase: NEXT_SKILL: directive found near end of output but \
                        SUMMARY: is missing — treating as terminal output. Ensure the skill \
                        emits both directives as its last two lines."
                    );
                }
                return Ok(None);
            }
        };

        // Phase gate check — runs before applying the transition.
        if let Some(ref hitl_cfg) = self.hitl {
            // skill_id is stored directly on SkillContext (added in Task 5).
            let current_skill_id = self
                .sessions
                .get_skill_context(session_id)
                .await?
                .and_then(|j| serde_json::from_str::<agentverse_skill::SkillContext>(&j).ok())
                .map(|ctx| ctx.skill_id);

            if let Some(ref skill_id) = current_skill_id {
                if hitl_cfg.policy.requires_phase_gate(skill_id) {
                    let kind = agentverse_hitl::InterruptKind::PhaseGate {
                        from_skill: skill_id.clone(),
                        to_skill: transition.next_skill.clone(),
                        deliverable: transition.deliverable.clone(),
                    };
                    let req = agentverse_hitl::ApprovalRequest::new(session_id, kind);
                    let approval_id = hitl_cfg.queue.submit(req).await.map_err(|e| {
                        AgentError::Session(agentverse_session::SessionMemoryError::Database(
                            e.to_string(),
                        ))
                    })?;

                    let state = InterruptedState::PendingPhaseGate {
                        approval_id: approval_id.to_string(),
                        transition_json: serde_json::to_string(&transition)?,
                    };
                    self.sessions
                        .set_interrupted_state(session_id, Some(&serde_json::to_string(&state)?))
                        .await?;
                    self.sessions
                        .update_status(session_id, agentverse_session::SessionStatus::Interrupted)
                        .await?;

                    return Ok(Some(PhaseAdvanceResult::Pending { approval_id }));
                }
            }
        }

        // No gate (or gate not required) — apply the transition immediately.
        let skills = self.skills.as_ref().ok_or_else(|| {
            SkillError::NotConfigured("no skill registry configured on this agent".into())
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

        Ok(Some(PhaseAdvanceResult::Advanced(transition)))
    }

    /// Reload the skill registry from disk. Existing sessions are unaffected;
    /// new routing calls pick up the refreshed registry.
    pub async fn reload_skills(&self) -> Result<(), AgentError> {
        let skills = self.skills.as_ref().ok_or_else(|| {
            AgentError::Skill(SkillError::NotConfigured(
                "no skills configured on this agent".into(),
            ))
        })?;
        // SkillRegistry::load does blocking filesystem I/O — run it off the Tokio executor.
        let dir = skills.dir.clone();
        let new_registry = tokio::task::spawn_blocking(move || SkillRegistry::load(&dir))
            .await
            .unwrap_or_else(|e| Err(SkillError::Io(std::io::Error::other(e))))
            .map_err(AgentError::Skill)?;
        // Rebuild summaries and ids while we still have owned access to new_registry.
        skills.rebuild_caches(&new_registry);
        *skills.registry.write().await = new_registry;
        tracing::info!(dir = ?skills.dir, "skill registry reloaded");
        Ok(())
    }

    fn hitl_b64_decode(s: &str) -> String {
        use base64::{engine::general_purpose::STANDARD, Engine};
        STANDARD
            .decode(s)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_default()
    }

    async fn handle_tool_interrupt(
        &self,
        _user_id: &str,
        session_id: SessionId,
        hitl_msg: &str,
        active_tool_names: &[String],
        skill_ctx: &Option<agentverse_skill::SkillContext>,
    ) -> Result<AgentOutput, AgentError> {
        // Format: "HITL:{uuid}:{kind_b64}:{history_b64}:{calls_b64}"
        let parts: Vec<&str> = hitl_msg.splitn(5, ':').collect();
        if parts.len() < 5 {
            return Err(AgentError::Llm(agentverse::AgentError::Memory(
                "malformed HITL message".into(),
            )));
        }
        let approval_id: Uuid = parts[1].parse().map_err(|_| {
            AgentError::Llm(agentverse::AgentError::Memory(
                "bad approval_id in HITL msg".into(),
            ))
        })?;
        let kind_json = Self::hitl_b64_decode(parts[2]);
        let history_json = Self::hitl_b64_decode(parts[3]);
        let pending_calls_json = Self::hitl_b64_decode(parts[4]);

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

        match state {
            InterruptedState::PendingPhaseGate {
                transition_json, ..
            } => match decision {
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
                    let phase_ctx_str =
                        format!("Context from previous phase: {}", transition.summary);
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
            },

            other => {
                let (history_json, pending_calls_json, active_tool_names, skill_ctx_json) =
                    match other {
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

                let history: Vec<agentverse::memory::Message> =
                    serde_json::from_str(&history_json)?;
                let pending: Vec<agentverse::ToolCall> = serde_json::from_str(&pending_calls_json)?;

                let observation = match &decision {
                    ApprovalDecision::Approved => {
                        if pending.is_empty() {
                            "Checkpoint approved. Continue.".to_string()
                        } else {
                            // Execute approved calls using execute_many_hitl so any further
                            // dangerous calls in the batch are re-checked.
                            if let Some(ref hitl_cfg) = self.hitl {
                                let skill_ctx: Option<agentverse_skill::SkillContext> =
                                    skill_ctx_json
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
                                        let pending_json =
                                            serde_json::to_string(&pending).unwrap_or_default();
                                        let history_json_enc =
                                            serde_json::to_string(&history).unwrap_or_default();
                                        use base64::{engine::general_purpose::STANDARD, Engine};
                                        let msg = format!(
                                            "HITL:{}:{}:{}:{}",
                                            approval_id,
                                            STANDARD.encode(kind_json.as_bytes()),
                                            STANDARD.encode(history_json_enc.as_bytes()),
                                            STANDARD.encode(pending_json.as_bytes()),
                                        );
                                        let active_tool_names_vec: Vec<String> = active_tool_names;
                                        let skill_ctx_val: Option<agentverse_skill::SkillContext> =
                                            skill_ctx_json
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
                                    format!(
                                        "Tool: {}\nResult: Rejected by approver: {}",
                                        c.name, reason
                                    )
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

                let skill_ctx: Option<agentverse_skill::SkillContext> = skill_ctx_json
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
                    Err(agentverse::AgentError::Memory(ref msg)) if msg.starts_with("HITL:") => {
                        self.handle_tool_interrupt(
                            user_id,
                            session_id,
                            msg,
                            &active_tool_names,
                            &skill_ctx,
                        )
                        .await
                    }
                    Err(e) => Err(AgentError::Llm(e)),
                }
            }
        }
    }

    pub async fn get_session(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Option<Session>, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;
        Ok(self.sessions.get_session(session_id).await?)
    }

    pub async fn end_session(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<(), AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;
        self.sessions.end_session(session_id).await?;
        // Layer-1 cascade: evict working buffer immediately on session delete
        let key = (user_id.to_string(), session_id);
        self.cache_memory.lock().await.remove(&key);
        Ok(())
    }

    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>, AgentError> {
        Ok(self.sessions.list_sessions(user_id).await?)
    }

    pub async fn load_messages(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Vec<Message>, AgentError> {
        self.sessions.assert_owner(user_id, session_id).await?;
        Ok(self.sessions.load_messages(session_id).await?)
    }
}

#[cfg(test)]
mod lt_tests {
    use super::*;
    use agentverse::memory::{LongtermMemory, LongtermRecord, MemoryError, ScoredMemory};
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    struct NoopMemoryStore;
    #[async_trait::async_trait]
    impl LongtermMemory for NoopMemoryStore {
        async fn write(&self, _: &str, _: LongtermRecord) -> Result<(), MemoryError> {
            Ok(())
        }
        async fn retrieve(
            &self,
            _: &str,
            _: &str,
            _: usize,
        ) -> Result<Vec<ScoredMemory>, MemoryError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn agent_with_memory_store_creates_session_normally() {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::OpenAI {
                    model_name: "test".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
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
        let ms: Arc<dyn LongtermMemory> = Arc::new(NoopMemoryStore);
        let agent = Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            Some(ms),
            None,
            None,
        );
        let sid = agent.create_session("alice").await.unwrap();
        assert!(agent.get_session("alice", sid).await.unwrap().is_some());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    async fn make_agent() -> Arc<Agent> {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::OpenAI {
                    model_name: "test".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
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
        Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            None,
            None,
            None,
        )
    }

    #[tokio::test]
    async fn invoke_stateless_returns_error_on_bad_port() {
        let agent = make_agent().await;
        let result = agent.invoke_stateless("hello").await;
        assert!(result.is_err());
    }

    #[test]
    fn parses_both_directives() {
        let output = "Extracted facts.\n\nNEXT_SKILL: analyzer\nSUMMARY: Found 3 entities; dates span 2020–2023.";
        let t = parse_phase_transition(output).unwrap();
        assert_eq!(t.next_skill, "analyzer");
        assert_eq!(t.summary, "Found 3 entities; dates span 2020–2023.");
        assert_eq!(t.deliverable, "Extracted facts.");
    }

    #[test]
    fn returns_none_when_no_directives() {
        let output = "Final summary. No directives here.";
        assert!(parse_phase_transition(output).is_none());
    }

    #[test]
    fn returns_none_when_only_next_skill() {
        let output = "Some output.\nNEXT_SKILL: analyzer";
        assert!(parse_phase_transition(output).is_none());
    }

    #[test]
    fn returns_none_when_only_summary() {
        let output = "Some output.\nSUMMARY: did something";
        assert!(parse_phase_transition(output).is_none());
    }

    #[test]
    fn handles_trailing_whitespace() {
        let output = "body\nNEXT_SKILL: writer  \nSUMMARY: Done.  \n  ";
        let t = parse_phase_transition(output).unwrap();
        assert_eq!(t.next_skill, "writer");
        assert_eq!(t.summary, "Done.");
    }

    #[test]
    fn strips_directives_from_deliverable() {
        let output = "Line one.\nLine two.\nNEXT_SKILL: b\nSUMMARY: summary text";
        let t = parse_phase_transition(output).unwrap();
        assert_eq!(t.deliverable, "Line one.\nLine two.");
    }

    #[test]
    fn returns_none_when_deliverable_is_empty() {
        // Directives only, no body at all
        assert!(parse_phase_transition("NEXT_SKILL: analyzer\nSUMMARY: done").is_none());
        // Only blank lines before directives
        assert!(parse_phase_transition("\n  \n\nNEXT_SKILL: analyzer\nSUMMARY: done").is_none());
    }

    #[tokio::test]
    async fn create_and_get_session_works() {
        let agent = make_agent().await;
        let session_id = agent.create_session("alice").await.unwrap();
        let session = agent.get_session("alice", session_id).await.unwrap();
        assert!(session.is_some());
        assert_eq!(session.unwrap().user_id, "alice");
    }

    #[tokio::test]
    async fn working_buffer_rehydrates_after_db_write() {
        // Verifies the rehydration path: fresh session → load_messages returns empty
        let agent = make_agent().await;
        let sid = agent.create_session("alice").await.unwrap();
        let msgs = agent.load_messages("alice", sid).await.unwrap();
        assert!(msgs.is_empty());
    }

    #[tokio::test]
    async fn assemble_system_with_active_skill_contains_instructions_and_docs() {
        let agent = make_agent().await;
        let ctx = SkillContext {
            skill_id: "test-skill".into(),
            instructions: "You are an expert reviewer.".into(),
            documents: vec!["## Principles\nBe thorough.".into()],
            tools: vec![],
            max_iterations: None,
        };
        let result = agent.assemble_system(Some(&ctx), None);
        let s = result.unwrap();
        assert!(
            s.contains("You are an expert reviewer."),
            "instructions missing"
        );
        assert!(s.contains("Be thorough."), "document content missing");
    }

    #[tokio::test]
    async fn assemble_system_with_summaries_contains_block() {
        let agent = make_agent().await;
        let block = "## Available Skills\n\n- code-review: Reviews code.";
        let result = agent.assemble_system(None, Some(block));
        assert!(result.unwrap().contains("## Available Skills"));
    }

    #[tokio::test]
    async fn assemble_system_skill_active_excludes_summaries() {
        let agent = make_agent().await;
        let ctx = SkillContext {
            skill_id: "test-skill".into(),
            instructions: "Skill active.".into(),
            documents: vec![],
            tools: vec![],
            max_iterations: None,
        };
        let result =
            agent.assemble_system(Some(&ctx), Some("## Available Skills\n\nshould not appear"));
        let s = result.unwrap();
        assert!(s.contains("Skill active."));
        assert!(
            !s.contains("## Available Skills"),
            "summaries must not appear when skill is active"
        );
    }

}

#[cfg(test)]
mod skill_tests {
    use super::*;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_skill::SkillMode;
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    async fn make_agent_with_skills(skills: Option<SkillConfig>) -> Arc<Agent> {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::OpenAI {
                    model_name: "test".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
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
        Agent::new(
            runner,
            tools,
            prompts,
            session_memory,
            strategy,
            false,
            None,
            skills,
            None,
        )
    }

    fn write_skill(dir: &std::path::Path, subdir: &str, name: &str, instructions: &str) {
        let pkg = dir.join(subdir).join(name);
        fs::create_dir_all(&pkg).unwrap();
        let content = format!(
            "---\nname: {name}\ndescription: Test skill.\nagentverse:\n  tools:\n    - find_tools\n---\n\n{instructions}\n"
        );
        fs::write(pkg.join("SKILL.md"), content).unwrap();
    }

    #[tokio::test]
    async fn create_session_with_skill_stores_context() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "test-skill", "You are a test agent.");
        let skills = SkillConfig::load(dir.path(), SkillMode::Open).ok();
        let agent = make_agent_with_skills(skills).await;

        let session_id = agent
            .create_session_with_skill("alice", "test-skill")
            .await
            .unwrap();

        // The skill context should be stored in the session
        let ctx_json = agent.sessions.get_skill_context(session_id).await.unwrap();
        assert!(ctx_json.is_some());
        let ctx: agentverse_skill::SkillContext = serde_json::from_str(&ctx_json.unwrap()).unwrap();
        assert!(ctx.instructions.contains("You are a test agent."));
        assert!(ctx.tools.contains(&"find_tools".to_string()));
    }

    #[tokio::test]
    async fn create_session_with_skill_returns_error_for_unknown_skill() {
        let dir = tempdir().unwrap();
        let skills = SkillConfig::load(dir.path(), SkillMode::Open).ok();
        let agent = make_agent_with_skills(skills).await;
        let result = agent
            .create_session_with_skill("alice", "nonexistent-skill")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn empty_tools_in_skill_restricts_to_zero_tools() {
        // Write a skill with an empty tools list
        let dir = tempdir().unwrap();
        let pkg = dir.path().join("system").join("no-tools-skill");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(
            pkg.join("SKILL.md"),
            "---\nname: no-tools-skill\ndescription: Restricted.\nagentverse:\n  tools: []\n---\n\nNo tools.\n",
        )
        .unwrap();

        let skills = SkillConfig::load(dir.path(), SkillMode::Open).ok();
        let agent = make_agent_with_skills(skills).await;
        let session_id = agent
            .create_session_with_skill("alice", "no-tools-skill")
            .await
            .unwrap();

        // Retrieve the stored context and check tools is empty
        let ctx_json = agent
            .sessions
            .get_skill_context(session_id)
            .await
            .unwrap()
            .unwrap();
        let ctx: agentverse_skill::SkillContext = serde_json::from_str(&ctx_json).unwrap();
        assert!(
            ctx.tools.is_empty(),
            "skill declared no tools — ctx.tools should be empty"
        );

        // Verify that invoke would resolve active_tool_names to [] not to all tools.
        // We can't call invoke without a live LLM, but we can verify the stored context
        // has empty tools, which is the input to the active_tool_names calculation.
        // The calculation `None => all, Some(ctx) => filtered(ctx.tools)` should give [].
        let active: Vec<String> = ctx
            .tools
            .iter()
            .filter(|name| agent.tools.has_tool(name))
            .cloned()
            .collect();
        assert!(
            active.is_empty(),
            "expected zero active tools for skills with empty tools list"
        );
    }

    #[tokio::test]
    async fn skill_config_load_creates_registry_and_wraps_in_rwlock() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "test-skill", "Test.");
        let config = SkillConfig::load(dir.path(), agentverse_skill::SkillMode::Open)
            .expect("SkillConfig::load");
        let reg = config.registry.read().await;
        assert!(reg.get("test-skill").is_some());
    }

    #[tokio::test]
    async fn agent_new_accepts_skill_config() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "my-skill", "Instructions.");
        let skills = SkillConfig::load(dir.path(), agentverse_skill::SkillMode::Open).ok();
        // If this compiles and creates the agent, the signature is correct.
        let _agent = make_agent_with_skills(skills).await;
    }

    mod reload {
        use super::*;

        #[tokio::test]
        async fn reload_skills_refreshes_registry_with_new_skills() {
            let dir = tempdir().unwrap();
            write_skill_with_description(
                dir.path(),
                "system",
                "skill-a",
                "Does A.",
                "Instructions A.",
            );
            let skills = SkillConfig::load(dir.path(), agentverse_skill::SkillMode::Open).unwrap();
            let agent = make_agent_with_skills(Some(skills)).await;

            // Before reload: skill-a exists, skill-b does not
            {
                let s = agent.skills.as_ref().unwrap();
                let reg = s.registry.read().await;
                assert!(reg.get("skill-a").is_some());
                assert!(reg.get("skill-b").is_none());
            }

            // Add skill-b to directory while agent is running
            write_skill_with_description(
                dir.path(),
                "system",
                "skill-b",
                "Does B.",
                "Instructions B.",
            );

            // Reload
            agent.reload_skills().await.expect("reload_skills");

            // After reload: both skills present
            {
                let s = agent.skills.as_ref().unwrap();
                let reg = s.registry.read().await;
                assert!(
                    reg.get("skill-a").is_some(),
                    "skill-a should still be present"
                );
                assert!(
                    reg.get("skill-b").is_some(),
                    "skill-b should be present after reload"
                );
            }
        }

        #[tokio::test]
        async fn reload_skills_returns_error_when_no_skills_configured() {
            let agent = make_agent_with_skills(None).await;
            let result = agent.reload_skills().await;
            assert!(
                result.is_err(),
                "reload_skills should error when no skills configured"
            );
        }
    }

    fn write_skill_with_description(
        dir: &std::path::Path,
        subdir: &str,
        name: &str,
        description: &str,
        instructions: &str,
    ) {
        let pkg = dir.join(subdir).join(name);
        fs::create_dir_all(&pkg).unwrap();
        let content =
            format!("---\nname: {name}\ndescription: {description}\n---\n\n{instructions}\n");
        fs::write(pkg.join("SKILL.md"), content).unwrap();
    }

    #[tokio::test]
    async fn first_invoke_routes_to_matching_skill() {
        let dir = tempdir().unwrap();
        write_skill_with_description(
            dir.path(),
            "system",
            "code-review",
            "Review code for bugs and style issues.",
            "You are an expert code reviewer.",
        );
        let skills = SkillConfig::load(dir.path(), agentverse_skill::SkillMode::Open).unwrap();
        let agent = make_agent_with_skills(Some(skills)).await;
        let session_id = agent.create_session("alice").await.unwrap();

        // invoke will fail (no real LLM), but routing + DB write happen before LLM call
        let _ = agent
            .invoke("alice", session_id, "please review my code for bugs")
            .await;

        let ctx_json = agent.sessions.get_skill_context(session_id).await.unwrap();
        assert!(
            ctx_json.is_some(),
            "skill should be bound after matching first invoke"
        );
        let ctx: agentverse_skill::SkillContext = serde_json::from_str(&ctx_json.unwrap()).unwrap();
        assert!(ctx
            .instructions
            .contains("You are an expert code reviewer."));
    }

    #[tokio::test]
    async fn first_invoke_no_match_leaves_session_without_skill() {
        let dir = tempdir().unwrap();
        write_skill_with_description(
            dir.path(),
            "system",
            "code-review",
            "Review code for bugs and style issues.",
            "You are a reviewer.",
        );
        let skills = SkillConfig::load(dir.path(), agentverse_skill::SkillMode::Open).unwrap();
        let agent = make_agent_with_skills(Some(skills)).await;
        let session_id = agent.create_session("alice").await.unwrap();

        let _ = agent
            .invoke("alice", session_id, "what is the weather today")
            .await;

        let ctx_json = agent.sessions.get_skill_context(session_id).await.unwrap();
        assert!(
            ctx_json.is_none(),
            "unrelated message should not bind a skill"
        );
    }

    #[tokio::test]
    async fn second_invoke_does_not_re_route_an_explicitly_bound_session() {
        let dir = tempdir().unwrap();
        write_skill_with_description(
            dir.path(),
            "system",
            "code-review",
            "Review code.",
            "Reviewer instructions.",
        );
        write_skill_with_description(
            dir.path(),
            "system",
            "docs-writer",
            "Write documentation.",
            "Writer instructions.",
        );
        let skills = SkillConfig::load(dir.path(), agentverse_skill::SkillMode::Open).unwrap();
        let agent = make_agent_with_skills(Some(skills)).await;

        // Explicitly bind code-review
        let session_id = agent
            .create_session_with_skill("alice", "code-review")
            .await
            .unwrap();

        // Second invoke with a message that would match docs-writer — skill must not change
        let _ = agent
            .invoke("alice", session_id, "write documentation for this")
            .await;

        let ctx_json = agent
            .sessions
            .get_skill_context(session_id)
            .await
            .unwrap()
            .unwrap();
        let ctx: agentverse_skill::SkillContext = serde_json::from_str(&ctx_json).unwrap();
        assert!(
            ctx.instructions.contains("Reviewer instructions."),
            "explicit binding must not be overridden by routing"
        );
    }

    #[tokio::test]
    async fn constrained_mode_only_routes_to_allowed_skills() {
        let dir = tempdir().unwrap();
        write_skill_with_description(
            dir.path(),
            "system",
            "code-review",
            "Review code for bugs.",
            "Reviewer.",
        );
        write_skill_with_description(
            dir.path(),
            "system",
            "hr-onboarding",
            "Onboard new employees.",
            "HR onboarding.",
        );
        // Only allow hr-onboarding
        let mode = agentverse_skill::SkillMode::Constrained(vec!["hr-onboarding".into()]);
        let skills = SkillConfig::load(dir.path(), mode).unwrap();
        let agent = make_agent_with_skills(Some(skills)).await;
        let session_id = agent.create_session("alice").await.unwrap();

        // This message matches code-review but it's not in the allow-list
        let _ = agent
            .invoke("alice", session_id, "please review my code for bugs")
            .await;

        let ctx_json = agent.sessions.get_skill_context(session_id).await.unwrap();
        assert!(
            ctx_json.is_none(),
            "code-review is not in Constrained allow-list, should not bind"
        );
    }

    #[tokio::test]
    async fn advance_phase_returns_none_for_terminal_output() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "skill-a", "Skill A.");
        let skills = SkillConfig::load(dir.path(), SkillMode::Open).ok();
        let agent = make_agent_with_skills(skills).await;

        let session_id = agent
            .create_session_with_skill("alice", "skill-a")
            .await
            .unwrap();

        let result = agent
            .advance_phase("alice", session_id, "Final output. No directives.")
            .await
            .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn advance_phase_rebinds_skill_and_stores_context() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "skill-a", "Skill A instructions.");
        write_skill(dir.path(), "system", "skill-b", "Skill B instructions.");
        let skills = SkillConfig::load(dir.path(), SkillMode::Open).ok();
        let agent = make_agent_with_skills(skills).await;

        let session_id = agent
            .create_session_with_skill("alice", "skill-a")
            .await
            .unwrap();

        let output =
            "Extracted data.\n\nNEXT_SKILL: skill-b\nSUMMARY: Found 5 items; selected approach X.";
        let PhaseAdvanceResult::Advanced(transition) = agent
            .advance_phase("alice", session_id, output)
            .await
            .unwrap()
            .expect("expected Advanced, got Pending")
        else {
            panic!("expected Advanced, got Pending");
        };

        assert_eq!(transition.next_skill, "skill-b");
        assert_eq!(transition.summary, "Found 5 items; selected approach X.");
        assert_eq!(transition.deliverable, "Extracted data.");

        // Skill context must now point to skill-b
        let ctx_json = agent
            .sessions
            .get_skill_context(session_id)
            .await
            .unwrap()
            .expect("skill context must be set");
        let ctx: agentverse_skill::SkillContext = serde_json::from_str(&ctx_json).unwrap();
        assert!(ctx.instructions.contains("Skill B instructions."));

        // Phase opening context must store the summary only (deliverable travels as input)
        let phase_ctx = agent
            .sessions
            .get_phase_opening_context(session_id)
            .await
            .unwrap();
        assert!(phase_ctx.is_some());
        let ctx_str = phase_ctx.unwrap();
        assert!(ctx_str.contains("Found 5 items"));
        assert!(
            !ctx_str.contains("Extracted data."),
            "deliverable must not be stored in phase_opening_context"
        );
    }

    #[tokio::test]
    async fn advance_phase_errors_on_unknown_next_skill() {
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "skill-a", "Skill A.");
        let skills = SkillConfig::load(dir.path(), SkillMode::Open).ok();
        let agent = make_agent_with_skills(skills).await;

        let session_id = agent
            .create_session_with_skill("alice", "skill-a")
            .await
            .unwrap();

        let output = "Output.\nNEXT_SKILL: nonexistent\nSUMMARY: done";
        let result = agent.advance_phase("alice", session_id, output).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn phase_opening_context_is_cleared_after_detection() {
        // Verifies that advance_phase stores the context and that the session
        // reports it as set. The clearing itself happens inside invoke (which
        // requires a live LLM), but we can verify the storage side here.
        let dir = tempdir().unwrap();
        write_skill(dir.path(), "system", "stage-one", "Stage one.");
        write_skill(dir.path(), "system", "stage-two", "Stage two.");
        let skills = SkillConfig::load(dir.path(), SkillMode::Open).ok();
        let agent = make_agent_with_skills(skills).await;

        let session_id = agent
            .create_session_with_skill("alice", "stage-one")
            .await
            .unwrap();

        // Simulate advance_phase storing context
        let output = "Done.\nNEXT_SKILL: stage-two\nSUMMARY: Completed stage one.";
        agent
            .advance_phase("alice", session_id, output)
            .await
            .unwrap();

        // Context must be stored before any invoke
        let phase_ctx = agent
            .sessions
            .get_phase_opening_context(session_id)
            .await
            .unwrap();
        assert!(
            phase_ctx.is_some(),
            "phase opening context must be present before first invoke of new phase"
        );
        assert!(phase_ctx.unwrap().contains("Completed stage one."));
    }

    #[tokio::test]
    async fn agent_output_done_variant_carries_string() {
        let out = AgentOutput::Done("hello".to_string());
        assert!(matches!(out, AgentOutput::Done(ref s) if s == "hello"));
    }

    #[test]
    fn hitl_config_can_be_constructed() {
        use agentverse_hitl::{HitlPolicy, InMemoryQueue};
        let _cfg = HitlConfig {
            policy: HitlPolicy::new(),
            queue: Arc::new(InMemoryQueue::new()) as Arc<dyn agentverse_hitl::ApprovalQueue>,
        };
    }

    #[test]
    fn phase_advance_result_pending_carries_approval_id() {
        let id = uuid::Uuid::new_v4();
        let r = PhaseAdvanceResult::Pending { approval_id: id };
        assert!(matches!(r, PhaseAdvanceResult::Pending { approval_id } if approval_id == id));
    }
}
