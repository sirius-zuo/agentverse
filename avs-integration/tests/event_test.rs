use agentverse_integration::Event;
use std::collections::HashMap;

#[test]
fn event_round_trips_json() {
    let event = Event {
        id: uuid::Uuid::new_v4(),
        conversation_id: "C123".to_string(),
        user_id: "U456".to_string(),
        text: "hello".to_string(),
        metadata: HashMap::from([("platform".to_string(), "slack".to_string())]),
    };
    let json = serde_json::to_string(&event).unwrap();
    let restored: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.conversation_id, event.conversation_id);
    assert_eq!(restored.user_id, event.user_id);
    assert_eq!(restored.text, event.text);
    assert_eq!(restored.metadata, event.metadata);
}
