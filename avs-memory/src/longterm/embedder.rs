use agentverse::memory::MemoryError;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

/// Turns text into vectors for similarity search.
#[async_trait]
pub trait Embedder: Send + Sync {
    /// Embed a batch of texts. One vector per input, same order.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError>;
    /// Dimensionality of produced vectors.
    fn dimensions(&self) -> usize;
}

pub type EmbedderFactory =
    Box<dyn Fn(&HashMap<String, String>) -> Result<Arc<dyn Embedder>, MemoryError> + Send + Sync>;

/// A name-keyed table of embedder factories. Plain struct, not global state —
/// every caller (production code and tests alike) constructs its own.
pub struct EmbedderRegistry {
    factories: HashMap<String, EmbedderFactory>,
}

impl EmbedderRegistry {
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Registers the embedders this crate ships: "openai", "gemini".
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register("openai", Box::new(super::embedder_openai::openai_factory));
        registry.register("gemini", Box::new(super::embedder_gemini::gemini_factory));
        registry
    }

    pub fn register(&mut self, name: impl Into<String>, factory: EmbedderFactory) {
        self.factories.insert(name.into(), factory);
    }

    pub fn build(
        &self,
        name: &str,
        settings: &HashMap<String, String>,
    ) -> Result<Arc<dyn Embedder>, MemoryError> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| MemoryError::Embedding(format!("unknown embedder provider: {name}")))?;
        factory(settings)
    }
}

impl Default for EmbedderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn require_setting(
    settings: &HashMap<String, String>,
    key: &str,
    provider: &str,
) -> Result<String, MemoryError> {
    match settings.get(key) {
        Some(v) if !v.is_empty() => Ok(v.clone()),
        _ => Err(MemoryError::Embedding(format!(
            "missing required setting '{key}' for provider '{provider}'"
        ))),
    }
}

pub(crate) fn require_dimensions(
    settings: &HashMap<String, String>,
    provider: &str,
) -> Result<usize, MemoryError> {
    let raw = require_setting(settings, "dimensions", provider)?;
    raw.parse::<usize>().map_err(|_| {
        MemoryError::Embedding(format!(
            "invalid 'dimensions' setting for provider '{provider}': {raw}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_provider_returns_error() {
        let registry = EmbedderRegistry::with_builtins();
        let result = registry.build("nonexistent", &HashMap::new());
        assert!(
            matches!(result, Err(MemoryError::Embedding(ref msg)) if msg.contains("nonexistent"))
        );
    }

    #[test]
    fn custom_provider_can_be_registered_and_built() {
        struct FakeEmbedder;
        #[async_trait]
        impl Embedder for FakeEmbedder {
            async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError> {
                Ok(texts.iter().map(|_| vec![0.0]).collect())
            }
            fn dimensions(&self) -> usize {
                1
            }
        }
        let mut registry = EmbedderRegistry::new(); // no builtins — proves this isn't special-cased
        registry.register(
            "fake",
            Box::new(|_settings: &HashMap<String, String>| {
                Ok(Arc::new(FakeEmbedder) as Arc<dyn Embedder>)
            }),
        );
        let e = registry.build("fake", &HashMap::new()).unwrap();
        assert_eq!(e.dimensions(), 1);
    }
}
