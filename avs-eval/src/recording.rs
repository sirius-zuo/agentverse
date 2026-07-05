use httpmock::prelude::*;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct RecordedTurn {
    #[serde(default)]
    pub body_contains: String,
    pub content: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Recording {
    #[serde(default)]
    pub agent_turns: Vec<RecordedTurn>,
    pub judge_turn: RecordedTurn,
}

/// Loads `fixtures/recordings/<case_name>.toml`. Panics with the file path
/// on any read/parse failure — a broken recording file is a hard error.
pub fn load_recording(case_name: &str) -> Recording {
    let path = format!("fixtures/recordings/{case_name}.toml");
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read recording {path}: {e}"));
    toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse recording {path}: {e}"))
}

fn chat_completion_envelope(content: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": content}}],
        "usage": {"prompt_tokens": 5, "completion_tokens": 3, "total_tokens": 8}
    })
}

/// Registers one `httpmock` mock per `agent_turns` entry against `server`,
/// each matched by that entry's `body_contains` substring, registered in
/// list order.
///
/// Prefer picking `body_contains` substrings that are truly mutually
/// exclusive across turns. That isn't always possible in a multi-turn
/// conversation, though: later turns' requests carry the full cumulative
/// message history, so an earlier turn's marker can still be present in a
/// later turn's request (e.g. the original user message persists across an
/// entire ReAct loop). When two turns' markers both match a given request,
/// `httpmock` resolves the tie by registration order — the first-registered
/// matching mock wins (verified against `httpmock` 0.7's `BTreeMap`-backed,
/// insertion-ordered mock table). In that situation, order this function's
/// *caller* registers the more-specific/later-appearing marker first in the
/// recording file's `agent_turns` list, so it's checked (and matches) before
/// a broader earlier-turn marker would. This is an internal `httpmock`
/// implementation detail, not a documented public guarantee — a future
/// `httpmock` upgrade could change it silently, so treat truly disjoint
/// substrings as the safer choice whenever the conversation shape allows it.
pub async fn register_agent_turns(server: &MockServer, recording: &Recording) {
    for turn in &recording.agent_turns {
        let body_contains = turn.body_contains.clone();
        let content = turn.content.clone();
        server
            .mock_async(move |when, then| {
                when.method(POST)
                    .path("/chat/completions")
                    .body_contains(body_contains.clone());
                then.status(200)
                    .json_body(chat_completion_envelope(&content));
            })
            .await;
    }
}

/// Registers the judge's single recorded response against `judge_server`
/// (a separate `MockServer` from the agent-under-test's — the judge is a
/// distinct model call with its own connection). Unconditional match: a
/// judge case makes exactly one judge call, so no `body_contains` filter
/// is needed.
pub async fn register_judge_turn(judge_server: &MockServer, recording: &Recording) {
    let content = recording.judge_turn.content.clone();
    judge_server
        .mock_async(move |when, then| {
            when.method(POST).path("/chat/completions");
            then.status(200)
                .json_body(chat_completion_envelope(&content));
        })
        .await;
}
