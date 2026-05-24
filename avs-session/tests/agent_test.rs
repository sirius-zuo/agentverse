use agentverse::{Config, LlmRunner, ProviderConfig};
use agentverse_session::agent::Agent;
use agentverse_session::session::SessionStatus;
use agentverse_session::sqlite::SqliteSessionStore;
use std::sync::Arc;
use tempfile::tempdir;

fn closed_port_config() -> Config {
    Config {
        provider: ProviderConfig::OpenAI {
            model_name: "gpt-4o".to_string(),
            api_key: "sk-xxx".to_string(),
            base_url: Some("http://127.0.0.1:1/v1".to_string()),
        },
        max_messages: 50,
        tools: vec![],
        prompts_dir: None,
        system_prompt: None,
    }
}

async fn make_agent() -> Agent {
    let dir = tempdir().unwrap();
    let db_path = dir.into_path().join("test.db");
    let store = Arc::new(
        SqliteSessionStore::new(db_path.to_str().unwrap()).await.unwrap()
    );
    let runner = Arc::new(LlmRunner::from_config(closed_port_config()).unwrap());
    Agent::new(runner, store)
}

#[tokio::test]
async fn agent_create_session_returns_id() {
    let agent = make_agent().await;
    let id = agent.create_session("alice").await.unwrap();
    let session = agent.get_session(id).await.unwrap().unwrap();
    assert_eq!(session.user_id, "alice");
}

#[tokio::test]
async fn agent_invoke_persists_user_message_before_llm_call() {
    let agent = make_agent().await;
    let id = agent.create_session("alice").await.unwrap();
    // Network error expected (closed port)
    let _ = agent.invoke(id, "hello").await;
    // User message should have been appended before the LLM call attempted
    let messages = agent.load_messages(id).await.unwrap();
    assert!(!messages.is_empty());
    assert_eq!(messages[0].content, "hello");
}

#[tokio::test]
async fn agent_end_session_marks_completed() {
    let agent = make_agent().await;
    let id = agent.create_session("bob").await.unwrap();
    agent.end_session(id).await.unwrap();
    let session = agent.get_session(id).await.unwrap().unwrap();
    assert!(matches!(session.status, SessionStatus::Completed));
}

#[tokio::test]
async fn agent_list_sessions_for_user() {
    let agent = make_agent().await;
    agent.create_session("charlie").await.unwrap();
    agent.create_session("charlie").await.unwrap();
    let sessions = agent.list_sessions("charlie").await.unwrap();
    assert_eq!(sessions.len(), 2);
}
