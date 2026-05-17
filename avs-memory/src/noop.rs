use super::traits::{LongTermBackend, Summarizer};
use agentverse::memory::{MemoryError, Message, MessageRole};
use async_trait::async_trait;

pub struct NoopSummarizer;
pub struct NoopBackend;

#[async_trait]
impl Summarizer for NoopSummarizer {
    async fn summarize(&self, _messages: &[Message]) -> Result<Message, MemoryError> {
        Ok(Message {
            role: MessageRole::System,
            content: "[summary]".to_string(),
        })
    }
}

#[async_trait]
impl LongTermBackend for NoopBackend {
    async fn store(&self, _message: Message, _embedding: Vec<f32>) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn search(
        &self,
        _embedding: Vec<f32>,
        _top_k: usize,
    ) -> Result<Vec<Message>, MemoryError> {
        Ok(vec![])
    }
}
