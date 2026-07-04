use super::{Agent, AgentError, AgentOutput, CacheMemory};
use agentverse::memory::{LongtermRecord, Message, MessageRole};
use agentverse_session::SessionId;
use agentverse_skill::{RouteSkills, SkillContext, SkillRouter};
use std::time::Instant;

impl Agent {
    pub(super) fn assemble_system(
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

    pub(super) async fn get_cache_memory(
        &self,
        user_id: &str,
        session_id: SessionId,
    ) -> Result<Vec<Message>, AgentError> {
        let key = (user_id.to_string(), session_id);
        {
            let cache = self.cache_memory.lock().await;
            if let Some(buf) = cache.get(&key) {
                if buf.last_used.elapsed() <= self.buffer_ttl {
                    agentverse::metrics::record_cache_access(agentverse::metrics::CacheResult::Hit);
                    return Ok(buf.messages.clone());
                }
            }
        }
        agentverse::metrics::record_cache_access(agentverse::metrics::CacheResult::Miss);
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
        let invoke_start = Instant::now();
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
                    agentverse::metrics::record_skill_routing(
                        agentverse::metrics::SkillRoutingOutcome::Matched,
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
                    agentverse::metrics::record_skill_routing(
                        agentverse::metrics::SkillRoutingOutcome::NoMatch,
                    );
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
            let hook = std::sync::Arc::new(agentverse_hitl::HitlContext::new(
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
            Err(agentverse::AgentError::Memory(ref msg))
                if agentverse::hitl::HitlWire::is_wire(msg) =>
            {
                agentverse::metrics::record_invoke_duration(
                    invoke_start.elapsed(),
                    agentverse::metrics::InvokeOutcome::Interrupted,
                );
                return self
                    .handle_tool_interrupt(user_id, session_id, msg, &active_tool_names, &skill_ctx)
                    .await;
            }
            Err(e) => {
                agentverse::metrics::record_invoke_duration(
                    invoke_start.elapsed(),
                    agentverse::metrics::InvokeOutcome::Error,
                );
                return Err(AgentError::Llm(e));
            }
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

        agentverse::metrics::record_invoke_duration(
            invoke_start.elapsed(),
            agentverse::metrics::InvokeOutcome::Done,
        );
        Ok(AgentOutput::Done(response))
    }
}

#[cfg(test)]
mod tests {
    use super::super::Agent;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_skill::SkillContext;
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
        Agent::builder(runner, tools, prompts, session_memory, strategy).build()
    }

    #[tokio::test]
    async fn invoke_stateless_returns_error_on_bad_port() {
        let agent = make_agent().await;
        let result = agent.invoke_stateless("hello").await;
        assert!(result.is_err());
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
