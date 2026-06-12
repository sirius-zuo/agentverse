use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::error::ToolError;

pub type ToolResult = Result<Value, ToolError>;

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    type Args: JsonSchema + DeserializeOwned + Send;

    fn name(&self) -> &str;
    fn description(&self) -> &str;
    async fn execute(&self, args: Self::Args) -> ToolResult;
}

/// Object-safe erased version of Tool for dynamic dispatch in the registry.
/// Never implement this directly — the blanket impl handles it for any T: Tool.
#[async_trait::async_trait]
pub trait ErasedTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    /// Returns an Anthropic-compatible tool definition:
    /// `{ "name": "...", "description": "...", "input_schema": { ... } }`
    fn schema(&self) -> Value;
    async fn execute_raw(&self, args: Value) -> ToolResult;
}

#[async_trait::async_trait]
impl<T: Tool> ErasedTool for T {
    fn name(&self) -> &str {
        Tool::name(self)
    }

    fn description(&self) -> &str {
        Tool::description(self)
    }

    fn schema(&self) -> Value {
        let gen = schemars::gen::SchemaGenerator::default();
        let root = gen.into_root_schema_for::<<T as Tool>::Args>();
        serde_json::json!({
            "name": Tool::name(self),
            "description": Tool::description(self),
            "input_schema": serde_json::to_value(&root).unwrap_or(Value::Null),
        })
    }

    async fn execute_raw(&self, args: Value) -> ToolResult {
        let typed: T::Args =
            serde_json::from_value(args).map_err(|e| ToolError::InvalidArgs(e.to_string()))?;
        Tool::execute(self, typed).await
    }
}

/// A single tool invocation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub args: Value,
}

/// The result of a single tool invocation.
#[derive(Debug)]
pub struct ToolCallResult {
    pub name: String,
    pub result: ToolResult,
}

/// Handle for a fire-and-forget tool dispatch (future use).
pub struct ToolHandle {
    pub id: Uuid,
    pub receiver: oneshot::Receiver<ToolCallResult>,
}

#[cfg(test)]
mod trait_tests {
    use super::*;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Deserialize, JsonSchema)]
    struct EchoArgs {
        message: String,
    }

    struct EchoTool;

    #[async_trait::async_trait]
    impl Tool for EchoTool {
        type Args = EchoArgs;
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echo a message"
        }
        async fn execute(&self, args: EchoArgs) -> ToolResult {
            Ok(json!({ "echo": args.message }))
        }
    }

    #[tokio::test]
    async fn erased_tool_schema_contains_name() {
        let tool = EchoTool;
        let erased: &dyn ErasedTool = &tool;
        let schema = erased.schema();
        assert_eq!(schema["name"], "echo");
        assert!(schema["input_schema"].is_object());
    }

    #[tokio::test]
    async fn erased_tool_execute_raw_deserializes() {
        let tool = EchoTool;
        let erased: &dyn ErasedTool = &tool;
        let result = erased.execute_raw(json!({"message": "hi"})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn erased_tool_execute_raw_invalid_args() {
        let tool = EchoTool;
        let erased: &dyn ErasedTool = &tool;
        let result = erased.execute_raw(json!({"wrong_key": 42})).await;
        assert!(matches!(
            result,
            Err(crate::error::ToolError::InvalidArgs(_))
        ));
    }
}
