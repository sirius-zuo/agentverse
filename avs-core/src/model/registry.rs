use std::collections::HashMap;

use super::{AnthropicProvider, GeminiProvider, ModelProvider, OpenAICompatible};
use crate::error::ModelError;
use reqwest::header::HeaderValue;

/// Everything `ConnectionManager` needs from a factory call: the provider
/// instance and the three settings every provider must resolve, even
/// though each reads them from its own settings keys/defaults.
pub struct ResolvedProvider {
    pub provider: Box<dyn ModelProvider>,
    pub api_base: String,
    pub api_key: String,
    pub model_name: String,
}

impl std::fmt::Debug for ResolvedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedProvider")
            .field("provider", &self.provider.name())
            .field("api_base", &self.api_base)
            .field("api_key", &"[REDACTED]")
            .field("model_name", &self.model_name)
            .finish()
    }
}

pub type ProviderFactory =
    Box<dyn Fn(&HashMap<String, String>) -> Result<ResolvedProvider, ModelError> + Send + Sync>;

/// A name-keyed table of provider factories. Plain struct, not global state —
/// every caller (production code and tests alike) constructs its own.
pub struct ProviderRegistry {
    factories: HashMap<String, ProviderFactory>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Registers the three providers `avs-core` ships: "anthropic", "openai", "gemini".
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register("anthropic", Box::new(anthropic_factory));
        registry.register("openai", Box::new(openai_factory));
        registry.register("gemini", Box::new(gemini_factory));
        registry
    }

    pub fn register(&mut self, name: impl Into<String>, factory: ProviderFactory) {
        self.factories.insert(name.into(), factory);
    }

    pub fn build(
        &self,
        name: &str,
        settings: &HashMap<String, String>,
    ) -> Result<ResolvedProvider, ModelError> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| ModelError::UnknownProvider(name.to_string()))?;
        factory(settings)
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn require_setting(
    settings: &HashMap<String, String>,
    key: &str,
    provider: &str,
) -> Result<String, ModelError> {
    match settings.get(key) {
        Some(v) if !v.is_empty() => Ok(v.clone()),
        _ => Err(ModelError::MissingSetting(
            key.to_string(),
            provider.to_string(),
        )),
    }
}

fn validate_header_value(api_key: &str) -> Result<(), ModelError> {
    if HeaderValue::from_str(api_key).is_err() {
        return Err(ModelError::InvalidApiKey(
            "API key contains characters that are invalid in an HTTP header \
             (control characters or non-visible ASCII)"
                .into(),
        ));
    }
    Ok(())
}

fn anthropic_factory(settings: &HashMap<String, String>) -> Result<ResolvedProvider, ModelError> {
    let model_name = require_setting(settings, "model_name", "anthropic")?;
    let api_key = require_setting(settings, "api_key", "anthropic")?;
    validate_header_value(&api_key)?;
    Ok(ResolvedProvider {
        provider: Box::new(AnthropicProvider::new()),
        api_base: "https://api.anthropic.com".to_string(),
        api_key,
        model_name,
    })
}

fn openai_factory(settings: &HashMap<String, String>) -> Result<ResolvedProvider, ModelError> {
    let model_name = require_setting(settings, "model_name", "openai")?;
    let base_url = settings.get("base_url").filter(|s| !s.is_empty()).cloned();
    let api_key = settings.get("api_key").cloned().unwrap_or_default();
    // api_key is optional when a custom base_url is set (local/self-hosted endpoints)
    if api_key.is_empty() && base_url.is_none() {
        return Err(ModelError::MissingSetting(
            "api_key".to_string(),
            "openai".to_string(),
        ));
    }
    if !api_key.is_empty() {
        validate_header_value(&api_key)?;
    }
    Ok(ResolvedProvider {
        provider: Box::new(OpenAICompatible::new()),
        api_base: base_url.unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        api_key,
        model_name,
    })
}

fn gemini_factory(settings: &HashMap<String, String>) -> Result<ResolvedProvider, ModelError> {
    let model_name = require_setting(settings, "model_name", "gemini")?;
    let api_key = require_setting(settings, "api_key", "gemini")?;
    validate_header_value(&api_key)?;
    Ok(ResolvedProvider {
        provider: Box::new(GeminiProvider::new()),
        api_base: "https://generativelanguage.googleapis.com".to_string(),
        api_key,
        model_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::GenerateRequest;

    fn settings(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn with_builtins_resolves_anthropic() {
        let registry = ProviderRegistry::with_builtins();
        let resolved = registry
            .build(
                "anthropic",
                &settings(&[("model_name", "claude-x"), ("api_key", "k")]),
            )
            .unwrap();
        assert_eq!(resolved.provider.name(), "anthropic");
        assert_eq!(resolved.api_base, "https://api.anthropic.com");
        assert_eq!(resolved.api_key, "k");
        assert_eq!(resolved.model_name, "claude-x");
    }

    #[test]
    fn with_builtins_resolves_openai_with_base_url_and_no_key() {
        let registry = ProviderRegistry::with_builtins();
        let resolved = registry
            .build(
                "openai",
                &settings(&[
                    ("model_name", "local-model"),
                    ("base_url", "http://localhost:9090/v1"),
                ]),
            )
            .unwrap();
        assert_eq!(resolved.provider.name(), "openai");
        assert_eq!(resolved.api_base, "http://localhost:9090/v1");
        assert_eq!(resolved.api_key, "");
    }

    #[test]
    fn with_builtins_openai_defaults_base_url_when_absent() {
        let registry = ProviderRegistry::with_builtins();
        let resolved = registry
            .build(
                "openai",
                &settings(&[("model_name", "gpt-4o"), ("api_key", "sk-x")]),
            )
            .unwrap();
        assert_eq!(resolved.api_base, "https://api.openai.com/v1");
    }

    #[test]
    fn with_builtins_openai_missing_key_and_base_url_errors() {
        let registry = ProviderRegistry::with_builtins();
        let err = registry
            .build("openai", &settings(&[("model_name", "gpt-4o")]))
            .unwrap_err();
        assert!(matches!(err, ModelError::MissingSetting(k, p) if k == "api_key" && p == "openai"));
    }

    #[test]
    fn with_builtins_resolves_gemini() {
        let registry = ProviderRegistry::with_builtins();
        let resolved = registry
            .build(
                "gemini",
                &settings(&[("model_name", "gemini-pro"), ("api_key", "k")]),
            )
            .unwrap();
        assert_eq!(resolved.provider.name(), "gemini");
        assert_eq!(
            resolved.api_base,
            "https://generativelanguage.googleapis.com"
        );
    }

    #[test]
    fn unknown_provider_returns_error() {
        let registry = ProviderRegistry::with_builtins();
        let err = registry.build("nonexistent", &settings(&[])).unwrap_err();
        assert!(matches!(err, ModelError::UnknownProvider(name) if name == "nonexistent"));
    }

    #[test]
    fn missing_model_name_returns_missing_setting() {
        let registry = ProviderRegistry::with_builtins();
        let err = registry
            .build("anthropic", &settings(&[("api_key", "k")]))
            .unwrap_err();
        assert!(
            matches!(err, ModelError::MissingSetting(k, p) if k == "model_name" && p == "anthropic")
        );
    }

    #[test]
    fn invalid_api_key_header_char_returns_invalid_api_key() {
        let registry = ProviderRegistry::with_builtins();
        let err = registry
            .build(
                "anthropic",
                &settings(&[("model_name", "m"), ("api_key", "bad\nkey")]),
            )
            .unwrap_err();
        assert!(matches!(err, ModelError::InvalidApiKey(_)));
    }

    struct FakeProvider;
    impl ModelProvider for FakeProvider {
        fn name(&self) -> &str {
            "fake"
        }
        fn build_request(
            &self,
            _model: &str,
            _request: GenerateRequest,
        ) -> Result<serde_json::Value, ModelError> {
            Ok(serde_json::json!({}))
        }
        fn parse_response(
            &self,
            _body: &str,
        ) -> Result<crate::model::GenerateResponse, ModelError> {
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
    fn custom_provider_can_be_registered_and_built() {
        let mut registry = ProviderRegistry::new(); // no builtins — proves this isn't special-cased
        registry.register(
            "fake",
            Box::new(|settings: &HashMap<String, String>| {
                Ok(ResolvedProvider {
                    provider: Box::new(FakeProvider),
                    api_base: "http://fake".to_string(),
                    api_key: settings.get("api_key").cloned().unwrap_or_default(),
                    model_name: settings.get("model_name").cloned().unwrap_or_default(),
                })
            }),
        );
        let resolved = registry
            .build("fake", &settings(&[("model_name", "m"), ("api_key", "k")]))
            .unwrap();
        assert_eq!(resolved.provider.name(), "fake");
        assert_eq!(resolved.api_base, "http://fake");
    }
}
