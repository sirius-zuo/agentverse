use async_trait::async_trait;
use super::{Memory, MemoryError, Message};

// Internal default used by Agent. Not re-exported from avs-core.
// External code should use agentverse_memory::SimpleMemory instead.
pub struct ShortTermMemory {
    messages: Vec<Message>,
    max_messages: usize,
}

impl ShortTermMemory {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: Vec::with_capacity(max_messages),
            max_messages,
        }
    }
}

#[async_trait]
impl Memory for ShortTermMemory {
    fn append(&mut self, message: Message) {
        self.messages.push(message);
        if self.messages.len() > self.max_messages {
            self.messages.drain(0..self.messages.len() - self.max_messages);
        }
    }

    // may mutate internal cache on first access (async to allow lazy summarization in other impls)
    async fn last_n(&mut self, n: usize) -> Result<Vec<Message>, MemoryError> {
        let start = self.messages.len().saturating_sub(n);
        Ok(self.messages[start..].to_vec())
    }

    fn pin(&mut self, _messages: Vec<Message>) {
        // ShortTermMemory has no pinned concept; used only by Agent placeholder.
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
        self.messages.clear();
    }
}
