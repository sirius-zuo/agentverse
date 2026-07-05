use agentverse::{ConnectionManager, ProviderConfig, ProviderRegistry};

#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Debug, serde::Deserialize)]
pub struct JudgeVerdict {
    pub verdict: Verdict,
    pub reasoning: String,
}

/// Fixed judge prompt template — not user-configurable. Instructs the judge
/// to answer with strict JSON so `parse_judge_verdict` can deserialize it.
pub fn build_judge_prompt(rubric: &str, agent_output: &str) -> String {
    format!(
        "You are evaluating an AI agent's response against a rubric.\n\n\
         Rubric: {rubric}\n\n\
         Agent's response: {agent_output}\n\n\
         Respond with ONLY a JSON object of the exact shape: \
         {{\"verdict\": \"pass\" or \"fail\", \"reasoning\": \"<one sentence>\"}}"
    )
}

/// Parses a judge model's raw text response into a `JudgeVerdict`.
/// A parse failure is surfaced as an `Err` — callers must treat this as a
/// hard test failure, never a silent pass.
pub fn parse_judge_verdict(raw_response: &str) -> Result<JudgeVerdict, String> {
    serde_json::from_str(raw_response.trim())
        .map_err(|e| format!("failed to parse judge verdict from {raw_response:?}: {e}"))
}

/// Builds a `ConnectionManager` for the judge model via the `ProviderRegistry`
/// (never hardcoded to one provider), pointed at `judge_server_base_url`.
pub fn build_judge_connection(
    judge_server_base_url: &str,
    model_name: &str,
    api_key: &str,
) -> Result<ConnectionManager, agentverse::error::ModelError> {
    let registry = ProviderRegistry::with_builtins();
    let config = ProviderConfig::openai(
        model_name.to_string(),
        api_key.to_string(),
        Some(judge_server_base_url.to_string()),
    );
    ConnectionManager::from_config(config, &registry)
}

/// Runs the judge: builds the prompt, invokes the (mocked) judge connection,
/// and parses the verdict.
pub async fn run_judge(
    connection: &ConnectionManager,
    rubric: &str,
    agent_output: &str,
) -> Result<JudgeVerdict, String> {
    let prompt = build_judge_prompt(rubric, agent_output);
    let request = agentverse::GenerateRequest {
        system: None,
        messages: vec![agentverse::Message {
            role: agentverse::MessageRole::User,
            content: prompt,
        }],
        tools: None,
        ..Default::default()
    };
    let response = connection
        .generate(request)
        .await
        .map_err(|e| format!("judge model call failed: {e}"))?;
    parse_judge_verdict(&response.content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::{load_recording, register_agent_turns, register_judge_turn};
    use httpmock::prelude::*;

    #[test]
    fn build_judge_prompt_embeds_rubric_and_output() {
        let prompt = build_judge_prompt("must be polite", "Sure, happy to help!");
        assert!(prompt.contains("must be polite"));
        assert!(prompt.contains("Sure, happy to help!"));
    }

    #[test]
    fn parse_judge_verdict_pass() {
        let verdict =
            parse_judge_verdict(r#"{"verdict": "pass", "reasoning": "polite and correct"}"#)
                .unwrap();
        assert_eq!(verdict.verdict, Verdict::Pass);
        assert_eq!(verdict.reasoning, "polite and correct");
    }

    #[test]
    fn parse_judge_verdict_fail() {
        let verdict = parse_judge_verdict(r#"{"verdict": "fail", "reasoning": "rude"}"#).unwrap();
        assert_eq!(verdict.verdict, Verdict::Fail);
    }

    #[test]
    fn parse_judge_verdict_malformed_is_hard_error() {
        let result = parse_judge_verdict("not json at all");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn recording_loader_drives_judge_end_to_end() {
        let recording = load_recording("_infrastructure_smoke_test");

        let agent_server = MockServer::start_async().await;
        register_agent_turns(&agent_server, &recording).await;
        // Prove the loaded mock actually answers a matching request, using
        // the raw connection machinery directly (no strategy needed for
        // this infrastructure-only smoke test).
        let registry = agentverse::ProviderRegistry::with_builtins();
        let config = agentverse::ProviderConfig::openai(
            "test-model".to_string(),
            "sk-test".to_string(),
            Some(agent_server.base_url()),
        );
        let connection = agentverse::ConnectionManager::from_config(config, &registry).unwrap();
        let response = connection
            .generate(agentverse::GenerateRequest {
                system: None,
                messages: vec![agentverse::Message {
                    role: agentverse::MessageRole::User,
                    content: "hello".to_string(),
                }],
                tools: None,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(response.content, "Thought: done.\nAnswer: hi there");

        let judge_server = MockServer::start_async().await;
        register_judge_turn(&judge_server, &recording).await;
        let judge_connection =
            build_judge_connection(&judge_server.base_url(), "judge-model", "test-key").unwrap();
        let verdict = run_judge(&judge_connection, "must be correct", "2+2=4")
            .await
            .unwrap();
        assert_eq!(verdict.verdict, Verdict::Pass);
    }
}
