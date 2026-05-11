use async_trait::async_trait;

use crate::error::ModelError;

mod anthropic_provider;
mod gemini_provider;
mod openai_compatible;
mod provider_wrapper;

pub use anthropic_provider::AnthropicProvider;
pub use gemini_provider::GeminiProvider;
pub use openai_compatible::OpenAICompatible;
pub use provider_wrapper::ProviderWrapper;

#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn name(&self) -> &str;

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
