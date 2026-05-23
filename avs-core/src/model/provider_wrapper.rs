use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

use tracing::{error, info, warn};

use super::{AnthropicProvider, GeminiProvider, ModelProvider, OpenAICompatible};
use crate::config::ProviderConfig;
use crate::error::{AgentError, ModelError};
use crate::model::{GenerateRequest, GenerateResponse};

/// Wrapper around a ModelProvider that adds retry and circuit breaker logic.
/// NOTE: This struct is being phased out in the multi-user session refactor (Task 2).
pub struct ProviderWrapper {
    #[allow(dead_code)]
    pub(crate) inner: Arc<dyn ModelProvider>,
    #[allow(dead_code)]
    api_base: String,
    #[allow(dead_code)]
    api_key: String,
    #[allow(dead_code)]
    model_name: String,
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
    pub fn new_with_config(
        inner: impl ModelProvider + 'static,
        api_base: &str,
        api_key: &str,
        model_name: &str,
    ) -> Self {
        Self {
            inner: Arc::new(inner),
            api_base: api_base.to_string(),
            api_key: api_key.to_string(),
            model_name: model_name.to_string(),
            max_retries: 3,
            retry_delay_ms: 500,
            circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::new(5, 30))),
        }
    }

    pub fn new(inner: impl ModelProvider + 'static) -> Self {
        Self::new_with_config(inner, "", "", "")
    }

    pub fn openai(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new_with_config(OpenAICompatible::new(), api_base, api_key, model_name)
    }

    pub fn anthropic(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new_with_config(AnthropicProvider::new(), api_base, api_key, model_name)
    }

    pub fn gemini(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new_with_config(GeminiProvider::new(), api_base, api_key, model_name)
    }

    pub fn from_config(config: ProviderConfig) -> Result<Self, AgentError> {
        let (inner, api_base, api_key, model_name): (Arc<dyn ModelProvider>, String, String, String) = match config {
            ProviderConfig::OpenAI { model_name, api_key, base_url } => (
                Arc::new(OpenAICompatible::new()),
                base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                api_key,
                model_name,
            ),
            ProviderConfig::Anthropic { model_name, api_key } => (
                Arc::new(AnthropicProvider::new()),
                "https://api.anthropic.com".to_string(),
                api_key,
                model_name,
            ),
            ProviderConfig::Gemini { model_name, api_key } => (
                Arc::new(GeminiProvider::new()),
                "https://generativelanguage.googleapis.com".to_string(),
                api_key,
                model_name,
            ),
        };
        Ok(Self {
            inner,
            api_base,
            api_key,
            model_name,
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

    /// Execute an HTTP call with retry and circuit breaker logic.
    /// Takes a future factory so it can be retried.
    pub async fn execute<F, Fut, T>(&self, f: F) -> Result<T, ModelError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, ModelError>>,
    {
        let mut cb = self.circuit_breaker.write().await;
        if !cb.can_execute() {
            return Err(ModelError::CircuitOpen(
                "Circuit breaker is open, retry later".to_string(),
            ));
        }
        drop(cb);

        let mut last_error = None;
        for attempt in 0..=self.max_retries {
            let result = f().await;

            match result {
                Ok(response) => {
                    let mut cb = self.circuit_breaker.write().await;
                    cb.record_success();
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

    /// Stub: will be replaced by ConnectionManager in Task 2.
    /// Returns an error indicating the method is not yet implemented.
    pub async fn generate(&self, _request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        Err(ModelError::ApiError(
            "ProviderWrapper.generate() is not implemented; use ConnectionManager".to_string(),
        ))
    }

    /// Log usage statistics after a successful LLM call.
    pub fn log_usage(usage: &crate::model::UsageStats) {
        info!(
            input_tokens = usage.input_tokens,
            output_tokens = usage.output_tokens,
            cache_read_tokens = usage.cache_read_tokens,
            cache_write_tokens = usage.cache_write_tokens,
            "LLM call complete"
        );
    }
}
