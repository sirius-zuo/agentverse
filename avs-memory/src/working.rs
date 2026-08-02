use std::collections::HashMap;
use std::time::{Duration, Instant};

use agentverse::memory::Message;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::session::SessionId;

/// L1 working-memory tier: a short-TTL in-process cache of recent turns for a
/// (user, session) pair, sitting in front of the L2 `SessionMemory` store.
#[async_trait]
pub trait WorkingMemory: Send + Sync {
    /// Some(messages) on fresh hit; None on miss or TTL-expired entry.
    async fn load(&self, user_id: &str, session_id: SessionId) -> Option<Vec<Message>>;
    /// Replace the entry (sweeps expired entries as it goes).
    async fn store(&self, user_id: &str, session_id: SessionId, messages: Vec<Message>);
    /// Append one turn; creates a minimal entry if the key was evicted mid-call.
    async fn append_turn(
        &self,
        user_id: &str,
        session_id: SessionId,
        user_msg: Message,
        assistant_msg: Message,
    );
    async fn evict(&self, user_id: &str, session_id: SessionId);
}

struct Entry {
    messages: Vec<Message>,
    last_used: Instant,
}

pub struct CacheMemory {
    entries: Mutex<HashMap<(String, SessionId), Entry>>,
    ttl: Duration,
}

impl CacheMemory {
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }
}

#[async_trait]
impl WorkingMemory for CacheMemory {
    async fn load(&self, user_id: &str, session_id: SessionId) -> Option<Vec<Message>> {
        let key = (user_id.to_string(), session_id);
        let entries = self.entries.lock().await;
        let entry = entries.get(&key)?;
        if entry.last_used.elapsed() <= self.ttl {
            Some(entry.messages.clone())
        } else {
            None
        }
    }

    async fn store(&self, user_id: &str, session_id: SessionId, messages: Vec<Message>) {
        let key = (user_id.to_string(), session_id);
        let mut entries = self.entries.lock().await;
        let ttl = self.ttl;
        entries.retain(|_, entry| entry.last_used.elapsed() <= ttl);
        entries.insert(
            key,
            Entry {
                messages,
                last_used: Instant::now(),
            },
        );
    }

    async fn append_turn(
        &self,
        user_id: &str,
        session_id: SessionId,
        user_msg: Message,
        assistant_msg: Message,
    ) {
        let key = (user_id.to_string(), session_id);
        let mut entries = self.entries.lock().await;
        if let Some(entry) = entries.get_mut(&key) {
            entry.messages.push(user_msg);
            entry.messages.push(assistant_msg);
            entry.last_used = Instant::now();
        } else {
            entries.insert(
                key,
                Entry {
                    messages: vec![user_msg, assistant_msg],
                    last_used: Instant::now(),
                },
            );
        }
    }

    async fn evict(&self, user_id: &str, session_id: SessionId) {
        let key = (user_id.to_string(), session_id);
        self.entries.lock().await.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::memory::MessageRole;
    use uuid::Uuid;

    fn msg(role: MessageRole, content: &str) -> Message {
        Message::text(role, content)
    }

    #[tokio::test]
    async fn load_misses_on_empty_cache() {
        let cache = CacheMemory::new(Duration::from_secs(300));
        let session_id = Uuid::new_v4();
        assert!(cache.load("alice", session_id).await.is_none());
    }

    #[tokio::test]
    async fn store_then_load_hits() {
        let cache = CacheMemory::new(Duration::from_secs(300));
        let session_id = Uuid::new_v4();
        let messages = vec![msg(MessageRole::User, "hi")];
        cache.store("alice", session_id, messages.clone()).await;

        let loaded = cache.load("alice", session_id).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].as_text(), "hi");
    }

    #[tokio::test]
    async fn load_misses_after_ttl_expires() {
        let cache = CacheMemory::new(Duration::from_millis(10));
        let session_id = Uuid::new_v4();
        cache
            .store("alice", session_id, vec![msg(MessageRole::User, "hi")])
            .await;

        tokio::time::sleep(Duration::from_millis(15)).await;

        assert!(cache.load("alice", session_id).await.is_none());
    }

    #[tokio::test]
    async fn append_turn_appends_to_present_entry() {
        let cache = CacheMemory::new(Duration::from_secs(300));
        let session_id = Uuid::new_v4();
        cache
            .store("alice", session_id, vec![msg(MessageRole::User, "first")])
            .await;

        cache
            .append_turn(
                "alice",
                session_id,
                msg(MessageRole::User, "second"),
                msg(MessageRole::Assistant, "reply"),
            )
            .await;

        let loaded = cache.load("alice", session_id).await.unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded[1].as_text(), "second");
        assert_eq!(loaded[2].as_text(), "reply");
    }

    #[tokio::test]
    async fn append_turn_inserts_minimal_entry_when_key_absent() {
        let cache = CacheMemory::new(Duration::from_secs(300));
        let session_id = Uuid::new_v4();

        cache
            .append_turn(
                "alice",
                session_id,
                msg(MessageRole::User, "hello"),
                msg(MessageRole::Assistant, "hi there"),
            )
            .await;

        let loaded = cache.load("alice", session_id).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].as_text(), "hello");
        assert_eq!(loaded[1].as_text(), "hi there");
    }

    #[tokio::test]
    async fn evict_removes_entry() {
        let cache = CacheMemory::new(Duration::from_secs(300));
        let session_id = Uuid::new_v4();
        cache
            .store("alice", session_id, vec![msg(MessageRole::User, "hi")])
            .await;

        cache.evict("alice", session_id).await;

        assert!(cache.load("alice", session_id).await.is_none());
    }
}
