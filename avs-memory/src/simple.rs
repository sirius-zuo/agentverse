use agentverse::memory::{Memory, MemoryError, Message};
use async_trait::async_trait;
use std::collections::VecDeque;

pub struct SimpleMemory {
    pinned: Vec<Message>,
    window: VecDeque<Message>,
    max_messages: usize,
}

impl SimpleMemory {
    pub fn new(max_messages: usize) -> Self {
        Self {
            pinned: Vec::new(),
            window: VecDeque::new(),
            max_messages,
        }
    }
}

#[async_trait]
impl Memory for SimpleMemory {
    fn append(&mut self, message: Message) {
        self.window.push_back(message);
        if self.window.len() > self.max_messages {
            self.window.pop_front();
        }
    }

    async fn last_n(&mut self, n: usize) -> Result<Vec<Message>, MemoryError> {
        let window_tail: Vec<Message> = self
            .window
            .iter()
            .rev()
            .take(n)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let mut result = self.pinned.clone();
        result.extend(window_tail);
        Ok(result)
    }

    fn pin(&mut self, messages: Vec<Message>) {
        self.pinned = messages;
    }

    async fn flush(&mut self) -> Result<(), MemoryError> {
        Ok(())
    }

    fn clear(&mut self) {
        self.pinned.clear();
        self.window.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::memory::MessageRole;

    #[tokio::test]
    async fn simple_memory_append_and_last_n() {
        let mut m = SimpleMemory::new(5);
        for i in 0..3u32 {
            m.append(Message {
                role: MessageRole::User,
                content: format!("msg {}", i),
            });
        }
        let msgs = m.last_n(2).await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "msg 2");
    }

    #[tokio::test]
    async fn simple_memory_evicts_beyond_max() {
        let mut m = SimpleMemory::new(2);
        for i in 0..4u32 {
            m.append(Message {
                role: MessageRole::User,
                content: format!("msg {}", i),
            });
        }
        let msgs = m.last_n(10).await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "msg 2");
        assert_eq!(msgs[1].content, "msg 3");
    }
}
