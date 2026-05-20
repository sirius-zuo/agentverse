// avs-integration/tests/integration_pipeline_test.rs
// End-to-end: IntegrationRuntime routes one event through a handler to two outputs.
use agentverse_integration::{Connector, ConnectorError, Event, InputConnector, IntegrationRuntime, OutputConnector};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

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
async fn runtime_routes_one_event_to_two_outputs() {
    let received_a = Arc::new(Mutex::new(vec![]));
    let received_b = Arc::new(Mutex::new(vec![]));

    let runtime = IntegrationRuntime::new(
        Box::new(OneShotInput {
            event: Event {
                id: Uuid::new_v4(),
                conversation_id: "ch1".to_string(),
                user_id: "u1".to_string(),
                text: "hello".to_string(),
                metadata: HashMap::new(),
            },
            sent: Mutex::new(false),
        }),
        vec![
            Box::new(RecordingOutput { received: Arc::clone(&received_a) }),
            Box::new(RecordingOutput { received: Arc::clone(&received_b) }),
        ],
    );

    let result = runtime
        .run(|event| async move {
            Ok::<Event, std::convert::Infallible>(Event {
                text: event.text.to_uppercase(),
                ..event
            })
        })
        .await;

    // run() stops when OneShotInput errors on the second receive call.
    assert!(result.is_err());
    let a = received_a.lock().await;
    let b = received_b.lock().await;
    assert_eq!(a.len(), 1);
    assert_eq!(b.len(), 1);
    assert_eq!(a[0].text, "HELLO");
    assert_eq!(b[0].text, "HELLO");
}
