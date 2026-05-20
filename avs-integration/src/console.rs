use super::connector::{Connector, InputConnector, OutputConnector};
use super::error::ConnectorError;
use super::event::Event;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use uuid::Uuid;

pub struct ConsoleConnector {
    reader: Mutex<BufReader<tokio::io::Stdin>>,
}

impl ConsoleConnector {
    pub fn new() -> Self {
        Self {
            reader: Mutex::new(BufReader::new(tokio::io::stdin())),
        }
    }
}

impl Default for ConsoleConnector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Connector for ConsoleConnector {
    fn name(&self) -> &str {
        "console"
    }
}

#[async_trait]
impl InputConnector for ConsoleConnector {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        // Flush stdout so any prompt appears before we block on input.
        tokio::io::stdout()
            .flush()
            .await
            .map_err(|e| ConnectorError::Connection(e.to_string()))?;

        let mut line = String::new();
        self.reader
            .lock()
            .await
            .read_line(&mut line)
            .await
            .map_err(|e| ConnectorError::Connection(e.to_string()))?;

        let text = line.trim().to_string();
        if text.is_empty() {
            return Err(ConnectorError::Connection("EOF".to_string()));
        }

        Ok(Event {
            id: Uuid::new_v4(),
            conversation_id: "console".to_string(),
            user_id: "user".to_string(),
            text,
            metadata: HashMap::new(),
        })
    }
}

#[async_trait]
impl OutputConnector for ConsoleConnector {
    async fn send(&self, event: Event) -> Result<(), ConnectorError> {
        let mut stdout = tokio::io::stdout();
        stdout
            .write_all(format!("{}\n", event.text).as_bytes())
            .await
            .map_err(|e| ConnectorError::Connection(e.to_string()))?;
        stdout
            .flush()
            .await
            .map_err(|e| ConnectorError::Connection(e.to_string()))
    }
}
