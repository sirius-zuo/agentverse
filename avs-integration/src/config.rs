use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct IntegrationConfig {
    pub integration: IntegrationSection,
    #[serde(default)]
    pub connector: ConnectorSection,
}

#[derive(Debug, Deserialize)]
pub struct IntegrationSection {
    pub input: String,
    pub outputs: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ConnectorSection {
    pub slack: Option<SlackConfig>,
    pub github: Option<GithubConfig>,
    pub whatsapp: Option<WhatsappConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SlackConfig {
    pub port: u16,
    pub bot_token_env: String,
    pub signing_secret_env: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GithubConfig {
    pub port: u16,
    pub token_env: String,
    pub webhook_secret_env: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WhatsappConfig {
    pub api_key_env: String,
    pub phone_number_id_env: String,
}
