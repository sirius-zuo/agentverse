use super::config::IntegrationConfig;
use super::connector::{InputConnector, OutputConnector};
use super::console::ConsoleConnector;
use super::error::IntegrationError;
use super::event::Event;
use super::github::GithubConnector;
use super::slack::SlackConnector;
use super::whatsapp::WhatsAppConnector;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;

/// Bridges all configured connector types to the typed trait objects needed
/// by IntegrationRuntime without exposing the concrete types outside this module.
enum BuiltConnector {
    Slack(Arc<SlackConnector>),
    Github(Arc<GithubConnector>),
    Whatsapp(Arc<WhatsAppConnector>),
}

impl BuiltConnector {
    fn input(&self) -> Box<dyn InputConnector> {
        match self {
            Self::Slack(c) => Box::new(Arc::clone(c)),
            Self::Github(c) => Box::new(Arc::clone(c)),
            Self::Whatsapp(c) => Box::new(Arc::clone(c)),
        }
    }

    fn output(&self) -> Box<dyn OutputConnector> {
        match self {
            Self::Slack(c) => Box::new(Arc::clone(c)),
            Self::Github(c) => Box::new(Arc::clone(c)),
            Self::Whatsapp(c) => Box::new(Arc::clone(c)),
        }
    }
}

/// Owned by the agent. Reads integration config, starts connectors, runs the
/// event loop. Output errors are logged and skipped; input errors stop the loop.
pub struct IntegrationRuntime {
    input: Box<dyn InputConnector>,
    outputs: Vec<Box<dyn OutputConnector>>,
}

impl IntegrationRuntime {
    /// Create with explicit connectors — useful for tests and programmatic wiring.
    pub fn new(
        input: Box<dyn InputConnector>,
        outputs: Vec<Box<dyn OutputConnector>>,
    ) -> Self {
        Self { input, outputs }
    }

    /// Create using stdin/stdout as the single bidirectional connector.
    pub fn console() -> Self {
        let c = Arc::new(ConsoleConnector::new());
        Self {
            input: Box::new(Arc::clone(&c)),
            outputs: vec![Box::new(c)],
        }
    }

    /// Read `path` as a TOML config file and instantiate connectors.
    /// Falls back to `console()` if the file does not exist.
    /// Fails fast if the file exists but is malformed, or if a referenced
    /// env var is not set.
    pub async fn from_config(path: &str) -> Result<Self, IntegrationError> {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let config: IntegrationConfig = toml::from_str(&content)
                    .map_err(|e| IntegrationError::Config(e.to_string()))?;
                Self::from_parsed_config(config)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::console()),
            Err(e) => Err(IntegrationError::Config(e.to_string())),
        }
    }

    fn from_parsed_config(config: IntegrationConfig) -> Result<Self, IntegrationError> {
        let mut built: HashMap<String, BuiltConnector> = HashMap::new();

        if let Some(cfg) = config.connector.slack {
            let token = std::env::var(&cfg.bot_token_env).map_err(|_| {
                IntegrationError::Config(format!("env var {} not set", cfg.bot_token_env))
            })?;
            let secret = std::env::var(&cfg.signing_secret_env).map_err(|_| {
                IntegrationError::Config(format!("env var {} not set", cfg.signing_secret_env))
            })?;
            built.insert(
                "slack".to_string(),
                BuiltConnector::Slack(Arc::new(SlackConnector::new(&token, &secret, cfg.port))),
            );
        }

        if let Some(cfg) = config.connector.github {
            let token = std::env::var(&cfg.token_env).map_err(|_| {
                IntegrationError::Config(format!("env var {} not set", cfg.token_env))
            })?;
            let secret = std::env::var(&cfg.webhook_secret_env).map_err(|_| {
                IntegrationError::Config(format!("env var {} not set", cfg.webhook_secret_env))
            })?;
            built.insert(
                "github".to_string(),
                BuiltConnector::Github(Arc::new(GithubConnector::new(
                    &token, &secret, cfg.port,
                ))),
            );
        }

        if let Some(cfg) = config.connector.whatsapp {
            let api_key = std::env::var(&cfg.api_key_env).map_err(|_| {
                IntegrationError::Config(format!("env var {} not set", cfg.api_key_env))
            })?;
            let phone_id = std::env::var(&cfg.phone_number_id_env).map_err(|_| {
                IntegrationError::Config(format!("env var {} not set", cfg.phone_number_id_env))
            })?;
            built.insert(
                "whatsapp".to_string(),
                BuiltConnector::Whatsapp(Arc::new(WhatsAppConnector::new(&api_key, &phone_id))),
            );
        }

        let input_name = &config.integration.input;
        let input_connector = built.get(input_name).ok_or_else(|| {
            IntegrationError::Config(format!(
                "input '{}' not found — add a [connector.{}] section",
                input_name, input_name
            ))
        })?;
        let input = input_connector.input();

        let mut outputs = Vec::new();
        for name in &config.integration.outputs {
            let c = built.get(name).ok_or_else(|| {
                IntegrationError::Config(format!(
                    "output '{}' not found — add a [connector.{}] section",
                    name, name
                ))
            })?;
            outputs.push(c.output());
        }

        Ok(Self { input, outputs })
    }

    /// Start connectors and run the receive → handle → send loop.
    ///
    /// Returns when the input connector returns an error (e.g. EOF or shutdown).
    /// Output errors are logged and skipped — they do not stop the loop.
    /// Handler errors are logged and skipped — the next event is processed normally.
    pub async fn run<F, Fut, E>(&self, handler: F) -> Result<(), IntegrationError>
    where
        F: Fn(Event) -> Fut + Send + Sync,
        Fut: Future<Output = Result<Event, E>> + Send,
        E: std::error::Error + Send + 'static,
    {
        self.input
            .start()
            .await
            .map_err(IntegrationError::Input)?;

        loop {
            let event = match self.input.receive().await {
                Ok(event) => event,
                Err(crate::error::ConnectorError::Eof) => return Ok(()),
                Err(e) => return Err(IntegrationError::Input(e)),
            };

            match handler(event).await {
                Ok(response) => {
                    for output in &self.outputs {
                        if let Err(e) = output.send(response.clone()).await {
                            tracing::warn!(
                                connector = output.name(),
                                error = %e,
                                "output send failed, skipping"
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "handler error, skipping event");
                }
            }
        }
    }
}
