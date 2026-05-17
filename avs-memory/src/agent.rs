use agentverse::memory::{Memory, MemoryError, Message};
use async_trait::async_trait;
use std::collections::VecDeque;
use super::traits::{LongTermBackend, Summarizer};

pub struct AgentMemory<S: Summarizer, B: LongTermBackend> {
    pinned: Vec<Message>,
    window: VecDeque<Message>,
    max_messages: usize,
    summarization_threshold: usize,
    summarizer: S,
    backend: B,
    needs_summarization: bool,
}

impl<S: Summarizer, B: LongTermBackend> AgentMemory<S, B> {
    pub fn new(
        max_messages: usize,
        summarization_threshold: usize,
        summarizer: S,
        backend: B,
    ) -> Self {
        Self {
            pinned: Vec::new(),
            window: VecDeque::new(),
            max_messages,
            summarization_threshold,
            summarizer,
            backend,
            needs_summarization: false,
        }
    }
}

#[async_trait]
impl<S: Summarizer + Send + Sync, B: LongTermBackend + Send + Sync> Memory
    for AgentMemory<S, B>
{
    fn append(&mut self, message: Message) {
        self.window.push_back(message);
        if self.window.len() > self.max_messages {
            self.window.pop_front();
        }
        if self.window.len() >= self.summarization_threshold {
            self.needs_summarization = true;
        }
    }

    async fn last_n(&mut self, n: usize) -> Result<Vec<Message>, MemoryError> {
        if self.needs_summarization {
            let split = (self.window.len() / 2).max(1);
            let to_summarize: Vec<Message> = self.window.drain(..split).collect();

            match self.summarizer.summarize(&to_summarize).await {
                Ok(summary) => {
                    // Non-fatal: ignore backend write failure
                    let _ = self.backend.store(summary.clone(), vec![]).await;
                    self.window.push_front(summary);
                }
                Err(_) => {
                    // Restore messages so caller always gets something
                    for msg in to_summarize.into_iter().rev() {
                        self.window.push_front(msg);
                    }
                }
            }
            self.needs_summarization = false;
        }

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
        top_k: usize,
    ) -> Result<(), MemoryError> {
        let results = self.backend.search(vec![], top_k).await?;
        for msg in results {
            self.window.push_back(msg);
        }
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), MemoryError> {
        for msg in self.window.iter() {
            let _ = self.backend.store(msg.clone(), vec![]).await;
        }
        Ok(())
    }

    fn clear(&mut self) {
        self.pinned.clear();
        self.window.clear();
        self.needs_summarization = false;
    }
}
