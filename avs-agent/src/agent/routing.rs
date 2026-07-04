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
#[path = "routing_tests.rs"]
mod tests;
