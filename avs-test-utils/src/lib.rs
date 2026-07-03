pub mod agent_builder;
pub mod session_conformance;

pub use agent_builder::{dead_endpoint_agent, unwrap_done};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dead_endpoint_agent_builds() {
        let agent = dead_endpoint_agent().await;
        // session creation works even when LLM is unreachable
        let session_id = agent.create_session("user1").await.unwrap();
        assert!(!session_id.to_string().is_empty());
    }
}
