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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;

    fn make_client(base_url: &str) -> AetherClient {
        AetherClient::new(
            base_url,
            "test-agent",
            "http://127.0.0.1:8080",
            vec!["chat".to_string()],
        )
    }

    #[tokio::test]
    async fn register_returns_registration_on_success() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method("POST").path("/registry/agents");
            then.status(200).json_body(serde_json::json!({
                "instance_id": "test-uuid",
                "poll_interval_secs": 30
            }));
        });
        let client = make_client(&server.base_url());
        let reg = client.register().await;
        assert!(reg.is_some());
        let r = reg.unwrap();
        assert_eq!(r.instance_id, "test-uuid");
        assert_eq!(r.poll_interval_secs, 30);
    }

    #[tokio::test]
    async fn register_returns_none_when_aether_unreachable() {
        let client = make_client("http://127.0.0.1:1");
        assert!(client.register().await.is_none());
    }

    #[tokio::test]
    async fn register_returns_none_on_server_error() {
        let server = MockServer::start();
        let _mock = server.mock(|when, then| {
            when.method("POST").path("/registry/agents");
            then.status(500).body("error");
        });
        let client = make_client(&server.base_url());
        assert!(client.register().await.is_none());
    }

    #[tokio::test]
    async fn deregister_does_not_panic_on_error() {
        let client = make_client("http://127.0.0.1:1");
        client.deregister("any-id").await; // must not panic
    }

    #[tokio::test]
    async fn push_event_is_fire_and_forget() {
        let server = MockServer::start();
        let mock = server.mock(|when, then| {
            when.method("POST")
                .path("/registry/instances/inst-1/events");
            then.status(202);
        });
        let client = make_client(&server.base_url());
        client
            .push_event("inst-1", "error", serde_json::json!({"msg": "oops"}))
            .await;
        mock.assert();
    }
}
