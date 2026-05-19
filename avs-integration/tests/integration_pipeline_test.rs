use agentverse_integration::{
    AgentInvoker, Connector, ConnectorError, Event, InputConnector, Integration,
    IntegrationError, InvokerError, OutputConnector,
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

// Input: returns one event, then errors to stop the loop.
struct OneShotInput {
    event: Event,
    sent: Mutex<bool>,
}

impl Connector for OneShotInput {
    fn name(&self) -> &str { "one-shot" }
}

#[async_trait]
impl InputConnector for OneShotInput {
    async fn receive(&self) -> Result<Event, ConnectorError> {
        let mut sent = self.sent.lock().await;
        if *sent {
            Err(ConnectorError::Connection("done".to_string()))
        } else {
            *sent = true;
            Ok(self.event.clone())
        }
    }
}

// Invoker: uppercases the text.
struct UpperInvoker;

#[async_trait]
impl AgentInvoker for UpperInvoker {
    async fn invoke(&self, event: Event) -> Result<Event, InvokerError> {
        Ok(Event { text: event.text.to_uppercase(), ..event })
    }
}

// Output: records received events.
struct RecordingOutput {
    received: Arc<Mutex<Vec<Event>>>,
}

impl Connector for RecordingOutput {
    fn name(&self) -> &str { "recorder" }
}

#[async_trait]
impl OutputConnector for RecordingOutput {
    async fn send(&self, event: Event) -> Result<(), ConnectorError> {
        self.received.lock().await.push(event);
        Ok(())
    }
}

#[tokio::test]
async fn integration_routes_one_event_to_two_outputs() {
    let received_a = Arc::new(Mutex::new(vec![]));
    let received_b = Arc::new(Mutex::new(vec![]));

    let integration = Integration::new(
        Box::new(OneShotInput {
            event: Event {
                id: uuid::Uuid::new_v4(),
                conversation_id: "ch1".to_string(),
                user_id: "u1".to_string(),
                text: "hello".to_string(),
                metadata: HashMap::new(),
            },
            sent: Mutex::new(false),
        }),
        Box::new(UpperInvoker),
        vec![
            Box::new(RecordingOutput { received: Arc::clone(&received_a) }),
            Box::new(RecordingOutput { received: Arc::clone(&received_b) }),
        ],
    );

    // run() stops when OneShotInput errors on the second receive call.
    let result = integration.run().await;
    assert!(matches!(result, Err(IntegrationError::Input(_))));

    let a = received_a.lock().await;
    let b = received_b.lock().await;
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].text, "HELLO");
    assert_eq!(b[0].text, "HELLO");
}
