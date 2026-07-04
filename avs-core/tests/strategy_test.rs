use agentverse::memory::{Message, MessageRole};
use agentverse::{AgentError, RunStrategy, StrategyOutcome};
use async_trait::async_trait;

struct EchoStrategy;

#[async_trait]
impl RunStrategy for EchoStrategy {
    async fn run(&self, messages: Vec<Message>) -> Result<StrategyOutcome, AgentError> {
        let last = messages
            .iter()
            .rev()
            .find(|m| matches!(m.role, MessageRole::User))
            .map(|m| m.content.clone())
            .unwrap_or_default();
        Ok(StrategyOutcome::Done(last))
    }
}

fn unwrap_done(outcome: StrategyOutcome) -> String {
    match outcome {
        StrategyOutcome::Done(text) => text,
        StrategyOutcome::Interrupted(_) => panic!("expected Done, got Interrupted"),
    }
}

#[tokio::test]
async fn run_strategy_echo_returns_last_user_message() {
    let strategy = EchoStrategy;
    let messages = vec![Message {
        role: MessageRole::User,
        content: "hello world".to_string(),
    }];
    let result = strategy.run(messages).await.unwrap();
    assert_eq!(unwrap_done(result), "hello world");
}

#[tokio::test]
async fn run_strategy_is_object_safe() {
    let strategy: Box<dyn RunStrategy> = Box::new(EchoStrategy);
    let messages = vec![Message {
        role: MessageRole::User,
        content: "test".to_string(),
    }];
    let result = strategy.run(messages).await.unwrap();
    assert_eq!(unwrap_done(result), "test");
}
