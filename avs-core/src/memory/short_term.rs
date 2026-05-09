use super::{Memory, Message};

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

impl Memory for ShortTermMemory {
    fn append(&mut self, message: Message) {
        self.messages.push(message);
        if self.messages.len() > self.max_messages {
            self.messages.drain(0..self.messages.len() - self.max_messages);
        }
    }

    fn last_n(&self, n: usize) -> Vec<Message> {
        let start = self.messages.len().saturating_sub(n);
        self.messages[start..].to_vec()
    }

    fn clear(&mut self) {
        self.messages.clear();
    }
}
