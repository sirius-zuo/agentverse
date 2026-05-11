use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::ModelProvider;
use crate::error::ModelError;
use crate::model::ToolDefinition;

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

    async fn generate(
        &self,
        prompt: &str,
        tools: Option<Vec<ToolDefinition>>,
    ) -> Result<String, ModelError> {
        // Check circuit breaker
        let mut cb = self.circuit_breaker.write().await;
        if !cb.can_execute() {
            return Err(ModelError::CircuitOpen(
                "Circuit breaker is open, retry later".to_string(),
            ));
        }
        drop(cb);

        let mut last_error = None;
        for attempt in 0..=self.max_retries {
            let result = self.inner.generate(prompt, tools.clone()).await;

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
                        return Err(Self::convert_to_rate_limited(e));
                    }
                    // Wait before retry with exponential backoff
                    let delay =
                        Duration::from_millis(self.retry_delay_ms * 2u64.pow(attempt as u32));
                    tokio::time::sleep(delay).await;
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ModelError::ApiError("Unexpected: no error after retries".to_string())
        }))
    }
}
