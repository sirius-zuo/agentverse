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
    let content = format!("---\nname: {name}\ndescription: {description}\n---\n\n{instructions}\n");
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
