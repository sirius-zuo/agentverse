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

    async fn prime_from_long_term(
        &mut self,
        _query: &str,
        _top_k: usize,
    ) -> Result<(), MemoryError> {
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), MemoryError> {
        Ok(())
    }

    fn clear(&mut self) {
        self.pinned.clear();
        self.window.clear();
    }
}
