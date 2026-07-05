use agentverse::memory::MemoryError;
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use super::embedder::{require_dimensions, require_setting, Embedder};

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiEmbedder {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model_name: String,
    dimensions: usize,
}

#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingDatum {
    index: usize,
    embedding: Vec<f32>,
}

#[async_trait]
impl Embedder for OpenAiEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let url = format!("{}/embeddings", self.base_url);
        let mut request = self.client.post(&url).json(&serde_json::json!({
            "model": self.model_name,
            "input": texts,
        }));
        if !self.api_key.is_empty() {
            request = request.bearer_auth(&self.api_key);
        }

        let response = request
            .send()
            .await
            .map_err(|e| MemoryError::Embedding(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| MemoryError::Embedding(e.to_string()))?;

        if !status.is_success() {
            return Err(MemoryError::Embedding(format!("{status}: {body}")));
        }

        let parsed: EmbeddingsResponse = serde_json::from_str(&body)
            .map_err(|e| MemoryError::Embedding(format!("invalid response body: {e}")))?;

        let mut data = parsed.data;
        data.sort_by_key(|d| d.index);

        for d in &data {
            if d.embedding.len() != self.dimensions {
                return Err(MemoryError::Embedding(format!(
                    "expected {} dimensions, got {}",
                    self.dimensions,
                    d.embedding.len()
                )));
            }
        }

        Ok(data.into_iter().map(|d| d.embedding).collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

pub(crate) fn openai_factory(
    settings: &HashMap<String, String>,
) -> Result<Arc<dyn Embedder>, MemoryError> {
    let model_name = require_setting(settings, "model_name", "openai")?;
    let dimensions = require_dimensions(settings, "openai")?;
    let base_url = settings.get("base_url").filter(|s| !s.is_empty()).cloned();
    let api_key = settings.get("api_key").cloned().unwrap_or_default();
    // api_key is optional when a custom base_url is set (local/self-hosted endpoints)
    if api_key.is_empty() && base_url.is_none() {
        return Err(MemoryError::Embedding(
            "missing required setting 'api_key' for provider 'openai'".to_string(),
        ));
    }
    Ok(Arc::new(OpenAiEmbedder {
        client: reqwest::Client::new(),
        base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        api_key,
        model_name,
        dimensions,
    }))
}
