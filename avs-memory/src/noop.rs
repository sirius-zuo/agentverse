use super::traits::LongTermBackend;
use agentverse::memory::{MemoryError, Message};
use async_trait::async_trait;

pub struct NoopBackend;

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
