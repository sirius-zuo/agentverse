use std::sync::Arc;
use tracing::info;

use crate::config::Config;
use crate::error::AgentError;
use crate::memory::{Message, MessageRole};
use crate::model::{ConnectionManager, GenerateRequest, GenerateResponse, ToolDefinition};

pub struct LlmRunner {
    connection: Arc<ConnectionManager>,
}

impl LlmRunner {
    pub fn new(connection: Arc<ConnectionManager>) -> Self {
        Self { connection }
    }

    pub fn from_config(config: Config) -> Result<Self, AgentError> {
        config.validate()?;

        let model_name = config
            .provider
            .settings
            .get("model_name")
            .cloned()
            .unwrap_or_default();
        info!(model = %model_name, provider = %config.provider.name, "LlmRunner initialized");

        let registry = crate::model::ProviderRegistry::with_builtins();
        let cm = ConnectionManager::from_config(config.provider.clone(), &registry)?;
        Ok(Self {
            connection: Arc::new(cm),
        })
    }

    pub async fn invoke(&self, messages: Vec<Message>) -> Result<GenerateResponse, AgentError> {
        self.invoke_inner(messages, None, None).await
    }

    pub async fn invoke_with_tools(
        &self,
        messages: Vec<Message>,
        tools: Vec<ToolDefinition>,
    ) -> Result<GenerateResponse, AgentError> {
        self.invoke_inner(messages, Some(tools), None).await
    }

    pub async fn invoke_structured(
        &self,
        messages: Vec<Message>,
        schema: serde_json::Value,
    ) -> Result<GenerateResponse, AgentError> {
        self.invoke_inner(messages, None, Some(schema)).await
    }

    async fn invoke_inner(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<ToolDefinition>>,
        response_format: Option<serde_json::Value>,
    ) -> Result<GenerateResponse, AgentError> {
        let mut system_parts: Vec<String> = Vec::new();
        let mut conv: Vec<Message> = Vec::new();

        for msg in messages {
            if matches!(msg.role, MessageRole::System) {
                system_parts.push(msg.content);
            } else {
                conv.push(msg);
            }
        }

        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n\n"))
        };

        self.connection
            .generate(GenerateRequest {
                system,
                messages: conv,
                tools,
                response_format,
            })
            .await
            .map_err(AgentError::Model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ModelError;
    use crate::model::{ModelProvider, ToolDefinition};
    use std::sync::Mutex;

    struct RecordingProvider {
        request: Arc<Mutex<Option<GenerateRequest>>>,
    }

    impl ModelProvider for RecordingProvider {
        fn name(&self) -> &str {
            "recording"
        }

        fn build_request(
            &self,
            _model: &str,
            request: GenerateRequest,
        ) -> Result<serde_json::Value, ModelError> {
            *self.request.lock().unwrap() = Some(request);
            Err(ModelError::InvalidResponse(
                "stop after recording".to_string(),
            ))
        }

        fn parse_response(&self, _body: &str) -> Result<GenerateResponse, ModelError> {
            Err(ModelError::InvalidResponse("not used".to_string()))
        }

        fn request_headers(&self, _api_key: &str) -> reqwest::header::HeaderMap {
            reqwest::header::HeaderMap::new()
        }

        fn endpoint_path(&self, _model: &str) -> String {
            "/recording".to_string()
        }
    }

    #[tokio::test]
    async fn invoke_with_tools_forwards_definitions_to_the_provider() {
        let recorded_request = Arc::new(Mutex::new(None));
        let runner = LlmRunner::new(Arc::new(ConnectionManager::new(
            RecordingProvider {
                request: Arc::clone(&recorded_request),
            },
            "http://unused",
            "key",
            "model",
        )));
        let tools = vec![ToolDefinition {
            name: "calculate".to_string(),
            description: "Perform arithmetic".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {"expression": {"type": "string"}}
            }),
        }];

        let result = runner
            .invoke_with_tools(
                vec![Message {
                    role: MessageRole::User,
                    content: "What is 2 + 2?".to_string(),
                }],
                tools,
            )
            .await;

        assert!(
            result.is_err(),
            "recording provider deliberately stops the call"
        );
        let request = recorded_request
            .lock()
            .unwrap()
            .take()
            .expect("runner must call the provider");
        let tools = request.tools.expect("native tools must be forwarded");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "calculate");
        assert_eq!(tools[0].description, "Perform arithmetic");
        assert_eq!(
            tools[0].parameters,
            serde_json::json!({
                "type": "object",
                "properties": {"expression": {"type": "string"}}
            })
        );
        assert!(request.response_format.is_none());
    }
}
