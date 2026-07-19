use agentverse::{Tool, ToolCall, ToolResult};
use agentverse_tools::{ToolOptions, ToolRegistry};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Deserialize, JsonSchema)]
struct AddArgs {
    a: f64,
    b: f64,
}

struct AddTool;

#[async_trait::async_trait]
impl Tool for AddTool {
    type Args = AddArgs;
    fn name(&self) -> &str {
        "add"
    }
    fn description(&self) -> &str {
        "Add two numbers"
    }
    async fn execute(&self, args: AddArgs) -> ToolResult {
        Ok(json!({ "result": args.a + args.b }))
    }
}

struct ReplacementAddTool;

#[async_trait::async_trait]
impl Tool for ReplacementAddTool {
    type Args = AddArgs;
    fn name(&self) -> &str {
        "add"
    }
    fn description(&self) -> &str {
        "Replacement add implementation"
    }
    async fn execute(&self, args: AddArgs) -> ToolResult {
        Ok(json!({ "result": args.a + args.b + 1.0 }))
    }
}

#[derive(Deserialize, JsonSchema)]
struct MulArgs {
    a: f64,
    b: f64,
}

struct MulTool;

#[async_trait::async_trait]
impl Tool for MulTool {
    type Args = MulArgs;
    fn name(&self) -> &str {
        "mul"
    }
    fn description(&self) -> &str {
        "Multiply two numbers"
    }
    async fn execute(&self, args: MulArgs) -> ToolResult {
        Ok(json!({ "result": args.a * args.b }))
    }
}

#[test]
fn registry_new_returns_arc() {
    let r: Arc<ToolRegistry> = ToolRegistry::new();
    // find_tools is auto-registered
    assert!(r.has_tool("find_tools"));
}

#[test]
fn register_and_has_tool() {
    let r = ToolRegistry::new();
    r.register(AddTool);
    assert!(r.has_tool("add"));
}

#[test]
fn register_if_absent_is_atomic_under_concurrency() {
    use std::sync::Barrier;
    use std::thread;

    const WORKERS: usize = 16;
    let registry = ToolRegistry::new();
    let barrier = Arc::new(Barrier::new(WORKERS));
    let handles = (0..WORKERS)
        .map(|_| {
            let registry = Arc::clone(&registry);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                registry.register_if_absent(AddTool)
            })
        })
        .collect::<Vec<_>>();

    let inserted = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|inserted| *inserted)
        .count();
    assert_eq!(inserted, 1);
    assert_eq!(
        registry
            .tool_names()
            .iter()
            .filter(|name| name.as_str() == "add")
            .count(),
        1
    );
    assert_eq!(
        registry
            .search("add", 10)
            .iter()
            .filter(|tool| tool.name == "add")
            .count(),
        1
    );
}

#[tokio::test]
async fn register_keeps_replacement_semantics() {
    let registry = ToolRegistry::new();
    registry.register(AddTool);
    registry.register(ReplacementAddTool);

    let result = registry
        .execute("add", json!({"a": 2.0, "b": 3.0}))
        .await
        .unwrap();
    assert_eq!(result["result"], 6.0);
}

#[test]
fn schema_contains_registered_tools() {
    let r = ToolRegistry::new();
    r.register(AddTool);
    let schemas = r.schema();
    assert!(schemas.iter().any(|s| s["name"] == "add"));
}

#[tokio::test]
async fn execute_known_tool() {
    let r = ToolRegistry::new();
    r.register(AddTool);
    let result = r.execute("add", json!({"a": 3.0, "b": 4.0})).await.unwrap();
    assert_eq!(result["result"], 7.0);
}

#[tokio::test]
async fn execute_unknown_tool_returns_not_found() {
    let r = ToolRegistry::new();
    let result = r.execute("nope", json!({})).await;
    assert!(matches!(result, Err(agentverse::ToolError::NotFound(_))));
}

#[tokio::test]
async fn execute_many_runs_in_parallel() {
    use std::time::{Duration, Instant};

    #[derive(Deserialize, JsonSchema)]
    struct SlowArgs {}

    struct SlowTool {
        millis: u64,
        tool_name: &'static str,
    }

    #[async_trait::async_trait]
    impl Tool for SlowTool {
        type Args = SlowArgs;
        fn name(&self) -> &str {
            self.tool_name
        }
        fn description(&self) -> &str {
            "slow"
        }
        async fn execute(&self, _: SlowArgs) -> ToolResult {
            tokio::time::sleep(Duration::from_millis(self.millis)).await;
            Ok(json!({"done": true}))
        }
    }

    let r = ToolRegistry::new();
    r.register(SlowTool {
        millis: 100,
        tool_name: "slow_a",
    });
    r.register(SlowTool {
        millis: 100,
        tool_name: "slow_b",
    });

    let calls = vec![
        ToolCall {
            name: "slow_a".into(),
            args: json!({}),
        },
        ToolCall {
            name: "slow_b".into(),
            args: json!({}),
        },
    ];

    let start = Instant::now();
    let results = r.execute_many(calls).await;
    let elapsed = start.elapsed();

    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.result.is_ok()));
    // Should complete in ~100ms (parallel), not ~200ms (sequential)
    assert!(
        elapsed < Duration::from_millis(180),
        "elapsed: {:?}",
        elapsed
    );
}

#[test]
fn filter_category_returns_subset() {
    let r = ToolRegistry::new();
    r.register_with_options(
        AddTool,
        ToolOptions {
            category: Some("math".into()),
            ..Default::default()
        },
    );
    r.register_with_options(
        MulTool,
        ToolOptions {
            category: Some("math".into()),
            ..Default::default()
        },
    );
    let math = r.filter_category("math");
    assert!(math.has_tool("add"));
    assert!(math.has_tool("mul"));
}

#[tokio::test]
async fn execute_many_hitl_intercepts_blocked_tool() {
    use agentverse::hitl::HitlHook;
    use std::sync::Arc;

    struct BlockAll;
    #[async_trait::async_trait]
    impl HitlHook for BlockAll {
        async fn check_tool(&self, _: &str, _: &serde_json::Value) -> Option<(uuid::Uuid, String)> {
            Some((uuid::Uuid::new_v4(), "{}".to_string()))
        }
    }

    let registry = ToolRegistry::new();
    let hook: Arc<dyn HitlHook> = Arc::new(BlockAll);
    let calls = vec![ToolCall {
        name: "find_tools".to_string(),
        args: json!({}),
    }];
    let result = registry.execute_many_hitl(calls, &hook).await;
    assert!(result.is_err(), "intercepted call must return Err");
}

#[tokio::test]
async fn execute_many_hitl_returns_kind_json_on_intercept() {
    use agentverse::hitl::HitlHook;
    use std::sync::Arc;

    struct BlockAll;
    #[async_trait::async_trait]
    impl HitlHook for BlockAll {
        async fn check_tool(&self, _: &str, _: &serde_json::Value) -> Option<(uuid::Uuid, String)> {
            Some((
                uuid::Uuid::new_v4(),
                r#"{"type":"ToolApproval"}"#.to_string(),
            ))
        }
    }

    let registry = ToolRegistry::new();
    let hook: Arc<dyn HitlHook> = Arc::new(BlockAll);
    let calls = vec![ToolCall {
        name: "find_tools".to_string(),
        args: json!({}),
    }];
    let result = registry.execute_many_hitl(calls, &hook).await;
    let err = result.unwrap_err();
    assert!(
        !err.kind_json.is_empty(),
        "kind_json must be forwarded from hook"
    );
}
