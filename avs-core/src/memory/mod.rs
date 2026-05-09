use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: MessageRole,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

pub trait Memory {
    fn append(&mut self, message: Message);
    fn last_n(&self, n: usize) -> Vec<Message>;
    fn clear(&mut self);
}

mod short_term;
pub use short_term::ShortTermMemory;
