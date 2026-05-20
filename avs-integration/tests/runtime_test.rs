use agentverse_integration::{Connector, ConnectorError, Event, InputConnector, IntegrationRuntime, OutputConnector};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

fn make_event(text: &str) -> Event {
    Event {
        id: Uuid::new_v4(),
        conversation_id: "ch1".to_string(),
        user_id: "u1".to_string(),
        text: text.to_string(),
        metadata: HashMap::new(),
    }
}

struct OneShotInput {
    event: Event,
    sent: Mutex<bool>,
}

impl Connector for OneShotInput {
    fn name(&self) -> &str { "oneshot" }
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

struct FailOutput;
impl Connector for FailOutput { fn name(&self) -> &str { "fail" } }
#[async_trait]
impl OutputConnector for FailOutput {
    async fn send(&self, _: Event) -> Result<(), ConnectorError> {
        Err(ConnectorError::Platform("always fails".to_string()))
    }
}

#[tokio::test]
async fn runtime_routes_event_through_handler_to_output() {
    let received = Arc::new(Mutex::new(vec![]));
    let recorded = Arc::clone(&received);

    let runtime = IntegrationRuntime::new(
        Box::new(OneShotInput { event: make_event("hello"), sent: Mutex::new(false) }),
        vec![Box::new(RecordingOutput { received })],
    );

    let result = runtime
        .run(|event| async move {
            Ok::<Event, std::convert::Infallible>(Event {
                text: event.text.to_uppercase(),
                ..event
            })
        })
        .await;

    // Loop stops when OneShotInput returns error on second receive.
    assert!(result.is_err());
    let events = recorded.lock().await;
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].text, "HELLO");
}

#[tokio::test]
async fn runtime_skips_failed_outputs_and_continues_to_next() {
    let received = Arc::new(Mutex::new(vec![]));
    let recorded = Arc::clone(&received);

    let runtime = IntegrationRuntime::new(
        Box::new(OneShotInput { event: make_event("hi"), sent: Mutex::new(false) }),
        vec![
            Box::new(FailOutput),
            Box::new(RecordingOutput { received }),
        ],
    );

    let _ = runtime
        .run(|event| async move { Ok::<Event, std::convert::Infallible>(event) })
        .await;

    // FailOutput error is skipped; RecordingOutput still receives the event.
    let events = recorded.lock().await;
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn runtime_skips_handler_errors_and_reads_next_event() {
    // Two events; first causes handler error, second succeeds.
    struct TwoShotInput {
        count: Mutex<usize>,
    }
    impl Connector for TwoShotInput { fn name(&self) -> &str { "twoshot" } }
    #[async_trait]
    impl InputConnector for TwoShotInput {
        async fn receive(&self) -> Result<Event, ConnectorError> {
            let mut c = self.count.lock().await;
            *c += 1;
            match *c {
                1 | 2 => Ok(make_event(&format!("msg{}", *c))),
                _ => Err(ConnectorError::Connection("done".to_string())),
            }
        }
    }

    let received = Arc::new(Mutex::new(vec![]));
    let recorded = Arc::clone(&received);

    let runtime = IntegrationRuntime::new(
        Box::new(TwoShotInput { count: Mutex::new(0) }),
        vec![Box::new(RecordingOutput { received })],
    );

    let _ = runtime
        .run(|event| async move {
            if event.text == "msg1" {
                Err(std::io::Error::other("handler error"))
            } else {
                Ok(event)
            }
        })
        .await;

    let events = recorded.lock().await;
    // msg1 handler errored (skipped), msg2 succeeded
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].text, "msg2");
}

#[tokio::test]
async fn from_config_falls_back_to_console_when_file_missing() {
    // A nonexistent path should return Ok (console fallback), not Err.
    let result = IntegrationRuntime::from_config("/tmp/does_not_exist_xyz.toml").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn from_config_errors_on_malformed_toml() {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(f, "not valid toml ][[[").unwrap();
    let result = IntegrationRuntime::from_config(f.path().to_str().unwrap()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn from_config_errors_when_input_connector_not_defined() {
    use std::io::Write;
    let mut f = tempfile::NamedTempFile::new().unwrap();
    writeln!(f, r#"
[integration]
input = "slack"
outputs = ["slack"]
"#).unwrap();
    // No [connector.slack] section — env var lookup never happens.
    // from_config reads the file, parses it, then from_parsed_config
    // fails because "slack" isn't in the built connector map.
    let result = IntegrationRuntime::from_config(f.path().to_str().unwrap()).await;
    let err = result.err().expect("expected an error");
    let msg = err.to_string();
    assert!(msg.contains("slack"), "error should mention missing connector name");
}

#[tokio::test]
async fn runtime_stops_cleanly_on_eof() {
    struct EofInput;
    impl Connector for EofInput { fn name(&self) -> &str { "eof" } }
    #[async_trait]
    impl InputConnector for EofInput {
        async fn receive(&self) -> Result<Event, ConnectorError> {
            Err(ConnectorError::Eof)
        }
    }

    let runtime = IntegrationRuntime::new(
        Box::new(EofInput),
        vec![],
    );

    let result = runtime
        .run(|event| async move { Ok::<Event, std::convert::Infallible>(event) })
        .await;

    assert!(result.is_ok(), "EOF should be a clean shutdown, not an error");
}
