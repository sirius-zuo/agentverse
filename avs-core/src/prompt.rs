use minijinja::Environment;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::error::{AgentError, ConfigError};
use crate::Example;

/// TOML wrapper: `[[example]]` sections deserialize into `example: Vec<Example>`.
#[derive(Deserialize, Default)]
struct ExampleFile {
    #[serde(default)]
    example: Vec<Example>,
}

/// Default embedded templates shipped with the library.
const DEFAULT_SYSTEM_TEMPLATE: &str = "You are a helpful AI assistant.\n\
     You are concise and accurate. Never claim to have done something you haven't.\n\
     If you don't know something, say so.\
     {% if tools %}\n\n\
     Available tools:\n\
     {{ tools }}\n\n\
     Always respond in this exact format:\n\
     Thought: <your reasoning>\n\
     Action: <tool_name>\n\
     Action Input: <json args>\n\n\
     When you have the final answer:\n\
     Thought: <your reasoning>\n\
     Answer: <final answer>\
     {% else %}\n\n\
     Always end your response with:\n\
     Answer: <your answer>\
     {% endif %}";

const DEFAULT_REACT_TEMPLATE: &str = "You solve tasks by reasoning step by step. When you need \
     information or need to take an action, call the appropriate tool directly — tool calls are \
     handled natively, you never need to describe them as text.\n\n\
     {% if examples %}\n\
     Examples:\n\
     {% for example in examples %}\n\
     User: {{ example.input }}\n\
     Assistant: {{ example.output }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     When you have a complete answer, respond with it directly.";

const DEFAULT_PLAN_AND_EXECUTE_TEMPLATE: &str =
    "You are a planning assistant. Generate a step-by-step plan.\n\n\
     Available tools:\n\
     {{ tools }}\n\n\
     {% if examples %}\n\
     Examples:\n\
     {% for example in examples %}\n\
     User: {{ example.input }}\n\
     Assistant: {{ example.output }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     Respond with ONLY a JSON object. Rules:\n\
     - Each step has exactly one \"args\" object — do not repeat the key\n\
     - Shell commands must use single quotes for quoting (e.g. grep '^pattern' not grep \"^pattern\")\n\
     - \"description\" values must not contain double-quote characters\n\
     {\"description\": \"...\", \"steps\": [{\"id\": 1, \"description\": \"...\", \"tool\": \"...\", \"args\": {}, \"depends_on\": []}]}\n\
     {% if conversation %}\n\
     Previous results:\n\
     {{ conversation }}\n\
     {% endif %}";

const DEFAULT_HIERARCHICAL_DECOMPOSE_TEMPLATE: &str =
    "Break this request into sub-goals. Each sub-goal should be independently executable.\n\n\
     {% if examples %}\n\
     Examples:\n\
     {% for example in examples %}\n\
     Input: {{ example.input }}\n\
     Strategy: {{ example.strategy }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     Respond with ONLY a JSON array of strings.";

const DEFAULT_ROUTER_TEMPLATE: &str =
    "Choose the best orchestration strategy for this request.\n\n\
     Available strategies:\n\
     - react: simple Q&A, tool use, step-by-step reasoning\n\
     - plan_and_execute: tasks with clear upfront steps\n\
     - hierarchical: complex tasks needing decomposition\n\n\
     {% if examples %}\n\
     Examples:\n\
     {% for example in examples %}\n\
     Input: {{ example.input }}\n\
     Strategy: {{ example.strategy }}\n\
     {% endfor %}\n\
     {% endif %}\n\n\
     Respond with ONLY the strategy name.";

/// Configuration for prompt registry loading.
#[derive(Debug, Default)]
pub struct PromptConfig {
    /// Optional system prompt override.
    pub system_prompt: Option<String>,
    /// Optional prompts directory for .j2/.toml file loading.
    pub prompts_dir: Option<String>,
    /// Additional templates to register (name → template string).
    pub templates: HashMap<String, String>,
    /// Additional example sets to register (name → examples).
    pub examples: HashMap<String, Vec<Example>>,
}

pub struct PromptRegistry {
    env: Environment<'static>,
    examples: HashMap<String, Vec<Example>>,
    /// True when a `react.j2` file was loaded from a prompts directory,
    /// meaning the cycle should use it as a one-time preamble message.
    react_template_loaded: bool,
}

impl PromptRegistry {
    /// Create from configuration — loads defaults, optional files, and overrides.
    pub fn from_config(config: &PromptConfig) -> Result<Self, AgentError> {
        let mut registry = Self::default();

        if let Some(ref dir) = config.prompts_dir {
            registry.load_from_directory(dir)?;
        }

        for (name, template) in &config.templates {
            registry.add_template(name, template)?;
        }
        for (name, examples) in &config.examples {
            registry.add_examples(name.clone(), examples.clone());
        }

        if let Some(ref system_prompt) = config.system_prompt {
            registry.add_template("system", system_prompt)?;
        }

        Ok(registry)
    }

    /// Create default registry with embedded templates only.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a template by name. Replaces any existing template with the
    /// same name. Fails on invalid template syntax.
    pub fn add_template(&mut self, name: &str, template: &str) -> Result<(), AgentError> {
        self.env
            .add_template_owned(name.to_string(), template.to_string())
            .map_err(|e| {
                AgentError::Config(ConfigError::Invalid(format!(
                    "Invalid template '{}': {}",
                    name, e
                )))
            })
    }

    /// Register an example set by name.
    pub fn add_examples(&mut self, name: String, examples: Vec<Example>) {
        self.examples.insert(name, examples);
    }

    /// Render a template by name with context.
    pub fn render(
        &self,
        name: &str,
        context: HashMap<String, Value>,
    ) -> Result<String, AgentError> {
        let tmpl = self.env.get_template(name).map_err(|e| {
            AgentError::Config(ConfigError::Invalid(format!(
                "Template '{}' not found: {}",
                name, e
            )))
        })?;
        let entries: Vec<(String, minijinja::value::Value)> = context
            .into_iter()
            .map(|(k, v)| (k, minijinja::value::Value::from_serialize(&v)))
            .collect();
        let ctx = minijinja::value::Value::from_iter(entries);
        let result = tmpl.render(ctx).map_err(|e| {
            AgentError::Config(ConfigError::Invalid(format!(
                "Template render error: {}",
                e
            )))
        })?;
        Ok(result)
    }

    /// Get examples for a named example set.
    pub fn get_examples(&self, name: &str) -> Option<&[Example]> {
        self.examples.get(name).map(|v| v.as_slice())
    }

    /// Returns true if a `react.j2` file was loaded from a prompts directory.
    /// Used by the cycle to decide whether to prime a react preamble message.
    pub fn has_react_template(&self) -> bool {
        self.react_template_loaded
    }

    /// Load templates and examples from a directory.
    fn load_from_directory(&mut self, dir: &str) -> Result<(), AgentError> {
        let path = Path::new(dir);
        if !path.is_dir() {
            return Err(AgentError::Config(ConfigError::Invalid(format!(
                "Prompts directory not found: {}",
                dir
            ))));
        }

        let entries = fs::read_dir(path).map_err(|e| {
            AgentError::Config(ConfigError::Invalid(format!(
                "Cannot read prompts directory: {}",
                e
            )))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                AgentError::Config(ConfigError::Invalid(format!(
                    "Error reading directory entry: {}",
                    e
                )))
            })?;
            let path = entry.path();

            match path.extension().and_then(|e| e.to_str()) {
                Some("j2") => {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    let template = fs::read_to_string(&path).map_err(|e| {
                        AgentError::Config(ConfigError::Invalid(format!(
                            "Cannot read template {}: {}",
                            path.display(),
                            e
                        )))
                    })?;
                    if name == "react" {
                        self.react_template_loaded = true;
                    }
                    // Map short file names to the canonical registry keys used by
                    // the strategy crates, so file-based overrides actually land.
                    let registry_name = match name.as_str() {
                        "plan_and_execute" => "strategies.plan_and_execute",
                        "hierarchical" => "strategies.hierarchical.decompose",
                        other => other,
                    };
                    self.add_template(registry_name, &template)?;
                }
                Some("toml") => {
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    let content = fs::read_to_string(&path).map_err(|e| {
                        AgentError::Config(ConfigError::Invalid(format!(
                            "Cannot read examples file {}: {}",
                            path.display(),
                            e
                        )))
                    })?;
                    // TOML can't express a root-level array; files use [[example]]
                    // which produces {"example": [...]}, so we unwrap via ExampleFile.
                    let file: ExampleFile = toml::from_str(&content).map_err(|e| {
                        AgentError::Config(ConfigError::Invalid(format!(
                            "Cannot parse examples file {}: {}",
                            path.display(),
                            e
                        )))
                    })?;
                    self.add_examples(name, file.example);
                }
                _ => {}
            }
        }

        Ok(())
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        let mut env = Environment::new();
        env.add_template("react", DEFAULT_REACT_TEMPLATE).unwrap();
        env.add_template("system", DEFAULT_SYSTEM_TEMPLATE).unwrap();
        env.add_template("strategies.react", DEFAULT_REACT_TEMPLATE)
            .unwrap();
        env.add_template(
            "strategies.plan_and_execute",
            DEFAULT_PLAN_AND_EXECUTE_TEMPLATE,
        )
        .unwrap();
        env.add_template(
            "strategies.hierarchical.decompose",
            DEFAULT_HIERARCHICAL_DECOMPOSE_TEMPLATE,
        )
        .unwrap();
        env.add_template("router", DEFAULT_ROUTER_TEMPLATE).unwrap();
        Self {
            env,
            examples: HashMap::new(),
            react_template_loaded: false,
        }
    }
}
