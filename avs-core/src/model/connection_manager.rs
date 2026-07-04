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
    /// Settings this instance was built from — retained so `with_model` can
    /// re-invoke the same provider's factory when switching models.
    settings: std::collections::HashMap<String, String>,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    max_retries: usize,
    retry_delay_ms: u64,
    fallback: Option<Box<ConnectionManager>>,
}

// Minimal manual impl (not derived: `provider` and `circuit_breaker` hold
// non-`Debug` types) — needed so `Result<ConnectionManager, _>::unwrap_err()`
// is usable in tests.
impl std::fmt::Debug for ConnectionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionManager")
            .field("model_name", &self.model_name)
            .finish_non_exhaustive()
    }
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
            settings: std::collections::HashMap::new(),
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new(5, 30))),
            max_retries: 3,
            retry_delay_ms: 500,
            fallback: None,
        }
    }

    /// Settings mirror what the "anthropic" registry factory expects, so a
    /// later `with_model` call on an instance built this way still resolves
    /// correctly through the registry.
    pub fn anthropic(api_base: &str, model_name: &str, api_key: &str) -> Self {
        let mut cm = Self::new(AnthropicProvider::new(), api_base, api_key, model_name);
        cm.settings = std::collections::HashMap::from([
            ("model_name".to_string(), model_name.to_string()),
            ("api_key".to_string(), api_key.to_string()),
        ]);
        cm
    }

    pub fn openai(api_base: &str, model_name: &str, api_key: &str) -> Self {
        let mut cm = Self::new(OpenAICompatible::new(), api_base, api_key, model_name);
        cm.settings = std::collections::HashMap::from([
            ("model_name".to_string(), model_name.to_string()),
            ("api_key".to_string(), api_key.to_string()),
            ("base_url".to_string(), api_base.to_string()),
        ]);
        cm
    }

    pub fn gemini(api_base: &str, model_name: &str, api_key: &str) -> Self {
        let mut cm = Self::new(GeminiProvider::new(), api_base, api_key, model_name);
        cm.settings = std::collections::HashMap::from([
            ("model_name".to_string(), model_name.to_string()),
            ("api_key".to_string(), api_key.to_string()),
        ]);
        cm
    }

    pub fn from_config(
        config: ProviderConfig,
        registry: &crate::model::ProviderRegistry,
    ) -> Result<Self, ModelError> {
        let resolved = registry.build(&config.name, &config.settings)?;
        // Built directly rather than via `Self::new` — `resolved.provider` is
        // already `Box<dyn ModelProvider>`, and `new()` takes `impl ModelProvider`
        // (for the un-boxed convenience constructors), so it can't accept it.
        Ok(Self {
            client: Client::new(),
            api_base: resolved.api_base,
            api_key: resolved.api_key,
            model_name: resolved.model_name,
            provider: resolved.provider,
            settings: config.settings,
            circuit_breaker: Arc::new(Mutex::new(CircuitBreaker::new(5, 30))),
            max_retries: 3,
            retry_delay_ms: 500,
            fallback: None,
        })
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

    /// Configure a fallback provider tried when the primary's circuit breaker is
    /// open or all retries are exhausted.
    pub fn with_fallback(mut self, fallback: ConnectionManager) -> Self {
        self.fallback = Some(Box::new(fallback));
        self
    }

    /// Return a new `ConnectionManager` targeting a different model within
    /// the same provider/endpoint/key. Used by `SubAgentExecutor` for
    /// per-SubAgent model overrides. `model_name` overrides whatever the
    /// factory resolves from `self.settings` (which still holds the
    /// original model_name) — same override semantics as before the registry.
    pub fn with_model(
        &self,
        model_name: &str,
        registry: &crate::model::ProviderRegistry,
    ) -> Result<Self, ModelError> {
        let resolved = registry.build(self.provider.name(), &self.settings)?;
        Ok(Self {
            client: self.client.clone(),
            api_base: self.api_base.clone(),
            api_key: self.api_key.clone(),
            model_name: model_name.to_string(),
            provider: resolved.provider,
            settings: self.settings.clone(),
            // Model overrides hit the same endpoint with the same key, so an
            // open circuit must apply to them too — share breaker state.
            circuit_breaker: Arc::clone(&self.circuit_breaker),
            max_retries: self.max_retries,
            retry_delay_ms: self.retry_delay_ms,
            fallback: None,
        })
    }

    #[doc(hidden)]
    pub fn circuit_breaker_ptr_for_test(&self) -> usize {
        Arc::as_ptr(&self.circuit_breaker) as usize
    }

    /// Expose provider's `build_request` for tests only. Not for production use.
    #[doc(hidden)]
    pub fn provider_build_request_for_test(
        &self,
        request: crate::model::GenerateRequest,
    ) -> Result<serde_json::Value, crate::error::ModelError> {
        self.provider.build_request(&self.model_name, request)
    }

    fn backoff_ms(&self, attempt: usize) -> u64 {
        self.retry_delay_ms
            .saturating_mul(2u64.pow((attempt as u32).min(10)))
    }

    /// 429s get a longer pause than transient errors when the server
    /// doesn't send Retry-After.
    fn rate_limit_backoff_ms(&self, attempt: usize) -> u64 {
        self.backoff_ms(attempt).saturating_mul(4)
    }

    /// Clamp a server-provided `Retry-After` delay so a hostile or
    /// misconfigured server can't stall the retry loop indefinitely.
    fn clamp_retry_after_ms(ms: u64) -> u64 {
        ms.min(60_000)
    }

    pub async fn generate(&self, request: GenerateRequest) -> Result<GenerateResponse, ModelError> {
        let call_start = Instant::now();
        let provider_name = self.provider.name();

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
                crate::metrics::record_circuit_open(provider_name);
                if let Some(ref fallback) = self.fallback {
                    tracing::warn!("Primary circuit open; using fallback provider");
                    crate::metrics::record_llm_fallback(provider_name);
                    return Box::pin(fallback.generate(request)).await;
                }
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
            let attempt_start = Instant::now();
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
                        crate::metrics::record_llm_retry(crate::metrics::RetryReason::Transport);
                        tokio::time::sleep(Duration::from_millis(self.backoff_ms(attempt))).await;
                        continue;
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    let retry_after_ms = resp
                        .headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.trim().parse::<u64>().ok())
                        .map(|secs| secs.saturating_mul(1000))
                        .map(Self::clamp_retry_after_ms);
                    let body_text = match resp.text().await {
                        Ok(text) => text,
                        Err(e) => {
                            crate::metrics::record_llm_call(
                                provider_name,
                                &self.model_name,
                                None,
                                call_start.elapsed(),
                                Some("api_error"),
                            );
                            return Err(ModelError::ApiError(e.to_string()));
                        }
                    };

                    if status == 429 {
                        let err = ModelError::RateLimited(body_text);
                        warn!(attempt, error = %err, "Rate limited");
                        self.circuit_breaker.lock().await.record_failure();
                        last_error = Some(err);
                        if attempt < self.max_retries {
                            crate::metrics::record_llm_retry(
                                crate::metrics::RetryReason::RateLimited,
                            );
                            let delay_ms = retry_after_ms
                                .unwrap_or_else(|| self.rate_limit_backoff_ms(attempt));
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                            continue;
                        }
                    } else if !status.is_success() {
                        let err = ModelError::ApiError(format!("HTTP {}: {}", status, body_text));
                        error!(attempt, error = %err, "LLM call failed");
                        self.circuit_breaker.lock().await.record_failure();
                        if let Some(ref fallback) = self.fallback {
                            tracing::warn!(primary_error = %err, "Primary provider failed; attempting fallback");
                            crate::metrics::record_llm_fallback(provider_name);
                            return Box::pin(fallback.generate(request)).await;
                        }
                        crate::metrics::record_llm_call(
                            provider_name,
                            &self.model_name,
                            None,
                            call_start.elapsed(),
                            Some("api_error"),
                        );
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
                                    elapsed_ms = attempt_start.elapsed().as_millis() as u64,
                                    "LLM call complete"
                                );
                                tracing::debug!(">>>>>>>>>> LLM RESPONSE BEGIN <<<<<<<<<<");
                                tracing::debug!(content = %response.content, "RESPONSE");
                                tracing::debug!(">>>>>>>>>> LLM RESPONSE END <<<<<<<<<<");
                                crate::metrics::record_llm_call(
                                    provider_name,
                                    &self.model_name,
                                    Some(&response.usage),
                                    call_start.elapsed(),
                                    None,
                                );
                                return Ok(response);
                            }
                            Err(e) => {
                                self.circuit_breaker.lock().await.record_failure();
                                error!(error = %e, "Failed to parse LLM response");
                                crate::metrics::record_llm_call(
                                    provider_name,
                                    &self.model_name,
                                    None,
                                    call_start.elapsed(),
                                    Some("invalid_response"),
                                );
                                return Err(e);
                            }
                        }
                    }
                }
            }
        }

        // Exhausted retries (failures already recorded per-attempt)
        let primary_err =
            last_error.unwrap_or_else(|| ModelError::ApiError("No response".to_string()));
        if let Some(ref fallback) = self.fallback {
            tracing::warn!(primary_error = %primary_err, "Primary provider exhausted retries; attempting fallback");
            crate::metrics::record_llm_fallback(provider_name);
            return Box::pin(fallback.generate(request)).await;
        }
        let error_type = if matches!(primary_err, ModelError::RateLimited(_)) {
            "rate_limited"
        } else {
            "api_error"
        };
        crate::metrics::record_llm_call(
            provider_name,
            &self.model_name,
            None,
            call_start.elapsed(),
            Some(error_type),
        );
        Err(primary_err)
    }
}

#[cfg(test)]
mod with_model_tests {
    use super::*;
    use crate::error::ModelError;
    use crate::model::{GenerateRequest, GenerateResponse};

    struct FakeProvider;
    impl ModelProvider for FakeProvider {
        fn name(&self) -> &'static str {
            "fake"
        }
        fn build_request(
            &self,
            _model: &str,
            _req: GenerateRequest,
        ) -> Result<serde_json::Value, ModelError> {
            Ok(serde_json::json!({}))
        }
        fn parse_response(&self, _body: &str) -> Result<GenerateResponse, ModelError> {
            Err(ModelError::InvalidResponse("fake".into()))
        }
        fn request_headers(&self, _api_key: &str) -> reqwest::header::HeaderMap {
            reqwest::header::HeaderMap::new()
        }
        fn endpoint_path(&self, _model: &str) -> String {
            "/fake".into()
        }
    }

    #[test]
    fn with_model_unknown_provider_returns_error() {
        let cm = ConnectionManager::new(FakeProvider, "http://x", "k", "m");
        let registry = crate::model::ProviderRegistry::with_builtins();
        let err = cm.with_model("other-model", &registry).unwrap_err();
        assert!(matches!(err, ModelError::UnknownProvider(name) if name == "fake"));
    }

    #[test]
    fn with_model_shares_circuit_breaker() {
        let cm = ConnectionManager::openai("http://x", "m", "k");
        let registry = crate::model::ProviderRegistry::with_builtins();
        let overridden = cm.with_model("m2", &registry).unwrap();
        assert_eq!(
            cm.circuit_breaker_ptr_for_test(),
            overridden.circuit_breaker_ptr_for_test()
        );
    }

    #[test]
    fn backoff_is_exponential_and_capped() {
        let cm = ConnectionManager::openai("http://x", "m", "k").with_retries(3, 500);
        assert_eq!(cm.backoff_ms(0), 500);
        assert_eq!(cm.backoff_ms(1), 1000);
        assert_eq!(cm.backoff_ms(2), 2000);
        // exponent capped at 10 — no overflow even for absurd attempt counts
        assert_eq!(cm.backoff_ms(64), 500 * 1024);
    }

    #[test]
    fn rate_limit_backoff_is_4x() {
        let cm = ConnectionManager::openai("http://x", "m", "k").with_retries(3, 500);
        assert_eq!(cm.rate_limit_backoff_ms(0), 2000);
    }

    #[test]
    fn clamp_retry_after_ms_caps_at_60s() {
        assert_eq!(ConnectionManager::clamp_retry_after_ms(59_999), 59_999);
        assert_eq!(ConnectionManager::clamp_retry_after_ms(60_000), 60_000);
        assert_eq!(ConnectionManager::clamp_retry_after_ms(1_000_000), 60_000);
        assert_eq!(ConnectionManager::clamp_retry_after_ms(u64::MAX), 60_000);
    }

    #[test]
    fn from_config_rejects_key_with_header_invalid_chars() {
        let registry = crate::model::ProviderRegistry::with_builtins();
        let err = ConnectionManager::from_config(
            crate::config::ProviderConfig::anthropic("m".to_string(), "bad\nkey".to_string()),
            &registry,
        )
        .unwrap_err();
        assert!(matches!(err, ModelError::InvalidApiKey(_)));
    }

    #[test]
    fn from_config_accepts_empty_key() {
        // Local OpenAI-compatible endpoints legitimately use no key.
        let registry = crate::model::ProviderRegistry::with_builtins();
        assert!(ConnectionManager::from_config(
            crate::config::ProviderConfig::openai(
                "m".to_string(),
                String::new(),
                Some("http://localhost:9090/v1".to_string()),
            ),
            &registry,
        )
        .is_ok());
    }
}
