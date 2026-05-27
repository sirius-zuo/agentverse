use serde::Deserialize;
use std::collections::HashMap;

use crate::error::McpError;
use crate::transport::McpTransport;

#[derive(Debug, Deserialize, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: TransportKind,
    // stdio fields
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    // streamable_http fields
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    Stdio,
    StreamableHttp,
}

impl McpServerConfig {
    /// Expand `${VAR}` placeholders and convert to McpTransport.
    pub fn into_transport(&self) -> Result<McpTransport, McpError> {
        match &self.transport {
            TransportKind::Stdio => {
                let command = self.command.as_deref().ok_or_else(|| {
                    McpError::Config(format!("{}: stdio requires 'command'", self.name))
                })?;
                let args = self
                    .args
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| expand_env(&a))
                    .collect::<Result<Vec<_>, _>>()?;
                let env = self
                    .env
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(k, v)| expand_env(&v).map(|v| (k, v)))
                    .collect::<Result<HashMap<_, _>, _>>()?;
                Ok(McpTransport::Stdio {
                    command: std::path::PathBuf::from(expand_env(command)?),
                    args,
                    env,
                })
            }
            TransportKind::StreamableHttp => {
                let url = self.url.as_deref().ok_or_else(|| {
                    McpError::Config(format!("{}: streamable_http requires 'url'", self.name))
                })?;
                let url = expand_env(url)?;
                let endpoint = url
                    .parse::<reqwest::Url>()
                    .map_err(|e| McpError::Config(format!("Invalid URL: {e}")))?;
                let mut header_map = reqwest::header::HeaderMap::new();
                if let Some(headers) = &self.headers {
                    for (k, v) in headers {
                        let name = reqwest::header::HeaderName::from_bytes(k.as_bytes())
                            .map_err(|e| McpError::Config(e.to_string()))?;
                        let value = reqwest::header::HeaderValue::from_str(&expand_env(v)?)
                            .map_err(|e| McpError::Config(e.to_string()))?;
                        header_map.insert(name, value);
                    }
                }
                Ok(McpTransport::StreamableHttp {
                    endpoint,
                    headers: header_map,
                })
            }
        }
    }
}

/// Replace `${VAR}` with the value of env var VAR. Errors on undefined vars.
fn expand_env(s: &str) -> Result<String, McpError> {
    let mut result = s.to_string();
    let mut i = 0;
    while let Some(start) = result[i..].find("${") {
        let abs_start = i + start;
        let rest = &result[abs_start + 2..];
        let end = rest
            .find('}')
            .ok_or_else(|| McpError::Config(format!("Unclosed '${{' in: {s}")))?;
        let var_name = &rest[..end];
        let value = std::env::var(var_name)
            .map_err(|_| McpError::Config(format!("Undefined env var: ${{{var_name}}}")))?;
        result.replace_range(abs_start..abs_start + 2 + end + 1, &value);
        i = abs_start + value.len();
    }
    Ok(result)
}
