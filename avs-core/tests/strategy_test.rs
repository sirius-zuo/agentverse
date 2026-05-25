use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, RunStrategy};
use async_trait::async_trait;

struct EchoStrategy;

#[async_trait]
impl RunStrategy for EchoStrategy {
    async fn run(&self, messages: Vec<Message>) -> Result<String, AgentError> {
        let last = messages.iter().rev()
            .find(|m| matches!(m.role, MessageRole::User))
            .map(|m| m.content.clone())
            .unwrap_or_default();
        Ok(last)
    }
}

#[tokio::test]
async fn run_strategy_echo_returns_last_user_message() {
    let strategy = EchoStrategy;
    let messages = vec![
        Message { role: MessageRole::User, content: "hello world".to_string() },
    ];
    let result = strategy.run(messages).await.unwrap();
    assert_eq!(result, "hello world");
}

#[tokio::test]
async fn run_strategy_is_object_safe() {
    let strategy: Box<dyn RunStrategy> = Box::new(EchoStrategy);
    let messages = vec![Message { role: MessageRole::User, content: "test".to_string() }];
    let result = strategy.run(messages).await.unwrap();
    assert_eq!(result, "test");
}
