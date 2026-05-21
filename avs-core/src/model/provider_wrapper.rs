use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::RwLock;

use tracing::{error, info, warn};

use super::{AnthropicProvider, GeminiProvider, ModelProvider, OpenAICompatible};
use crate::config::ProviderConfig;
use crate::error::{AgentError, ModelError};
use crate::model::{GenerateRequest, GenerateResponse};

/// Wrapper around a ModelProvider that adds retry and circuit breaker logic.
pub struct ProviderWrapper {
    inner: Arc<dyn ModelProvider>,
    max_retries: usize,
    retry_delay_ms: u64,
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
}

struct CircuitBreaker {
    state: CircuitState,
    consecutive_failures: usize,
    threshold: usize,
    last_failure: Option<Instant>,
    timeout_secs: u64,
}

enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

impl CircuitBreaker {
    fn new(threshold: usize, timeout_secs: u64) -> Self {
        Self {
            state: CircuitState::Closed,
            consecutive_failures: 0,
            threshold,
            last_failure: None,
            timeout_secs,
        }
    }

    fn can_execute(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                if let Some(last) = self.last_failure {
                    if last.elapsed() > Duration::from_secs(self.timeout_secs) {
                        self.state = CircuitState::HalfOpen;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.state = CircuitState::Closed;
    }

    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.last_failure = Some(Instant::now());
        if self.consecutive_failures >= self.threshold {
            self.state = CircuitState::Open;
        }
    }
}

impl ProviderWrapper {
    pub fn new(inner: impl ModelProvider + 'static) -> Self {
        Self {
            inner: Arc::new(inner),
            max_retries: 3,
            retry_delay_ms: 500,
            circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::new(5, 30))),
        }
    }

    pub fn openai(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new(OpenAICompatible::new(api_base, model_name, api_key))
    }

    pub fn anthropic(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new(AnthropicProvider::new(api_base, model_name, api_key))
    }

    pub fn gemini(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new(GeminiProvider::new(api_base, model_name, api_key))
    }

    pub fn from_config(config: ProviderConfig) -> Result<Self, AgentError> {
        let inner: Arc<dyn ModelProvider> = match config {
            ProviderConfig::OpenAI { .. } => Arc::new(OpenAICompatible::from_config(config)?),
            ProviderConfig::Anthropic { .. } => Arc::new(AnthropicProvider::from_config(config)?),
            ProviderConfig::Gemini { .. } => Arc::new(GeminiProvider::from_config(config)?),
        };
        Ok(Self {
            inner,
            max_retries: 3,
            retry_delay_ms: 500,
            circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::new(5, 30))),
        })
    }

    pub fn with_retries(mut self, max_retries: usize, retry_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.retry_delay_ms = retry_delay_ms;
        self
    }

    pub fn with_circuit_breaker(mut self, threshold: usize, timeout_secs: u64) -> Self {
        self.circuit_breaker = Arc::new(RwLock::new(CircuitBreaker::new(threshold, timeout_secs)));
        self
    }

    fn should_retry(error: &ModelError) -> bool {
        matches!(error, ModelError::RateLimited(_) | ModelError::ApiError(_))
    }

    fn convert_to_rate_limited(error: ModelError) -> ModelError {
        match &error {
            ModelError::ApiError(msg) if msg.contains("429") => {
                ModelError::RateLimited(msg.clone())
            }
            _ => error,
        }
    }
}

#[async_trait]
impl ModelProvider for ProviderWrapper {
    fn name(&self) -> &str {
        self.inner.name()
    }

    async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        tracing::debug!(">>>>>>>>>> LLM PROMPT BEGIN <<<<<<<<<<");
        if let Some(sys) = &request.system {
            tracing::debug!(content = %sys, "SYSTEM");
        }
        for (i, msg) in request.messages.iter().enumerate() {
            tracing::debug!(index = i, role = ?msg.role, content = %msg.content, "MSG");
        }
        if let Some(tools) = &request.tools {
            tracing::debug!(tool_count = tools.len(), "TOOLS");
        }
        tracing::debug!(">>>>>>>>>> LLM PROMPT END <<<<<<<<<<");

        let mut cb = self.circuit_breaker.write().await;
        if !cb.can_execute() {
            return Err(ModelError::CircuitOpen(
                "Circuit breaker is open, retry later".to_string(),
            ));
        }
        drop(cb);

        let mut last_error = None;
        for attempt in 0..=self.max_retries {
            let result = self.inner.generate(request.clone()).await;

            match result {
                Ok(response) => {
                    let mut cb = self.circuit_breaker.write().await;
                    cb.record_success();
                    info!(
                        input_tokens = response.usage.input_tokens,
                        output_tokens = response.usage.output_tokens,
                        cache_read_tokens = response.usage.cache_read_tokens,
                        cache_write_tokens = response.usage.cache_write_tokens,
                        "LLM call complete"
                    );
                    tracing::debug!(">>>>>>>>>> LLM RESPONSE BEGIN <<<<<<<<<<");
                    tracing::debug!(content = %response.content, "RESPONSE");
                    tracing::debug!(">>>>>>>>>> LLM RESPONSE END <<<<<<<<<<");
                    return Ok(response);
                }
                Err(e) => {
                    last_error = Some(e.clone());
                    if !Self::should_retry(&e) || attempt == self.max_retries {
                        let mut cb = self.circuit_breaker.write().await;
                        cb.record_failure();
                        error!(attempt, error = %e, "LLM call failed");
                        return Err(Self::convert_to_rate_limited(e));
                    }
                    let delay =
                        Duration::from_millis(self.retry_delay_ms * 2u64.pow(attempt as u32));
                    warn!(attempt, retry_delay_ms = delay.as_millis(), error = %e, "LLM call failed, retrying");
                    tokio::time::sleep(delay).await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ModelError::ApiError("Unexpected: no error after retries".to_string())
        }))
    }
}

#[cfg(test)]
mod logging_tests {
    use super::*;
    use crate::error::ModelError;
    use crate::model::{GenerateResponse, UsageStats};

    struct FakeProvider;

    #[async_trait::async_trait]
    impl ModelProvider for FakeProvider {
        fn name(&self) -> &str {
            "fake"
        }
        async fn generate(
            &self,
            _request: GenerateRequest,
        ) -> Result<GenerateResponse, ModelError> {
            Ok(GenerateResponse {
                content: "ok".to_string(),
                usage: UsageStats {
                    input_tokens: 10,
                    output_tokens: 5,
                    cache_read_tokens: 2,
                    cache_write_tokens: 0,
                },
            })
        }
    }

    #[tokio::test]
    async fn generate_logs_usage_without_panic() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();

        let wrapper = ProviderWrapper::new(FakeProvider);
        let result = wrapper
            .generate(GenerateRequest {
                system: Some("sys".to_string()),
                messages: vec![],
                tools: None,
            })
            .await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert_eq!(resp.usage.input_tokens, 10);
    }
}
