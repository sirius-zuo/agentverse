//! Shared cycle skeleton used by all orchestration strategies.
//!
//! Provides the fixed loop structure; each strategy only implements
//! its own `step()` logic via a closure.

use agentverse::{
    AgentError, ContentBlock, GenerateResponse, LlmRunner, PromptRegistry, ToolCall, ToolCallResult,
};
use agentverse_guardrails::check_output;
use agentverse_tools::ToolRegistry;
use serde_json::Value;
use std::sync::Arc;

/// The fixed cycle skeleton that all strategies share.
pub struct CycleSkeleton {
    pub runner: Arc<LlmRunner>,
    pub prompts: Arc<PromptRegistry>,
    pub tools: Arc<ToolRegistry>,
    max_iterations: usize,
}

/// The strategy's decision for the next action, derived directly from a
/// model response's structured content blocks. Native tool calling removes
/// the free-text ambiguity that used to require a `Continue`/nudge-retry
/// state and a separate single-call variant — a response either carries one
/// or more tool calls, is a plain-text final answer, or is a malformed
/// protocol violation.
#[derive(Debug)]
pub enum CycleAction {
    /// One or more tool calls, dispatched together (parallel-safe by
    /// construction — this also covers the single-call case).
    ToolCalls { calls: Vec<ToolCall> },
    /// A text-only response — the final answer.
    Done { answer: String },
    /// The response had neither a tool call nor any text — a protocol
    /// violation, not a retryable ambiguity.
    Error { message: String },
}

/// Derive the next action from a model response's content blocks.
///
/// `ToolUse` blocks take priority: if the model both narrates in `Text` and
/// calls tools in the same turn, the tools are still dispatched (mirrors the
/// old free-text parser's "tool calls override a hallucinated answer" rule,
/// now for free — there is no separate hallucination case once tool calls
/// are structurally distinct from prose).
pub fn action_from_response(response: &GenerateResponse) -> CycleAction {
    let mut calls = Vec::new();
    let mut text_parts = Vec::new();

    for block in &response.content {
        match block {
            ContentBlock::ToolUse { id, name, input } => {
                calls.push(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    args: input.clone(),
                });
            }
            ContentBlock::Text { text } => {
                if !text.trim().is_empty() {
                    text_parts.push(text.clone());
                }
            }
            ContentBlock::ToolResult { .. } => {}
        }
    }

    if !calls.is_empty() {
        CycleAction::ToolCalls { calls }
    } else if !text_parts.is_empty() {
        CycleAction::Done {
            answer: text_parts.join("\n"),
        }
    } else {
        CycleAction::Error {
            message: "Empty response from model".to_string(),
        }
    }
}

/// Restore tool results to the order their originating calls were made in,
/// backfilling a synthetic error result for any call `ToolRegistry::execute_many`
/// silently dropped (its completion loop discards a call's result entirely if
/// that call's task panicked). Without this, a panicking tool call among
/// several would leave the `Tool`-role message with fewer `ToolResult` blocks
/// than the assistant turn has `ToolUse` blocks — a wire-protocol violation
/// Anthropic hard-rejects and some OpenAI-compatible backends silently misalign on.
pub fn reconcile_and_order_results(
    results: Vec<ToolCallResult>,
    calls: &[(String, String)],
) -> Vec<ToolCallResult> {
    let mut by_id: std::collections::HashMap<String, ToolCallResult> =
        results.into_iter().map(|r| (r.id.clone(), r)).collect();
    calls
        .iter()
        .map(|(id, name)| {
            by_id.remove(id).unwrap_or_else(|| ToolCallResult {
                id: id.clone(),
                name: name.clone(),
                result: Err(agentverse::ToolError::Execution(
                    "tool execution failed (no result returned)".to_string(),
                )),
            })
        })
        .collect()
}

/// Convert executed tool results into `ToolResult` content blocks for a
/// `Tool`-role message, in the order given.
pub fn results_to_tool_result_blocks(results: Vec<ToolCallResult>) -> Vec<ContentBlock> {
    results
        .into_iter()
        .map(|r| {
            let (content, is_error) = match r.result {
                Ok(v) => (v.to_string(), false),
                Err(e) => (e.to_string(), true),
            };
            ContentBlock::ToolResult {
                tool_use_id: r.id,
                content,
                is_error,
            }
        })
        .collect()
}

impl CycleSkeleton {
    /// Create a new cycle skeleton.
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        max_iterations: usize,
    ) -> Self {
        Self {
            runner,
            prompts,
            tools,
            max_iterations,
        }
    }

    /// Execute a tool by name with the given arguments.
    pub async fn execute_tool(&self, tool_name: &str, args: Value) -> Result<String, AgentError> {
        let result = self
            .tools
            .execute(tool_name, args)
            .await
            .map_err(AgentError::Tool)?;
        Ok(result.to_string())
    }

    /// Optionally insert the ReAct preamble into a message buffer.
    ///
    /// If a `react.j2` template is registered, the rendered preamble
    /// (few-shot examples only — tool schemas are sent natively via
    /// `GenerateRequest.tools`, not as rendered text) is inserted before the
    /// first non-system message. When no template is present the buffer is
    /// returned unchanged.
    pub fn prepare_buffer(&self, messages: Vec<agentverse::Message>) -> Vec<agentverse::Message> {
        if !self.prompts.has_react_template() {
            return messages;
        }

        let mut ctx = std::collections::HashMap::new();
        if let Some(examples) = self.prompts.get_examples("react_examples") {
            if let Ok(val) = serde_json::to_value(examples) {
                ctx.insert("examples".to_string(), val);
            }
        }

        let mut buf = messages;
        if let Ok(preamble) = self.prompts.render("react", ctx) {
            if !preamble.trim().is_empty() {
                let insert_pos = buf
                    .iter()
                    .position(|m| !matches!(m.role, agentverse::MessageRole::System))
                    .unwrap_or(0);
                buf.insert(
                    insert_pos,
                    agentverse::Message::text(agentverse::MessageRole::User, preamble),
                );
            }
        }

        buf
    }

    /// Apply output guardrail to a model response.
    pub fn check_output_guardrail(&self, output: &str) -> Result<(), AgentError> {
        check_output(output).map_err(|e| match e {
            agentverse_guardrails::GuardrailError::OutputFiltered(msg) => {
                AgentError::Guardrail(agentverse::GuardrailError::OutputFiltered(msg))
            }
            agentverse_guardrails::GuardrailError::PromptInjection(msg) => {
                AgentError::Guardrail(agentverse::GuardrailError::PromptInjection(msg))
            }
            _ => AgentError::Guardrail(agentverse::GuardrailError::OutputFiltered(e.to_string())),
        })
    }

    /// Return the number of registered tools.
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    /// Execute multiple tool calls concurrently.
    pub async fn execute_many(
        &self,
        calls: Vec<ToolCall>,
    ) -> Result<Vec<ToolCallResult>, AgentError> {
        Ok(self.tools.execute_many(calls).await)
    }

    /// Return max iterations.
    pub fn max_iterations(&self) -> usize {
        self.max_iterations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::{Config, PromptRegistry, UsageStats};
    use agentverse_tools::ToolRegistry;
    use std::sync::Arc;

    fn make_skeleton() -> CycleSkeleton {
        let runner = Arc::new(
            LlmRunner::from_config(Config {
                provider: agentverse::ProviderConfig::openai(
                    "test".to_string(),
                    "sk-test".to_string(),
                    Some("http://127.0.0.1:1/v1".to_string()),
                ),
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            })
            .unwrap(),
        );
        CycleSkeleton::new(
            runner,
            Arc::new(PromptRegistry::new()),
            ToolRegistry::new(),
            5,
        )
    }

    #[test]
    fn skeleton_tool_count_has_find_tools() {
        let s = make_skeleton();
        // find_tools is auto-registered, so count is at least 1
        assert!(s.tool_count() >= 1);
    }

    #[test]
    fn skeleton_max_iterations() {
        let s = make_skeleton();
        assert_eq!(s.max_iterations(), 5);
    }

    #[test]
    fn skeleton_prepare_buffer_no_preamble() {
        let s = make_skeleton();
        let msgs = vec![agentverse::Message::text(
            agentverse::MessageRole::User,
            "hi",
        )];
        let buf = s.prepare_buffer(msgs);
        // Without a react prompt template, buffer is unchanged
        assert_eq!(buf.len(), 1);
    }

    fn response_with(content: Vec<ContentBlock>) -> GenerateResponse {
        GenerateResponse {
            content,
            usage: UsageStats::default(),
        }
    }

    #[test]
    fn action_from_response_single_tool_call() {
        let response = response_with(vec![ContentBlock::ToolUse {
            id: "call_1".to_string(),
            name: "search".to_string(),
            input: serde_json::json!({"q": "test"}),
        }]);
        match action_from_response(&response) {
            CycleAction::ToolCalls { calls } => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].id, "call_1");
                assert_eq!(calls[0].name, "search");
                assert_eq!(calls[0].args["q"], "test");
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn action_from_response_parallel_tool_calls_preserve_order() {
        let response = response_with(vec![
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "calculator".to_string(),
                input: serde_json::json!({}),
            },
            ContentBlock::ToolUse {
                id: "call_2".to_string(),
                name: "datetime".to_string(),
                input: serde_json::json!({}),
            },
        ]);
        match action_from_response(&response) {
            CycleAction::ToolCalls { calls } => {
                assert_eq!(calls.len(), 2);
                assert_eq!(calls[0].id, "call_1");
                assert_eq!(calls[1].id, "call_2");
            }
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn action_from_response_text_only_is_done() {
        let response = response_with(vec![ContentBlock::Text {
            text: "The answer is 42.".to_string(),
        }]);
        match action_from_response(&response) {
            CycleAction::Done { answer } => assert_eq!(answer, "The answer is 42."),
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[test]
    fn action_from_response_tool_calls_take_priority_over_text() {
        let response = response_with(vec![
            ContentBlock::Text {
                text: "Let me check.".to_string(),
            },
            ContentBlock::ToolUse {
                id: "call_1".to_string(),
                name: "search".to_string(),
                input: serde_json::json!({}),
            },
        ]);
        match action_from_response(&response) {
            CycleAction::ToolCalls { calls } => assert_eq!(calls.len(), 1),
            other => panic!("expected ToolCalls, got {other:?}"),
        }
    }

    #[test]
    fn action_from_response_empty_content_is_error() {
        let response = response_with(vec![]);
        match action_from_response(&response) {
            CycleAction::Error { message } => assert!(message.contains("Empty")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn action_from_response_blank_text_is_error() {
        let response = response_with(vec![ContentBlock::Text {
            text: "   ".to_string(),
        }]);
        match action_from_response(&response) {
            CycleAction::Error { message } => assert!(message.contains("Empty")),
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn reconcile_and_order_results_reorders_completion_order_results() {
        let calls = vec![
            ("call_a".to_string(), "double".to_string()),
            ("call_b".to_string(), "double".to_string()),
        ];
        let results = vec![
            ToolCallResult {
                id: "call_b".to_string(),
                name: "double".to_string(),
                result: Ok(serde_json::json!(4)),
            },
            ToolCallResult {
                id: "call_a".to_string(),
                name: "double".to_string(),
                result: Ok(serde_json::json!(2)),
            },
        ];
        let results = reconcile_and_order_results(results, &calls);
        assert_eq!(results[0].id, "call_a");
        assert_eq!(results[1].id, "call_b");
    }

    #[test]
    fn reconcile_and_order_results_backfills_dropped_call() {
        let calls = vec![
            ("call_a".to_string(), "double".to_string()),
            ("call_b".to_string(), "double".to_string()),
        ];
        // Simulates `ToolRegistry::execute_many` silently dropping call_b's
        // result (e.g. because its task panicked).
        let results = vec![ToolCallResult {
            id: "call_a".to_string(),
            name: "double".to_string(),
            result: Ok(serde_json::json!(2)),
        }];
        let results = reconcile_and_order_results(results, &calls);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "call_a");
        assert!(results[0].result.is_ok());
        assert_eq!(results[1].id, "call_b");
        match &results[1].result {
            Err(e) => assert!(
                e.to_string().contains("no result returned"),
                "expected error message to mention the call was not returned, got: {e}"
            ),
            Ok(_) => panic!("expected backfilled call_b to be an error result"),
        }
    }

    #[test]
    fn results_to_tool_result_blocks_preserves_order_and_marks_errors() {
        let results = vec![
            ToolCallResult {
                id: "call_a".to_string(),
                name: "double".to_string(),
                result: Ok(serde_json::json!(2)),
            },
            ToolCallResult {
                id: "call_b".to_string(),
                name: "missing".to_string(),
                result: Err(agentverse::ToolError::NotFound("missing".to_string())),
            },
        ];
        let blocks = results_to_tool_result_blocks(results);
        match &blocks[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                assert_eq!(tool_use_id, "call_a");
                assert_eq!(content, "2");
                assert!(!is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
        match &blocks[1] {
            ContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "call_b");
                assert!(is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}
