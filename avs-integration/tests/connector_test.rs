use agentverse_integration::{ConnectorError, Event, InputConnector, OutputConnector};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

struct MockConnector {
    events: Mutex<Vec<Event>>,
}

impl agentverse_integration::Connector for MockConnector {
    fn name(&self) -> &str {
        "mock"
    }
}

#[async_trait]
impl InputConnector for MockConnector {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        Ok(Event {
            id: uuid::Uuid::new_v4(),
            conversation_id: "ch1".to_string(),
            user_id: "u1".to_string(),
            text: "ping".to_string(),
            metadata: HashMap::new(),
        })
    }
}

#[async_trait]
impl OutputConnector for MockConnector {
    async fn send(&self, event: Event) -> Result<(), ConnectorError> {
        self.events.lock().await.push(event);
        Ok(())
    }
}

#[tokio::test]
async fn mock_connector_receive_and_send() {
    let connector = MockConnector {
        events: Mutex::new(vec![]),
    };
    let event = connector.receive().await.unwrap();
    assert_eq!(event.text, "ping");
    connector.send(event.clone()).await.unwrap();
    assert_eq!(connector.events.lock().await.len(), 1);
}

#[tokio::test]
async fn arc_connector_implements_input() {
    let connector = Arc::new(MockConnector {
        events: Mutex::new(vec![]),
    });
    let event = connector.receive().await.unwrap();
    assert_eq!(event.text, "ping");
}
