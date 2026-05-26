use agentverse::memory::{Memory, Message, MessageRole};
use agentverse_memory::SimpleMemory;

fn user_msg(content: &str) -> Message {
    Message {
        role: MessageRole::User,
        content: content.to_string(),
    }
}

#[tokio::test]
async fn test_append_and_last_n() {
    let mut m = SimpleMemory::new(10);
    for i in 0..5 {
        m.append(user_msg(&format!("msg{}", i)));
    }
    let result = m.last_n(3).await.unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].content, "msg2");
    assert_eq!(result[2].content, "msg4");
}

#[tokio::test]
async fn test_window_eviction() {
    let mut m = SimpleMemory::new(3);
    for i in 0..5 {
        m.append(user_msg(&format!("msg{}", i)));
    }
    let result = m.last_n(10).await.unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(result[0].content, "msg2");
    assert_eq!(result[2].content, "msg4");
}

#[tokio::test]
async fn test_pin_always_prepended() {
    let mut m = SimpleMemory::new(5);
    m.pin(vec![user_msg("preamble")]);
    m.append(user_msg("turn1"));
    m.append(user_msg("turn2"));
    let result = m.last_n(10).await.unwrap();
    assert_eq!(result[0].content, "preamble");
    assert_eq!(result[1].content, "turn1");
    assert_eq!(result[2].content, "turn2");
}

#[tokio::test]
async fn test_pin_survives_eviction() {
    let mut m = SimpleMemory::new(2);
    m.pin(vec![user_msg("preamble")]);
    // Fill window past max_messages
    for i in 0..5 {
        m.append(user_msg(&format!("msg{}", i)));
    }
    let result = m.last_n(10).await.unwrap();
    // Preamble still first, not evicted
    assert_eq!(result[0].content, "preamble");
    // Window has only last 2
    assert_eq!(result.len(), 3); // 1 pinned + 2 window
}

#[tokio::test]
async fn test_pin_not_counted_toward_max() {
    let mut m = SimpleMemory::new(2);
    m.pin(vec![user_msg("pin1"), user_msg("pin2")]);
    m.append(user_msg("a"));
    m.append(user_msg("b"));
    // Window is at max (2), not evicted by pinned
    let result = m.last_n(10).await.unwrap();
    assert_eq!(result.len(), 4); // 2 pinned + 2 window
}

#[tokio::test]
async fn test_clear_resets_both_pinned_and_window() {
    let mut m = SimpleMemory::new(10);
    m.pin(vec![user_msg("preamble")]);
    m.append(user_msg("msg"));
    m.clear();
    let result = m.last_n(10).await.unwrap();
    assert!(result.is_empty());
}

#[tokio::test]
async fn test_flush_is_noop() {
    let mut m = SimpleMemory::new(10);
    m.append(user_msg("msg"));
    m.flush().await.unwrap();
    // Data still present
    let result = m.last_n(10).await.unwrap();
    assert_eq!(result.len(), 1);
}
