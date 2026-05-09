use async_trait::async_trait;

use crate::error::ModelError;

mod openai_compatible;
pub use openai_compatible::OpenAICompatible;

#[async_trait]
pub trait ModelProvider: Send + Sync {
    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
