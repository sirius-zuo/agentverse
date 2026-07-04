use super::{Agent, AgentError, PhaseAdvanceResult, PhaseTransition};
use agentverse_session::{InterruptedState, SessionId};
use agentverse_skill::{SkillError, SkillRegistry};

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

                    agentverse::metrics::record_phase_transition(
                        agentverse::metrics::PhaseTransitionOutcome::PendingApproval,
                    );
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

        agentverse::metrics::record_phase_transition(
            agentverse::metrics::PhaseTransitionOutcome::Advanced,
        );
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
        let join_result = tokio::task::spawn_blocking(move || SkillRegistry::load(&dir)).await;
        let new_registry = match join_result {
            Ok(load_result) => load_result.map_err(AgentError::Skill)?,
            Err(join_err) if join_err.is_panic() => {
                // Propagate the real panic instead of disguising it as IO.
                std::panic::resume_unwind(join_err.into_panic())
            }
            Err(join_err) => {
                return Err(AgentError::Skill(SkillError::Io(std::io::Error::other(
                    format!("skill reload task was cancelled: {join_err}"),
                ))))
            }
        };
        // Rebuild summaries and ids while we still have owned access to new_registry.
        skills.rebuild_caches(&new_registry);
        *skills.registry.write().await = new_registry;
        tracing::info!(dir = ?skills.dir, "skill registry reloaded");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Agent, PhaseAdvanceResult};
    use super::parse_phase_transition;
    use agentverse::{Config, LlmRunner, PromptRegistry};
    use agentverse_session::SqliteSessionMemory;
    use agentverse_skill::{SkillConfig, SkillMode};
    use agentverse_strategy::{build, StrategyKind};
    use agentverse_tools::ToolRegistry;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

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
        let builder = Agent::builder(runner, tools, prompts, session_memory, strategy);
        match skills {
            Some(skills) => builder.with_skills(skills).build(),
            None => builder.build(),
        }
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

            {
                let s = agent.skills.as_ref().unwrap();
                let reg = s.registry.read().await;
                assert!(reg.get("skill-a").is_some());
                assert!(reg.get("skill-b").is_none());
            }

            write_skill_with_description(
                dir.path(),
                "system",
                "skill-b",
                "Does B.",
                "Instructions B.",
            );

            agent.reload_skills().await.expect("reload_skills");

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

        let session_id = agent
            .create_session_with_skill("alice", "code-review")
            .await
            .unwrap();

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
        let mode = agentverse_skill::SkillMode::Constrained(vec!["hr-onboarding".into()]);
        let skills = SkillConfig::load(dir.path(), mode).unwrap();
        let agent = make_agent_with_skills(Some(skills)).await;
        let session_id = agent.create_session("alice").await.unwrap();

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

        let ctx_json = agent
            .sessions
            .get_skill_context(session_id)
            .await
            .unwrap()
            .expect("skill context must be set");
        let ctx: agentverse_skill::SkillContext = serde_json::from_str(&ctx_json).unwrap();
        assert!(ctx.instructions.contains("Skill B instructions."));

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

        let output = "Done.\nNEXT_SKILL: stage-two\nSUMMARY: Completed stage one.";
        agent
            .advance_phase("alice", session_id, output)
            .await
            .unwrap();

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
}
