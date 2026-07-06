use agentverse::memory::MemoryError;
use async_trait::async_trait;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;

use super::embedder::{require_dimensions, require_setting, Embedder};

const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";

pub struct GeminiEmbedder {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model_name: String,
    dimensions: usize,
}

#[derive(Debug, Deserialize)]
struct BatchEmbedResponse {
    embeddings: Vec<EmbeddingValues>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingValues {
    values: Vec<f32>,
}

#[async_trait]
impl Embedder for GeminiEmbedder {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let url = format!(
            "{}/v1beta/models/{}:batchEmbedContents?key={}",
            self.base_url, self.model_name, self.api_key
        );
        let requests: Vec<serde_json::Value> = texts
            .iter()
            .map(|t| {
                serde_json::json!({
                    "model": format!("models/{}", self.model_name),
                    "content": {"parts": [{"text": t}]},
                })
            })
            .collect();

        // `without_url()` strips the URL from reqwest's error Display — the URL
        // carries the API key in its query string and must never reach logs.
        let response = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "requests": requests }))
            .send()
            .await
            .map_err(|e| MemoryError::Embedding(e.without_url().to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| MemoryError::Embedding(e.without_url().to_string()))?;

        if !status.is_success() {
            return Err(MemoryError::Embedding(format!("{status}: {body}")));
        }

        let parsed: BatchEmbedResponse = serde_json::from_str(&body)
            .map_err(|e| MemoryError::Embedding(format!("invalid response body: {e}")))?;

        if parsed.embeddings.len() != texts.len() {
            return Err(MemoryError::Embedding(format!(
                "expected {} embeddings, got {}",
                texts.len(),
                parsed.embeddings.len()
            )));
        }

        for e in &parsed.embeddings {
            if e.values.len() != self.dimensions {
                return Err(MemoryError::Embedding(format!(
                    "expected {} dimensions, got {}",
                    self.dimensions,
                    e.values.len()
                )));
            }
        }

        Ok(parsed.embeddings.into_iter().map(|e| e.values).collect())
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

pub(crate) fn gemini_factory(
    settings: &HashMap<String, String>,
) -> Result<Arc<dyn Embedder>, MemoryError> {
    let model_name = require_setting(settings, "model_name", "gemini")?;
    let api_key = require_setting(settings, "api_key", "gemini")?;
    let dimensions = require_dimensions(settings, "gemini")?;
    let base_url = settings
        .get("base_url")
        .filter(|s| !s.is_empty())
        .cloned()
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
    Ok(Arc::new(GeminiEmbedder {
        client: reqwest::Client::new(),
        base_url,
        api_key,
        model_name,
        dimensions,
    }))
}
