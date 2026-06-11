use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderValue, CONTENT_TYPE};
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use super::anthropic_provider::AnthropicProvider;
use super::gemini_provider::GeminiProvider;
use super::openai_compatible::OpenAICompatible;
use super::ModelProvider;
use crate::config::ProviderConfig;
use crate::error::ModelError;
use crate::model::{GenerateRequest, GenerateResponse};

// ── Circuit breaker ───────────────────────────────────────────────────────────

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

// ── ConnectionManager ─────────────────────────────────────────────────────────

/// Owns the HTTP client, auth config, circuit breaker, and retry logic.
/// `provider` is a pure protocol translator (no HTTP).
pub struct ConnectionManager {
    client: Client,
    api_base: String,
    api_key: String,
    model_name: String,
    provider: Box<dyn ModelProvider>,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    max_retries: usize,
    retry_delay_ms: u64,
}

impl ConnectionManager {
    pub fn new(
        provider: impl ModelProvider + 'static,
        api_base: &str,
        api_key: &str,
        model_name: &str,
    ) -> Self {
        Self {
            client: Client::new(),
            api_base: api_base.to_string(),
            api_key: api_key.to_string(),
            model_name: model_name.to_string(),
            provider: Box::new(provider),
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new(5, 30))),
            max_retries: 3,
            retry_delay_ms: 500,
        }
    }

    pub fn anthropic(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new(AnthropicProvider::new(), api_base, api_key, model_name)
    }

    pub fn openai(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new(OpenAICompatible::new(), api_base, api_key, model_name)
    }

    pub fn gemini(api_base: &str, model_name: &str, api_key: &str) -> Self {
        Self::new(GeminiProvider::new(), api_base, api_key, model_name)
    }

    pub fn from_config(config: ProviderConfig) -> Result<Self, ModelError> {
        match config {
            ProviderConfig::Anthropic {
                model_name,
                api_key,
            } => Ok(Self::anthropic(
                "https://api.anthropic.com",
                &model_name,
                &api_key,
            )),
            ProviderConfig::OpenAI {
                model_name,
                api_key,
                base_url,
            } => Ok(Self::openai(
                base_url.as_deref().unwrap_or("https://api.openai.com/v1"),
                &model_name,
                &api_key,
            )),
            ProviderConfig::Gemini {
                model_name,
                api_key,
            } => Ok(Self::gemini(
                "https://generativelanguage.googleapis.com",
                &model_name,
                &api_key,
            )),
        }
    }

    pub fn with_retries(mut self, max_retries: usize, retry_delay_ms: u64) -> Self {
        self.max_retries = max_retries;
        self.retry_delay_ms = retry_delay_ms;
        self
    }

    pub fn with_circuit_breaker(mut self, threshold: usize, timeout_secs: u64) -> Self {
        self.circuit_breaker = Arc::new(Mutex::new(CircuitBreaker::new(threshold, timeout_secs)));
        self
    }

    /// Return a new `ConnectionManager` targeting a different model. Used by
    /// `SubAgentExecutor` for per-SubAgent model overrides.
    pub fn with_model(&self, model_name: &str) -> Self {
        let provider: Box<dyn ModelProvider> = match self.provider.name() {
            "anthropic" => Box::new(AnthropicProvider::new()),
            "gemini" => Box::new(GeminiProvider::new()),
            "openai" => Box::new(OpenAICompatible::new()),
            other => unreachable!("with_model: unknown provider name {:?}", other),
        };
        Self {
            client: self.client.clone(),
            api_base: self.api_base.clone(),
            api_key: self.api_key.clone(),
            model_name: model_name.to_string(),
            provider,
            // SubAgent model overrides start with a fresh circuit breaker —
            // the old breaker's state is irrelevant for a different model endpoint.
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new(5, 30))),
            max_retries: self.max_retries,
            retry_delay_ms: self.retry_delay_ms,
        }
    }

    /// Expose provider's `build_request` for tests only. Not for production use.
    #[doc(hidden)]
    pub fn provider_build_request_for_test(
        &self,
        request: crate::model::GenerateRequest,
    ) -> Result<serde_json::Value, crate::error::ModelError> {
        self.provider.build_request(&self.model_name, request)
    }

    pub async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        // 1. Log prompt
        tracing::debug!(">>>>>>>>>> LLM PROMPT BEGIN <<<<<<<<<<");
        if let Some(sys) = &request.system {
            tracing::debug!(content = %sys, "SYSTEM");
        }
        for (i, msg) in request.messages.iter().enumerate() {
            tracing::debug!(index = i, role = ?msg.role, content = %msg.content, "MSG");
        }
        tracing::debug!(">>>>>>>>>> LLM PROMPT END <<<<<<<<<<");

        // 2. Circuit breaker check
        {
            let mut cb = self.circuit_breaker.lock().await;
            if !cb.can_execute() {
                return Err(ModelError::CircuitOpen(
                    "Circuit breaker is open".to_string(),
                ));
            }
        }

        // 3. Build request body
        let body = self
            .provider
            .build_request(&self.model_name, request.clone())?;

        // 4. Build URL
        let path = self.provider.endpoint_path(&self.model_name);
        let url = if self.provider.name() == "gemini" {
            format!("{}{}?key={}", self.api_base, path, self.api_key)
        } else {
            format!("{}{}", self.api_base, path)
        };

        // 5. Build headers
        let mut headers = self.provider.request_headers(&self.api_key);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        // 6-9. Retry loop
        let mut last_error: Option<ModelError> = None;
        for attempt in 0..=self.max_retries {
            match self
                .client
                .post(&url)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await
            {
                Err(e) => {
                    let err = ModelError::ApiError(e.to_string());
                    warn!(attempt, error = %err, "HTTP send failed");
                    self.circuit_breaker.lock().await.record_failure();
                    last_error = Some(err);
                    if attempt < self.max_retries {
                        tokio::time::sleep(Duration::from_millis(
                            self.retry_delay_ms * 2u64.pow(attempt as u32),
                        ))
                        .await;
                        continue;
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body_text = resp
                        .text()
                        .await
                        .map_err(|e| ModelError::ApiError(e.to_string()))?;

                    if status == 429 {
                        let err = ModelError::RateLimited(body_text);
                        warn!(attempt, error = %err, "Rate limited");
                        self.circuit_breaker.lock().await.record_failure();
                        last_error = Some(err);
                        if attempt < self.max_retries {
                            tokio::time::sleep(Duration::from_millis(
                                self.retry_delay_ms * 2u64.pow(attempt as u32),
                            ))
                            .await;
                            continue;
                        }
                    } else if !status.is_success() {
                        let err = ModelError::ApiError(format!("HTTP {}: {}", status, body_text));
                        error!(attempt, error = %err, "LLM call failed");
                        self.circuit_breaker.lock().await.record_failure();
                        return Err(err);
                    } else {
                        match self.provider.parse_response(&body_text) {
                            Ok(response) => {
                                self.circuit_breaker.lock().await.record_success();
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
                                self.circuit_breaker.lock().await.record_failure();
                                error!(error = %e, "Failed to parse LLM response");
                                return Err(e);
                            }
                        }
                    }
                }
            }
        }

        // Exhausted retries (failures already recorded per-attempt)
        Err(last_error.unwrap_or_else(|| ModelError::ApiError("No response".to_string())))
    }
}
