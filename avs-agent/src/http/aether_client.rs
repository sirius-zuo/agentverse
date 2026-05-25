#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterResponse {
    pub instance_id: String,
    pub poll_interval_secs: u64,
}

#[derive(Debug)]
pub struct AetherRegistration {
    pub instance_id: String,
    /// Returned by the registry; reserved for a future polling-aware shutdown.
    #[allow(dead_code)]
    pub poll_interval_secs: u64,
}

#[derive(Debug, Clone)]
pub struct AetherClient {
    registry_url: String,
    agent_name: String,
    agent_http_url: String,
    capabilities: Vec<String>,
    client: reqwest::Client,
}

impl AetherClient {
    pub fn new(
        registry_url: impl Into<String>,
        agent_name: impl Into<String>,
        agent_http_url: impl Into<String>,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            registry_url: registry_url.into(),
            agent_name: agent_name.into(),
            agent_http_url: agent_http_url.into(),
            capabilities,
            client: reqwest::Client::new(),
        }
    }

    /// POST /registry/agents — returns registration on success, None if aether unreachable.
    pub async fn register(&self) -> Option<AetherRegistration> {
        #[derive(Serialize)]
        struct Req<'a> {
            name: &'a str,
            http_url: &'a str,
            capabilities: &'a [String],
        }

        let url = format!(
            "{}/registry/agents",
            self.registry_url.trim_end_matches('/')
        );
        match self
            .client
            .post(&url)
            .json(&Req {
                name: &self.agent_name,
                http_url: &self.agent_http_url,
                capabilities: &self.capabilities,
            })
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => match res.json::<RegisterResponse>().await {
                Ok(r) => {
                    tracing::info!(instance_id = %r.instance_id, "Registered with aether registry");
                    Some(AetherRegistration {
                        instance_id: r.instance_id,
                        poll_interval_secs: r.poll_interval_secs,
                    })
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to parse aether registration response");
                    None
                }
            },
            Ok(res) => {
                tracing::warn!(status = %res.status(), "Aether registration rejected");
                None
            }
            Err(e) => {
                tracing::warn!(error = %e, "Aether registry unreachable; running standalone");
                None
            }
        }
    }

    /// DELETE /registry/instances/{id} — best-effort.
    pub async fn deregister(&self, instance_id: &str) {
        let url = format!(
            "{}/registry/instances/{}",
            self.registry_url.trim_end_matches('/'),
            instance_id
        );
        if let Err(e) = self.client.delete(&url).send().await {
            tracing::warn!(error = %e, "Failed to deregister from aether (best-effort)");
        }
    }

    /// POST /registry/instances/{id}/events — fire-and-forget.
    /// Called from the /invoke error path once that wiring is added.
    #[allow(dead_code)]
    pub async fn push_event(
        &self,
        instance_id: &str,
        event_type: &str,
        payload: serde_json::Value,
    ) {
        let url = format!(
            "{}/registry/instances/{}/events",
            self.registry_url.trim_end_matches('/'),
            instance_id
        );
        let _ = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "event_type": event_type, "payload": payload }))
            .send()
            .await;
    }
}
