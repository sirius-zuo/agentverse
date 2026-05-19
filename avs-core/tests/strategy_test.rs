use agentverse::{AgentError, RunStrategy};

struct Echo;

#[async_trait::async_trait]
impl RunStrategy for Echo {
    async fn process(&mut self, input: String) -> Result<String, AgentError> {
        Ok(format!("echo: {}", input))
    }
}

#[tokio::test]
async fn test_run_strategy_trait() {
    let mut echo = Echo;
    let result = echo.process("hello".to_string()).await.unwrap();
    assert_eq!(result, "echo: hello");
}
