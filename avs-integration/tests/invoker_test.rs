use agentverse::{AgentError, RunStrategy};
use agentverse_integration::{AgentInvoker, Event, StrategyInvoker};
use async_trait::async_trait;
use std::collections::HashMap;

struct EchoStrategy;

#[async_trait]
impl RunStrategy for EchoStrategy {
    async fn process(&mut self, input: String) -> Result<String, AgentError> {
        Ok(format!("echo: {}", input))
    }
}

#[tokio::test]
async fn strategy_invoker_echoes_text() {
    let invoker = StrategyInvoker::new(EchoStrategy);
    let event = Event {
        id: uuid::Uuid::new_v4(),
        conversation_id: "ch1".to_string(),
        user_id: "u1".to_string(),
        text: "hello".to_string(),
        metadata: HashMap::new(),
    };
    let response = invoker.invoke(event.clone()).await.unwrap();
    assert_eq!(response.text, "echo: hello");
    assert_eq!(response.conversation_id, event.conversation_id);
    assert_eq!(response.user_id, event.user_id);
}
