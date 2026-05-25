use super::traits::{LongTermBackend, Summarizer};
use agentverse::memory::{Memory, MemoryError, Message};
use async_trait::async_trait;
use std::collections::VecDeque;

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
impl<S: Summarizer + Send + Sync, B: LongTermBackend + Send + Sync> Memory for AgentMemory<S, B> {
    fn append(&mut self, message: Message) {
        self.window.push_back(message);
        if self.window.len() > self.max_messages {
            self.window.pop_front();
        }
        if self.window.len() >= self.summarization_threshold {
            self.needs_summarization = true;
        }
        tracing::debug!(
            operation = "store",
            window_size = self.window.len(),
            "Memory append"
        );
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
        tracing::debug!(
            operation = "retrieve",
            count = result.len(),
            "Memory retrieve"
        );
        Ok(result)
    }

    fn pin(&mut self, messages: Vec<Message>) {
        self.pinned = messages;
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

#[cfg(test)]
mod logging_tests {
    use super::*;
    use agentverse::memory::{Memory, MemoryError, Message, MessageRole};
    use async_trait::async_trait;

    struct NoopBackend;
    struct NoopSummarizer;

    #[async_trait]
    impl crate::traits::LongTermBackend for NoopBackend {
        async fn store(&self, _: Message, _: Vec<f32>) -> Result<(), MemoryError> {
            Ok(())
        }
        async fn search(&self, _: Vec<f32>, _: usize) -> Result<Vec<Message>, MemoryError> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl crate::traits::Summarizer for NoopSummarizer {
        async fn summarize(&self, _msgs: &[Message]) -> Result<Message, MemoryError> {
            Ok(Message {
                role: MessageRole::Assistant,
                content: "summary".to_string(),
            })
        }
    }

    #[tokio::test]
    async fn memory_ops_log_without_panic() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let mut mem = AgentMemory::new(10, 8, NoopSummarizer, NoopBackend);
        mem.append(Message {
            role: MessageRole::User,
            content: "hello".to_string(),
        });
        let msgs = mem.last_n(5).await.unwrap();
        assert_eq!(msgs.len(), 1);
    }
}
