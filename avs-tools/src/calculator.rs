use agentverse::{Tool, ToolResult};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema)]
pub struct CalculatorArgs {
    /// Arithmetic operation to perform
    pub operation: Operation,
    /// First operand
    pub a: f64,
    /// Second operand
    pub b: f64,
}

#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

pub struct Calculator;

#[async_trait::async_trait]
impl Tool for Calculator {
    type Args = CalculatorArgs;

    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Perform arithmetic calculations: add, subtract, multiply, divide"
    }

    async fn execute(&self, args: CalculatorArgs) -> ToolResult {
        let result = match args.operation {
            Operation::Add => args.a + args.b,
            Operation::Subtract => args.a - args.b,
            Operation::Multiply => args.a * args.b,
            Operation::Divide => {
                if args.b == 0.0 {
                    return Err(agentverse::ToolError::Execution(
                        "Division by zero".to_string(),
                    ));
                }
                args.a / args.b
            }
        };
        Ok(json!({ "result": result }))
    }
}
