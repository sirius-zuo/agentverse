use agentverse::{AsyncTool, ToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Simple calculator tool for arithmetic operations.
pub struct Calculator;

#[async_trait]
impl AsyncTool for Calculator {
    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Perform arithmetic calculations: add, subtract, multiply, divide"
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["add", "subtract", "multiply", "divide"],
                    "description": "Arithmetic operation"
                },
                "a": {
                    "type": "number",
                    "description": "First operand"
                },
                "b": {
                    "type": "number",
                    "description": "Second operand"
                }
            },
            "required": ["operation", "a", "b"]
        })
    }

    async fn execute(&self, args: Value) -> ToolResult {
        let op = args["operation"]
            .as_str()
            .ok_or_else(|| agentverse::ToolError::Execution("Missing 'operation'".to_string()))?;

        let a = args["a"].as_f64().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'a'".to_string())
        })?;

        let b = args["b"].as_f64().ok_or_else(|| {
            agentverse::ToolError::Execution("Missing or invalid 'b'".to_string())
        })?;

        let result = match op {
            "add" => a + b,
            "subtract" => a - b,
            "multiply" => a * b,
            "divide" => {
                if b == 0.0 {
                    return Err(agentverse::ToolError::Execution(
                        "Division by zero".to_string(),
                    ));
                }
                a / b
            }
            other => {
                return Err(agentverse::ToolError::Execution(format!(
                    "Unknown operation: {}",
                    other
                )))
            }
        };

        Ok(json!({ "result": result }))
    }
}
