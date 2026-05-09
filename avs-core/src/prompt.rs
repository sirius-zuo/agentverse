use minijinja::{context, Environment};
use std::collections::HashMap;

const DEFAULT_REACT_TEMPLATE: &str = r#"
You are a helpful assistant. Think step by step.

Current conversation:
{% for message in conversation %}
{{ message.role }}: {{ message.content }}
{% endfor %}

Available tools:
{% for tool in tools %}
- {{ tool.name }}: {{ tool.description }}
{% endfor %}

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

    pub fn render(
        &self,
        name: &str,
        context: HashMap<String, String>,
    ) -> Result<String, String> {
        let tmpl = self.env.get_template(name).map_err(|e| e.to_string())?;
        let ctx = context! {
            name => context.get("name").map(|s| s.as_str()).unwrap_or(""),
        };
        tmpl.render(ctx).map_err(|e| e.to_string())
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}
