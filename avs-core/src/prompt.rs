use minijinja::Environment;
use serde_json::Value;
use std::collections::HashMap;

const DEFAULT_REACT_TEMPLATE: &str = r#"
You are a helpful assistant. Think step by step.

Current conversation:
{{ conversation }}

Available tools:
{{ tools }}

Respond in the following format:
Thought: [your reasoning]
Action: [tool name]
Action Input: [tool arguments as JSON]

Or if you have the final answer:
Thought: [your reasoning]
Answer: [final answer]
"#;

pub struct PromptRegistry {
    env: Environment<'static>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        let mut env = Environment::new();
        env.add_template("react", DEFAULT_REACT_TEMPLATE).unwrap();
        Self { env }
    }

    pub fn add_template(&mut self, name: &str, template: &str) {
        // Leak to get 'static lifetime — acceptable since templates are few
        let name: &'static str = Box::leak(name.to_string().into_boxed_str());
        let source: &'static str = Box::leak(template.to_string().into_boxed_str());
        self.env.add_template(name, source).unwrap();
    }

    pub fn render(&self, name: &str, context: HashMap<String, Value>) -> Result<String, String> {
        let tmpl = self.env.get_template(name).map_err(|e| e.to_string())?;
        // Build minijinja context from the HashMap
        let entries: Vec<(String, minijinja::value::Value)> = context
            .into_iter()
            .map(|(k, v)| (k, minijinja::value::Value::from_serialize(&v)))
            .collect();
        let ctx = minijinja::value::Value::from_iter(entries);
        tmpl.render(ctx).map_err(|e| e.to_string())
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}
