use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use agentverse::memory::{Message, MessageRole};
use agentverse::{
    AgentError as CoreAgentError, Config, HitlHook, LlmRunner, PromptRegistry, RunStrategy,
    StrategyOutcome, Tool, ToolCall, ToolResult,
};
use agentverse_agent::{agent::HitlConfig, Agent, AgentOutput, SkillConfig, SkillMode};
use agentverse_hitl::{HitlPolicy, InMemoryQueue};
use agentverse_session::{
    Session, SessionId, SessionMemory, SessionMemoryError, SessionStatus, SqliteSessionMemory,
};
use agentverse_tools::ToolRegistry;
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use tempfile::TempDir;

#[derive(Default)]
struct FakeStore {
    sessions: Mutex<HashMap<SessionId, Session>>,
    messages: Mutex<HashMap<SessionId, Vec<Message>>>,
    watermarks: Mutex<HashMap<SessionId, i64>>,
}

#[async_trait]
impl SessionMemory for FakeStore {
    async fn create(&self, user_id: &str) -> Result<Session, SessionMemoryError> {
        let session = Session::new(user_id);
        self.sessions
            .lock()
            .unwrap()
            .insert(session.id, session.clone());
        Ok(session)
    }

    async fn get(&self, session_id: SessionId) -> Result<Option<Session>, SessionMemoryError> {
        Ok(self.sessions.lock().unwrap().get(&session_id).cloned())
    }

    async fn update_status(
        &self,
        session_id: SessionId,
        status: SessionStatus,
    ) -> Result<(), SessionMemoryError> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions
            .get_mut(&session_id)
            .ok_or(SessionMemoryError::NotFound(session_id))?;
        session.status = status;
        Ok(())
    }

    async fn list_by_user(&self, user_id: &str) -> Result<Vec<Session>, SessionMemoryError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn append_message(
        &self,
        session_id: SessionId,
        message: Message,
    ) -> Result<(), SessionMemoryError> {
        if !self.sessions.lock().unwrap().contains_key(&session_id) {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        self.messages
            .lock()
            .unwrap()
            .entry(session_id)
            .or_default()
            .push(message);
        Ok(())
    }

    async fn append_turn(
        &self,
        session_id: SessionId,
        user_message: Message,
        assistant_message: Message,
    ) -> Result<(), SessionMemoryError> {
        self.append_message(session_id, user_message).await?;
        self.append_message(session_id, assistant_message).await
    }

    async fn load_messages(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<Message>, SessionMemoryError> {
        if !self.sessions.lock().unwrap().contains_key(&session_id) {
            return Err(SessionMemoryError::NotFound(session_id));
        }
        Ok(self
            .messages
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionMemoryError> {
        Ok(*self
            .watermarks
            .lock()
            .unwrap()
            .get(&session_id)
            .unwrap_or(&0))
    }

    async fn advance_watermark(
        &self,
        session_id: SessionId,
        new_watermark: i64,
    ) -> Result<(), SessionMemoryError> {
        let mut wm = self.watermarks.lock().unwrap();
        let entry = wm.entry(session_id).or_insert(0);
        if new_watermark > *entry {
            *entry = new_watermark;
        }
        Ok(())
    }

    async fn load_messages_above_watermark(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<(i64, agentverse::memory::Message)>, SessionMemoryError> {
        let wm = self.get_watermark(session_id).await?;
        let msgs = self.load_messages(session_id).await?;
        Ok(msgs
            .into_iter()
            .enumerate()
            .map(|(i, m)| (i as i64 + 1, m))
            .filter(|(seq, _)| *seq > wm)
            .collect())
    }

    async fn cleanup_expired_messages(
        &self,
        _session_id: SessionId,
        _cutoff_ts: i64,
        _watermark: i64,
    ) -> Result<u64, SessionMemoryError> {
        Ok(0)
    }

    async fn list_sessions_needing_maintenance(&self) -> Result<Vec<Session>, SessionMemoryError> {
        Ok(vec![])
    }

    async fn delete_ended_sessions_before(
        &self,
        _cutoff_ts: i64,
    ) -> Result<u64, SessionMemoryError> {
        Ok(0)
    }

    async fn delete_session(&self, session_id: SessionId) -> Result<(), SessionMemoryError> {
        self.sessions.lock().unwrap().remove(&session_id);
        Ok(())
    }
}

#[derive(Deserialize, JsonSchema)]
struct WireTransferArgs {
    amount: u64,
}

struct WireTransferTool;

#[async_trait]
impl Tool for WireTransferTool {
    type Args = WireTransferArgs;

    fn name(&self) -> &str {
        "wire_transfer"
    }

    fn description(&self) -> &str {
        "Transfer funds"
    }

    async fn execute(&self, args: Self::Args) -> ToolResult {
        Ok(serde_json::json!({ "transferred": args.amount }))
    }
}

struct WireTransferStrategy {
    tools: Arc<ToolRegistry>,
}

impl WireTransferStrategy {
    fn call() -> ToolCall {
        ToolCall {
            name: "wire_transfer".into(),
            args: serde_json::json!({ "amount": 100 }),
        }
    }
}

#[async_trait]
impl RunStrategy for WireTransferStrategy {
    async fn run(&self, _messages: Vec<Message>) -> Result<StrategyOutcome, CoreAgentError> {
        Ok(StrategyOutcome::Done("wire transfer executed".into()))
    }

    async fn run_with_active_tools(
        &self,
        _messages: Vec<Message>,
        active_tool_names: &[String],
    ) -> Result<StrategyOutcome, CoreAgentError> {
        assert_eq!(active_tool_names, ["wire_transfer"]);
        let results = self.tools.execute_many(vec![Self::call()]).await;
        assert!(results[0].result.is_ok());
        Ok(StrategyOutcome::Done("wire transfer executed".into()))
    }

    async fn run_hitl(
        &self,
        messages: Vec<Message>,
        active_tool_names: &[String],
        hook: Arc<dyn HitlHook>,
    ) -> Result<StrategyOutcome, CoreAgentError> {
        let call = Self::call();
        match self
            .tools
            .execute_many_hitl(vec![call.clone()], &hook)
            .await
        {
            Ok(results) => {
                assert!(results[0].result.is_ok());
                Ok(StrategyOutcome::Done("wire transfer executed".into()))
            }
            Err(interrupt) => Ok(StrategyOutcome::Interrupted(agentverse::HitlInterrupt {
                approval_id: interrupt.approval_id,
                kind_json: interrupt.kind_json,
                history: messages,
                pending_calls: vec![call],
                active_tool_names: active_tool_names.to_vec(),
            })),
        }
    }
}

fn write_skill(root: &Path, slot: &str, id: &str, instructions: &str, hitl_tools: &[&str]) {
    let package = root.join(slot).join(id);
    std::fs::create_dir_all(&package).unwrap();
    let hitl_tools = hitl_tools
        .iter()
        .map(|name| format!("    - {name}"))
        .collect::<Vec<_>>()
        .join("\n");
    let hitl_block = if hitl_tools.is_empty() {
        String::new()
    } else {
        format!("  hitl_tools:\n{hitl_tools}\n")
    };
    std::fs::write(
        package.join("SKILL.md"),
        format!(
            "---\nname: {id}\ndescription: Transfer funds.\nagentverse:\n  tools:\n    - wire_transfer\n{hitl_block}---\n\n{instructions}\n"
        ),
    )
    .unwrap();
}

async fn agent_with_skills(root: &TempDir) -> Arc<Agent> {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test",
                "sk-test",
                Some("http://127.0.0.1:1/v1".into()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(WireTransferTool);
    let strategy = Arc::new(WireTransferStrategy {
        tools: Arc::clone(&tools),
    });
    let sessions = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
    let skills = SkillConfig::load(root.path(), SkillMode::Open).unwrap();

    Agent::builder(
        runner,
        tools,
        Arc::new(PromptRegistry::new()),
        sessions,
        strategy,
    )
    .with_skills(skills)
    .build()
}

async fn agent_with_global_tool_blocklist() -> Arc<Agent> {
    let runner = Arc::new(
        LlmRunner::from_config(Config {
            provider: agentverse::ProviderConfig::openai(
                "test",
                "sk-test",
                Some("http://127.0.0.1:1/v1".into()),
            ),
            max_messages: 10,
            tools: vec![],
            prompts_dir: None,
            system_prompt: None,
        })
        .unwrap(),
    );
    let tools = ToolRegistry::new();
    tools.register(WireTransferTool);
    let strategy = Arc::new(WireTransferStrategy {
        tools: Arc::clone(&tools),
    });
    let sessions = Arc::new(SqliteSessionMemory::new("sqlite::memory:").await.unwrap());
    let policy = HitlPolicy {
        global_tool_blocklist: HashSet::from(["wire_transfer".to_string()]),
        ..HitlPolicy::default()
    };

    Agent::builder(
        runner,
        tools,
        Arc::new(PromptRegistry::new()),
        sessions,
        strategy,
    )
    .with_hitl(HitlConfig {
        policy,
        queue: Arc::new(InMemoryQueue::new()),
    })
    .build()
}

#[tokio::test]
async fn global_hitl_tool_blocklist_interrupts_through_public_agent_invoke() {
    let agent = agent_with_global_tool_blocklist().await;
    let session_id = agent.create_session("alice").await.unwrap();

    let output = agent
        .invoke("alice", session_id, "Transfer $100")
        .await
        .unwrap();

    assert!(matches!(
        output,
        AgentOutput::Interrupted {
            kind: agentverse_hitl::InterruptKind::ToolApproval { ref tool_name, .. },
            ..
        } if tool_name == "wire_transfer"
    ));
}

#[tokio::test]
async fn system_hitl_gate_survives_same_id_user_shadow_through_invoke() {
    let root = tempfile::tempdir().unwrap();
    write_skill(
        root.path(),
        "system",
        "payments",
        "Trusted system instructions.",
        &["wire_transfer"],
    );
    write_skill(
        root.path(),
        "user",
        "payments",
        "User runtime instructions.",
        &[],
    );
    let agent = agent_with_skills(&root).await;
    let session_id = agent
        .create_session_with_skill("alice", "payments")
        .await
        .unwrap();

    let output = agent
        .invoke("alice", session_id, "Transfer $100")
        .await
        .unwrap();

    assert!(matches!(
        output,
        AgentOutput::Interrupted {
            kind: agentverse_hitl::InterruptKind::ToolApproval { ref tool_name, .. },
            ..
        } if tool_name == "wire_transfer"
    ));
}

#[tokio::test]
async fn user_only_hitl_declaration_does_not_influence_policy_through_invoke() {
    let root = tempfile::tempdir().unwrap();
    write_skill(
        root.path(),
        "user",
        "payments",
        "User runtime instructions.",
        &["wire_transfer"],
    );
    let agent = agent_with_skills(&root).await;
    let session_id = agent
        .create_session_with_skill("alice", "payments")
        .await
        .unwrap();

    let output = agent
        .invoke("alice", session_id, "Transfer $100")
        .await
        .unwrap();

    assert!(matches!(
        output,
        AgentOutput::Done(ref text) if text == "wire transfer executed"
    ));
}

#[tokio::test]
async fn session_manager_rejects_wrong_user_before_llm_call() {
    let session_memory = Arc::new(FakeStore::default());
    let session = session_memory.create("alice").await.unwrap();
    let manager = agentverse_session::SessionManager::new(session_memory);

    let err = manager.assert_owner("bob", session.id).await.unwrap_err();
    assert!(matches!(err, SessionMemoryError::NotFound(id) if id == session.id));
}

#[tokio::test]
async fn append_turn_contract_preserves_user_then_assistant_order() {
    let session_memory = Arc::new(FakeStore::default());
    let session = session_memory.create("alice").await.unwrap();

    session_memory
        .append_turn(
            session.id,
            Message {
                role: MessageRole::User,
                content: "hello".to_string(),
            },
            Message {
                role: MessageRole::Assistant,
                content: "hi".to_string(),
            },
        )
        .await
        .unwrap();

    let messages = session_memory.load_messages(session.id).await.unwrap();
    assert_eq!(messages[0].content, "hello");
    assert_eq!(messages[1].content, "hi");
}

#[test]
fn agent_with_subagent_executor_registers_spawn_subagent_tool() {
    use agentverse::ConnectionManager;
    use agentverse::PromptRegistry;
    use agentverse_subagent::SubAgentExecutor;
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    let tools = ToolRegistry::new();
    assert!(!tools.has_tool("spawn_subagent"));

    let cm = Arc::new(ConnectionManager::anthropic(
        "http://127.0.0.1:1",
        "claude-sonnet-4-6",
        "test-key",
    ));
    let executor = Arc::new(SubAgentExecutor::new(
        cm,
        Arc::clone(&tools),
        Arc::new(PromptRegistry::new()),
    ));

    SubAgentExecutor::register_tool(&executor, &tools);
    assert!(tools.has_tool("spawn_subagent"));
}
