use agentverse::memory::MessageRole;
use agentverse::{Memory, Message, ShortTermMemory};

#[test]
fn test_memory_append_and_last_n() {
    let mut memory = ShortTermMemory::new(10);

    for i in 0..5 {
        memory.append(Message {
            role: MessageRole::User,
            content: format!("msg{}", i),
        });
    }

    let last = memory.last_n(3);
    assert_eq!(last.len(), 3);
    assert_eq!(last[0].content, "msg2");
    assert_eq!(last[2].content, "msg4");
}

#[test]
fn test_memory_eviction() {
    let mut memory = ShortTermMemory::new(3);

    for i in 0..5 {
        memory.append(Message {
            role: MessageRole::User,
            content: format!("msg{}", i),
        });
    }

    // Should only keep last 3
    let last = memory.last_n(10);
    assert_eq!(last.len(), 3);
    assert_eq!(last[0].content, "msg2");
    assert_eq!(last[2].content, "msg4");
}

#[test]
fn test_memory_clear() {
    let mut memory = ShortTermMemory::new(10);
    memory.append(Message {
        role: MessageRole::User,
        content: "hello".to_string(),
    });
    memory.clear();
    assert!(memory.last_n(10).is_empty());
}
