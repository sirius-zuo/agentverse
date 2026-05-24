use crate::error::ModelError;

mod anthropic_provider;
mod gemini_provider;
mod openai_compatible;
mod connection_manager;

pub use anthropic_provider::AnthropicProvider;
pub use connection_manager::ConnectionManager;
pub use gemini_provider::GeminiProvider;
pub use openai_compatible::OpenAICompatible;

/// Per-call LLM usage statistics. Zero-filled for providers that do not report a field.
#[derive(Debug, Default, Clone, Copy)]
pub struct UsageStats {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Tokens written to provider cache (Anthropic: cache_creation_input_tokens).
    pub cache_write_tokens: u32,
    /// Tokens served from provider cache (Anthropic: cache_read_input_tokens).
    pub cache_read_tokens: u32,
}

impl std::ops::AddAssign for UsageStats {
    fn add_assign(&mut self, other: Self) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
    }
}

/// Structured input to a model provider.
///
/// Replaces the flat `prompt: &str`. Each provider maps these fields to its
/// own wire format. Caching is applied internally by each provider.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    /// Rendered system prompt (from the "system" template). Stable across iterations.
    pub system: Option<String>,
    /// Conversation history with roles preserved.
    pub messages: Vec<crate::memory::Message>,
    /// Tool definitions for native tool calling (optional).
    pub tools: Option<Vec<ToolDefinition>>,
}

/// Response from a model provider, including usage statistics.
#[derive(Debug, Clone)]
pub struct GenerateResponse {
    pub content: String,
    pub usage: UsageStats,
}

/// Final output of a strategy loop: the answer plus accumulated token usage.
#[derive(Debug, Clone)]
pub struct CycleResult {
    pub answer: String,
    pub total_usage: UsageStats,
}

pub trait ModelProvider: Send + Sync {
    fn name(&self) -> &str;
    fn build_request(
        &self,
        model: &str,
        request: GenerateRequest,
    ) -> Result<serde_json::Value, ModelError>;
    fn parse_response(&self, body: &str) -> Result<GenerateResponse, ModelError>;
    fn request_headers(&self, api_key: &str) -> reqwest::header::HeaderMap;
    fn endpoint_path(&self, model: &str) -> String;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_stats_add_assign() {
        let mut a = UsageStats {
            input_tokens: 10,
            output_tokens: 5,
            cache_write_tokens: 100,
            cache_read_tokens: 0,
        };
        let b = UsageStats {
            input_tokens: 20,
            output_tokens: 8,
            cache_write_tokens: 0,
            cache_read_tokens: 100,
        };
        a += b;
        assert_eq!(a.input_tokens, 30);
        assert_eq!(a.output_tokens, 13);
        assert_eq!(a.cache_write_tokens, 100);
        assert_eq!(a.cache_read_tokens, 100);
    }

    #[test]
    fn usage_stats_default_is_zero() {
        let u = UsageStats::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.cache_read_tokens, 0);
    }

    #[test]
    fn cycle_result_holds_answer_and_usage() {
        let r = CycleResult {
            answer: "done".to_string(),
            total_usage: UsageStats {
                input_tokens: 50,
                output_tokens: 10,
                cache_write_tokens: 40,
                cache_read_tokens: 40,
            },
        };
        assert_eq!(r.answer, "done");
        assert_eq!(r.total_usage.cache_read_tokens, 40);
    }
}
