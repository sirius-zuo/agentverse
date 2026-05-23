//! Tests for the agentverse-react crate.
//!
//! NOTE: Tests that call strategy.run() are marked #[ignore] because
//! ProviderWrapper::generate() is a stub returning an error until
//! ConnectionManager is implemented in Task 2.

use agentverse::{
    AsyncTool, GenerateRequest, GenerateResponse, Memory, MemoryError, Message, ModelProvider,
    ModelError, PromptConfig, PromptRegistry, ProviderWrapper, UsageStats,
};
use agentverse_react::{parse::parse_response, CycleAction, CycleSkeleton, ReActStrategy};
use agentverse_tools::{Calculator, ToolRegistry};
use async_trait::async_trait;
use reqwest::header::HeaderMap;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

// ─── Mock types ───────────────────────────────────────────────────────────────

/// Minimal in-test memory that satisfies the Memory trait.
struct TestMemory {
    messages: Vec<Message>,
    max_messages: usize,
    pinned: Vec<Message>,
}

impl TestMemory {
    fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::new(),
            max_messages,
            pinned: Vec::new(),
        }
    }
}

#[async_trait]
impl Memory for TestMemory {
    fn append(&mut self, message: Message) {
        self.messages.push(message);
        if self.messages.len() > self.max_messages {
            self.messages
                .drain(0..self.messages.len() - self.max_messages);
        }
    }

    async fn last_n(&mut self, n: usize) -> Result<Vec<Message>, MemoryError> {
        let mut all: Vec<Message> = self.pinned.clone();
        all.extend(self.messages.clone());
        let start = all.len().saturating_sub(n);
        Ok(all[start..].to_vec())
    }

    fn pin(&mut self, messages: Vec<Message>) {
        self.pinned.extend(messages);
    }

    async fn prime_from_long_term(
        &mut self,
        _query: &str,
        _top_k: usize,
    ) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), MemoryError> {
        Ok(())
    }

    fn clear(&mut self) {
        self.messages.clear();
        self.pinned.clear();
    }
}

/// A mock model that implements the new 4-method ModelProvider trait.
/// The responses field is unused at the ProviderWrapper level (generate() is stubbed),
/// but is kept for future use when ConnectionManager replaces the stub.
struct MockModel {
    #[allow(dead_code)]
    responses: Vec<String>,
    #[allow(dead_code)]
    index: AtomicUsize,
}

impl ModelProvider for MockModel {
    fn name(&self) -> &str {
        "mock-model"
    }

    fn build_request(
        &self,
        _model: &str,
        _request: GenerateRequest,
    ) -> Result<serde_json::Value, ModelError> {
        Ok(json!({}))
    }

    fn parse_response(&self, _body: &str) -> Result<GenerateResponse, ModelError> {
        let idx = self.index.fetch_add(1, Ordering::SeqCst) % self.responses.len().max(1);
        let content = self
            .responses
            .get(idx)
            .cloned()
            .unwrap_or_default();
        Ok(GenerateResponse {
            content,
            usage: UsageStats::default(),
        })
    }

    fn request_headers(&self, _api_key: &str) -> HeaderMap {
        HeaderMap::new()
    }

    fn endpoint_path(&self, _model: &str) -> String {
        "/mock".to_string()
    }
}

fn make_wrapper(responses: Vec<String>) -> ProviderWrapper {
    ProviderWrapper::new(MockModel {
        responses,
        index: AtomicUsize::new(0),
    })
}

/// A mock async tool for testing.
struct MockTool {
    name: String,
    description: String,
}

impl MockTool {
    fn new(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
        }
    }
}

#[async_trait]
impl AsyncTool for MockTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }

    async fn execute(
        &self,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, agentverse::ToolError> {
        Ok(json!({"result": format!("Executed {} with args {:?}", self.name, args)}))
    }
}

fn mock_skeleton(
    responses: Vec<String>,
    tools: ToolRegistry,
    max_iter: usize,
) -> CycleSkeleton<TestMemory> {
    CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(make_wrapper(responses)),
        tools,
        Arc::new(Mutex::new(TestMemory::new(10))),
        max_iter,
    )
}

fn mock_strategy(
    responses: Vec<String>,
    tools: ToolRegistry,
    max_iter: usize,
) -> ReActStrategy<TestMemory> {
    ReActStrategy::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(make_wrapper(responses)),
        tools,
        Arc::new(Mutex::new(TestMemory::new(10))),
        max_iter,
    )
}

fn empty_registry() -> ToolRegistry {
    ToolRegistry::new()
}

// ─── Parse tests ──────────────────────────────────────────────────────────────

#[test]
fn test_parse_response_answer() {
    let result = parse_response("Thought: done.\nAnswer: Hello world");
    match result {
        CycleAction::Done { answer } => assert_eq!(answer, "Hello world"),
        other => panic!("Expected Done, got {:?}", other),
    }
}

#[test]
fn test_parse_response_tool_call() {
    let result =
        parse_response("Thought: searching.\nAction: search\nAction Input: {\"q\": \"test\"}");
    match result {
        CycleAction::ToolCall { tool_name, args } => {
            assert_eq!(tool_name, "search");
            assert_eq!(args["q"], "test");
        }
        other => panic!("Expected ToolCall, got {:?}", other),
    }
}

#[test]
fn test_parse_response_thought_only() {
    let result = parse_response("I need to think about this first");
    match result {
        CycleAction::Continue { thought } => {
            assert_eq!(thought, "I need to think about this first");
        }
        other => panic!("Expected Continue, got {:?}", other),
    }
}

// ─── Cycle skeleton tests ─────────────────────────────────────────────────────

#[test]
fn test_cycle_skeleton_tool_count() {
    let mut registry = ToolRegistry::new();
    registry.register(MockTool::new("test_tool", "A test tool"));
    let skeleton = mock_skeleton(vec!["Answer: done".to_string()], registry, 10);
    assert_eq!(skeleton.tool_count(), 1);
    assert_eq!(skeleton.max_iterations(), 10);
    assert_eq!(skeleton.current_iteration(), 0);
}

#[tokio::test]
async fn test_cycle_skeleton_execute_tool() {
    let mut registry = ToolRegistry::new();
    registry.register(MockTool::new("echo", "Echo back input"));
    let skeleton = mock_skeleton(vec!["Answer: done".to_string()], registry, 10);

    let result = skeleton
        .execute_tool("echo", json!({"text": "hello"}))
        .await
        .unwrap();
    assert!(result.contains("Executed echo"));
}

#[tokio::test]
async fn test_cycle_skeleton_execute_tool_not_found() {
    let mut registry = ToolRegistry::new();
    registry.register(MockTool::new("echo", "Echo back input"));
    let skeleton = mock_skeleton(vec!["Answer: done".to_string()], registry, 10);

    let result = skeleton.execute_tool("missing_tool", json!({})).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_cycle_skeleton_build_request() {
    let memory = Arc::new(Mutex::new(TestMemory::new(10)));
    memory.lock().await.append(Message {
        role: agentverse::memory::MessageRole::User,
        content: "Hello".to_string(),
    });

    let skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(make_wrapper(vec!["Answer: done".to_string()])),
        empty_registry(),
        Arc::clone(&memory),
        10,
    );

    let request = skeleton.build_request().await.unwrap();
    assert!(request.messages.iter().any(|m| m.content.contains("Hello")));
}

// ─── ReActStrategy::run() tests — disabled pending ConnectionManager (Task 2) ──

#[tokio::test]
#[ignore = "ProviderWrapper::generate() is a stub; re-enable after ConnectionManager (Task 2)"]
async fn test_react_strategy_direct_answer() {
    let mut strategy = mock_strategy(vec!["Answer: 42".to_string()], empty_registry(), 10);
    let result = strategy.run("What is 6*7?".to_string()).await.unwrap();
    assert_eq!(result.answer, "42");
}

#[tokio::test]
#[ignore = "ProviderWrapper::generate() is a stub; re-enable after ConnectionManager (Task 2)"]
async fn test_react_strategy_thought_then_answer() {
    let mut strategy = mock_strategy(
        vec![
            "Thought: let me think.".to_string(),
            "Answer: 42".to_string(),
        ],
        empty_registry(),
        10,
    );
    let result = strategy.run("What is 6*7?".to_string()).await.unwrap();
    assert_eq!(result.answer, "42");
}

#[tokio::test]
#[ignore = "ProviderWrapper::generate() is a stub; re-enable after ConnectionManager (Task 2)"]
async fn test_react_strategy_tool_call_then_answer() {
    let mut registry = ToolRegistry::new();
    registry.register(MockTool::new("echo", "Echo back input"));
    let mut strategy = mock_strategy(
        vec![
            "Thought: use tool.\nAction: echo\nAction Input: {}".to_string(),
            "Answer: done".to_string(),
        ],
        registry,
        10,
    );
    let result = strategy.run("use echo tool".to_string()).await.unwrap();
    assert_eq!(result.answer, "done");
}

#[tokio::test]
#[ignore = "ProviderWrapper::generate() is a stub; re-enable after ConnectionManager (Task 2)"]
async fn test_react_strategy_max_iterations() {
    let mut strategy = mock_strategy(
        vec!["Thought: still thinking.".to_string()],
        empty_registry(),
        1,
    );
    let err = strategy.run("infinite loop".to_string()).await.unwrap_err();
    assert!(err.to_string().contains("Max iterations"));
}

#[tokio::test]
#[ignore = "ProviderWrapper::generate() is a stub; re-enable after ConnectionManager (Task 2)"]
async fn test_react_strategy_empty_response_is_error() {
    let mut strategy = mock_strategy(vec!["".to_string()], empty_registry(), 5);
    let err = strategy.run("test".to_string()).await.unwrap_err();
    assert!(err.to_string().contains("Empty response"));
}

#[tokio::test]
#[ignore = "ProviderWrapper::generate() is a stub; re-enable after ConnectionManager (Task 2)"]
async fn test_react_strategy_usage_accumulates() {
    let mut strategy = mock_strategy(vec!["Answer: done".to_string()], empty_registry(), 5);
    let result = strategy.run("test".to_string()).await.unwrap();
    assert_eq!(result.total_usage.input_tokens, 0);
}

// ─── CycleSkeleton preamble and guardrail tests ────────────────────────────────

#[tokio::test]
async fn test_cycle_no_preamble_without_react_template() {
    let memory = Arc::new(Mutex::new(TestMemory::new(10)));
    let mut skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(make_wrapper(vec!["Answer: done".to_string()])),
        empty_registry(),
        Arc::clone(&memory),
        10,
    );
    skeleton.prime_react_preamble().await;
    assert!(!skeleton.is_react_primed());
    assert_eq!(memory.lock().await.last_n(20).await.unwrap().len(), 0);
}

#[tokio::test]
async fn test_cycle_preamble_inserted_when_react_template_loaded() {
    let dir = tempfile::tempdir().unwrap();
    let react_path = dir.path().join("react.j2");
    std::fs::write(&react_path, "Tools: {{ tools }}\nUse ReAct format.").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    assert!(registry.has_react_template());

    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(MockTool::new("test", "A test tool"));

    let memory = Arc::new(Mutex::new(TestMemory::new(10)));
    let mut skeleton = CycleSkeleton::new(
        Arc::new(registry),
        Arc::new(make_wrapper(vec!["Answer: done".to_string()])),
        tool_registry,
        Arc::clone(&memory),
        10,
    );

    skeleton.prime_react_preamble().await;
    assert!(skeleton.is_react_primed());

    let messages = memory.lock().await.last_n(20).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].content.contains("Tools:"));
}

#[tokio::test]
async fn test_cycle_preamble_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("react.j2"), "Tools: {{ tools }}").unwrap();

    let registry = PromptRegistry::from_config(&PromptConfig {
        prompts_dir: Some(dir.path().to_str().unwrap().to_string()),
        ..Default::default()
    })
    .unwrap();

    let memory = Arc::new(Mutex::new(TestMemory::new(10)));
    let mut skeleton = CycleSkeleton::new(
        Arc::new(registry),
        Arc::new(make_wrapper(vec!["Answer: done".to_string()])),
        empty_registry(),
        Arc::clone(&memory),
        10,
    );

    skeleton.prime_react_preamble().await;
    skeleton.prime_react_preamble().await; // second call must be a no-op
    assert_eq!(memory.lock().await.last_n(20).await.unwrap().len(), 1);
}

#[test]
fn test_cycle_accumulate_usage() {
    use agentverse::UsageStats;
    let mut skeleton = mock_skeleton(vec![], empty_registry(), 5);
    skeleton.accumulate_usage(UsageStats {
        input_tokens: 10,
        output_tokens: 5,
        ..Default::default()
    });
    skeleton.accumulate_usage(UsageStats {
        input_tokens: 20,
        output_tokens: 3,
        ..Default::default()
    });
    assert_eq!(skeleton.total_usage().input_tokens, 30);
    assert_eq!(skeleton.total_usage().output_tokens, 8);
}

#[test]
fn test_cycle_check_output_guardrail_clean() {
    let skeleton = mock_skeleton(vec![], empty_registry(), 5);
    assert!(skeleton
        .check_output_guardrail("This is a clean response.")
        .is_ok());
}

#[tokio::test]
async fn test_cycle_build_request_with_guardrails_clean() {
    let skeleton = mock_skeleton(vec![], empty_registry(), 5);
    assert!(skeleton.build_request_with_guardrails().await.is_ok());
}

#[tokio::test]
async fn test_cycle_build_tools_str_with_parameters() {
    struct ParamTool;

    #[async_trait]
    impl agentverse::AsyncTool for ParamTool {
        fn name(&self) -> &str {
            "param_tool"
        }
        fn description(&self) -> &str {
            "Tool with params"
        }
        fn parameters(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search query"},
                    "limit": {"type": "integer", "description": "Max results"}
                },
                "required": ["query"]
            })
        }
        async fn execute(
            &self,
            _args: serde_json::Value,
        ) -> Result<serde_json::Value, agentverse::ToolError> {
            Ok(json!({}))
        }
    }

    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(ParamTool);

    let skeleton = CycleSkeleton::new(
        Arc::new(PromptRegistry::new()),
        Arc::new(make_wrapper(vec![])),
        tool_registry,
        Arc::new(Mutex::new(TestMemory::new(10))),
        5,
    );
    let req = skeleton.build_request().await.unwrap();
    let system = req.system.unwrap();
    assert!(system.contains("query"));
    assert!(system.contains("required"));
}

// ─── New test: ToolRegistry with Calculator ───────────────────────────────────

#[tokio::test]
async fn test_cycle_skeleton_execute_calculator_via_registry() {
    let mut registry = ToolRegistry::new();
    registry.register(Calculator);

    let skeleton = mock_skeleton(vec!["Answer: done".to_string()], registry, 10);

    let result = skeleton
        .execute_tool(
            "calculator",
            json!({"operation": "add", "a": 3.0, "b": 4.0}),
        )
        .await;

    assert!(result.is_ok(), "expected Ok, got {:?}", result);
    let output = result.unwrap();
    assert!(
        output.contains("7"),
        "expected result 7 in output: {}",
        output
    );
}
