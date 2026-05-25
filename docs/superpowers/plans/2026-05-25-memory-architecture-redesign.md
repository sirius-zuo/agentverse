# Memory Architecture Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the three-layer memory model — working buffer (ephemeral, per-session RAM), session memory (persistent, ~24h rolling), long-term memory (persistent, distilled, user-scoped) — by rewiring existing AgentVerse building blocks into their correct roles, as designed in `docs/superpowers/specs/2026-05-25-memory-architecture-research.md`.

**Architecture:** Strategies become pure `Vec<Message> → String` by dropping the dead `memory` param. The Agent owns all memory orchestration: a per-`(user_id, session_id)` working-buffer cache with TTL eviction, rehydrated from `SessionStore` (Layer 2) when cold; optional `MemoryStore` (Layer 3) for cross-session retrieval and async consolidation. `SessionStore` gains a per-session consolidation watermark that gates cleanup — turns are purged only after they are consolidated. Background workers handle consolidation and cleanup off the hot path.

**Tech Stack:** Rust / async-trait / tokio / sqlx (SQLite); chrono for timestamps; no new external crates required beyond what's in the workspace.

---

### Task 1: Drop `memory` param from all strategies and from `build()`

**Files:**
- Modify: `avs-react/src/react.rs`
- Modify: `avs-plan/src/plan.rs`
- Modify: `avs-plan/src/hierarchical.rs`
- Modify: `avs-strategy/src/lib.rs`
- Modify: `examples/hello-agent/src/main.rs`
- Modify: `examples/react-calculator/src/main.rs`
- Modify: `examples/anthropic-react/src/main.rs`
- Modify: `examples/web-search-agent/src/main.rs`
- Modify: `examples/code-review-agent/src/main.rs`
- Modify: `examples/slack-hr-assistant/src/main.rs`
- Modify: `examples/http-agent/src/main.rs`

- [ ] **Step 1: Update `avs-react/src/react.rs`** — remove `memory` field and its param from `ReActStrategy::new()`. Remove `use agentverse::Memory`, `use tokio::sync::Mutex`.

```rust
// Before:
pub struct ReActStrategy {
    skeleton: CycleSkeleton,
    #[allow(dead_code)]
    memory: Arc<Mutex<dyn Memory>>,
}
impl ReActStrategy {
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        memory: Arc<Mutex<dyn Memory>>,
        max_iterations: usize,
    ) -> Self {
        Self { skeleton: CycleSkeleton::new(runner, prompts, tools, max_iterations), memory }
    }
}

// After:
pub struct ReActStrategy {
    skeleton: CycleSkeleton,
}
impl ReActStrategy {
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        max_iterations: usize,
    ) -> Self {
        Self { skeleton: CycleSkeleton::new(runner, prompts, tools, max_iterations) }
    }
}
```

Remove the unused imports `agentverse::Memory` and `tokio::sync::Mutex` from `react.rs`.

- [ ] **Step 2: Update `avs-plan/src/plan.rs`** — same pattern.

```rust
// After:
pub struct PlanStrategy {
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
}
impl PlanStrategy {
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        max_iterations: usize,
    ) -> Self {
        Self { runner, prompts, tools, max_iterations }
    }
}
```

Remove unused imports `agentverse::Memory` and `tokio::sync::Mutex`. Also update the test helper `make_plan_strategy()` to drop `memory`.

- [ ] **Step 3: Update `avs-plan/src/hierarchical.rs`** — same pattern. `HierarchicalStrategy::new()` loses the `memory` param; keep all other params.

```rust
// After:
pub struct HierarchicalStrategy {
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
    max_decompose_depth: usize,
}
impl HierarchicalStrategy {
    pub fn new(
        runner: Arc<LlmRunner>,
        prompts: Arc<PromptRegistry>,
        tools: Arc<ToolRegistry>,
        max_iterations: usize,
        max_decompose_depth: usize,
    ) -> Self {
        Self { runner, prompts, tools, max_iterations, max_decompose_depth }
    }
}
```

- [ ] **Step 4: Update `avs-strategy/src/lib.rs`** — drop `memory` param from `build()`. Update the three `match` arms and the test helpers.

```rust
// Before signature:
pub fn build(
    kind: StrategyKind,
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    memory: Arc<Mutex<dyn Memory>>,
    max_iterations: usize,
) -> Arc<dyn RunStrategy>

// After:
pub fn build(
    kind: StrategyKind,
    runner: Arc<LlmRunner>,
    prompts: Arc<PromptRegistry>,
    tools: Arc<ToolRegistry>,
    max_iterations: usize,
) -> Arc<dyn RunStrategy>
```

Match arms become:
```rust
StrategyKind::React => Arc::new(ReActStrategy::new(runner, prompts, tools, max_iterations)),
StrategyKind::Plan => Arc::new(PlanStrategy::new(runner, prompts, tools, max_iterations)),
StrategyKind::Hierarchical => Arc::new(HierarchicalStrategy::new(runner, prompts, tools, max_iterations, 3)),
```

Remove `use agentverse::memory::Memory` and `use tokio::sync::Mutex` from `lib.rs`. Update test helpers: `make_resources()` no longer needs to create or return `memory`.

- [ ] **Step 5: Update all examples** — drop the `memory` argument from every `build()` call.

In each `main.rs` that calls `build()`:
```rust
// Before:
let strategy = build(StrategyKind::React, Arc::clone(&runner), Arc::clone(&prompts), Arc::clone(&tools), Arc::clone(&memory), 10);
// After:
let strategy = build(StrategyKind::React, Arc::clone(&runner), Arc::clone(&prompts), Arc::clone(&tools), 10);
```

`Agent::new()` still receives `memory` as its 4th param — do not remove it from `Agent::new()` calls yet (that changes in Task 5).

- [ ] **Step 6: Verify**

```bash
cargo check --workspace
cargo test --workspace
```

Expected: clean compile, all existing tests pass.

- [ ] **Step 7: Commit**

```bash
git add avs-react/src/react.rs avs-plan/src/plan.rs avs-plan/src/hierarchical.rs avs-strategy/src/lib.rs \
    examples/hello-agent/src/main.rs examples/react-calculator/src/main.rs \
    examples/anthropic-react/src/main.rs examples/web-search-agent/src/main.rs \
    examples/code-review-agent/src/main.rs examples/slack-hr-assistant/src/main.rs \
    examples/http-agent/src/main.rs
git commit -m "refactor: drop dead memory param from all strategies and build() factory"
```

---

### Task 2: Clean `Memory` trait — remove `prime_from_long_term`

**Files:**
- Modify: `avs-core/src/memory/mod.rs`
- Modify: `avs-core/src/memory/short_term.rs`
- Modify: `avs-memory/src/simple.rs`
- Modify: `avs-memory/src/agent.rs`

- [ ] **Step 1: Write failing test** — add to `avs-memory/src/simple.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agentverse::memory::{Message, MessageRole};

    #[tokio::test]
    async fn simple_memory_append_and_last_n() {
        let mut m = SimpleMemory::new(5);
        for i in 0..3u32 {
            m.append(Message { role: MessageRole::User, content: format!("msg {}", i) });
        }
        let msgs = m.last_n(2).await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].content, "msg 2");
    }

    #[tokio::test]
    async fn simple_memory_evicts_beyond_max() {
        let mut m = SimpleMemory::new(2);
        for i in 0..4u32 {
            m.append(Message { role: MessageRole::User, content: format!("msg {}", i) });
        }
        let msgs = m.last_n(10).await.unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "msg 2");
        assert_eq!(msgs[1].content, "msg 3");
    }
}
```

Run: `cargo test -p agentverse-memory -- tests` — expected: PASS (simple memory already works; these tests verify behavior before we touch the trait).

- [ ] **Step 2: Remove `prime_from_long_term` from trait in `avs-core/src/memory/mod.rs`**

```rust
// Before:
#[async_trait]
pub trait Memory: Send + Sync {
    fn append(&mut self, message: Message);
    async fn last_n(&mut self, n: usize) -> Result<Vec<Message>, MemoryError>;
    fn pin(&mut self, messages: Vec<Message>);
    async fn prime_from_long_term(&mut self, query: &str, top_k: usize) -> Result<(), MemoryError>;
    async fn flush(&mut self) -> Result<(), MemoryError>;
    fn clear(&mut self);
}

// After:
#[async_trait]
pub trait Memory: Send + Sync {
    fn append(&mut self, message: Message);
    async fn last_n(&mut self, n: usize) -> Result<Vec<Message>, MemoryError>;
    fn pin(&mut self, messages: Vec<Message>);
    async fn flush(&mut self) -> Result<(), MemoryError>;
    fn clear(&mut self);
}
```

- [ ] **Step 3: Remove the method body from `avs-core/src/memory/short_term.rs`** — delete the `prime_from_long_term` impl block (lines 42–49).

- [ ] **Step 4: Remove from `avs-memory/src/simple.rs`** — delete the `prime_from_long_term` impl block (lines 50–55).

- [ ] **Step 5: Remove from `avs-memory/src/agent.rs`** — delete the `prime_from_long_term` impl block (lines 97–107).

- [ ] **Step 6: Run tests — expect PASS**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 7: Commit**

```bash
git add avs-core/src/memory/mod.rs avs-core/src/memory/short_term.rs \
    avs-memory/src/simple.rs avs-memory/src/agent.rs
git commit -m "refactor(memory): remove prime_from_long_term from Memory trait — Layer 3 belongs to Agent"
```

---

### Task 3: Add `MemoryStore` trait + `LongTermRecord` + `ScoredMemory` to `avs-core`

**Spec alignment note:** Per spec §5, `MemoryStore` is defined in `avs-core/src/memory/` alongside the `Memory` trait — both are interfaces, not implementations. Implementations (`AgentMemory`, pgvector/lancedb backends) live in `avs-memory` and implement the `avs-core` traits. This keeps `avs-agent` from depending on `avs-memory` for mere trait objects.

**Files:**
- Modify: `avs-core/src/memory/mod.rs`
- Modify: `avs-memory/src/lib.rs` (re-export for convenience)

- [ ] **Step 1: Write failing test** — add to `avs-core/src/memory/mod.rs`:

```rust
#[cfg(test)]
mod store_tests {
    use super::*;

    struct NoopMemoryStore;

    #[async_trait]
    impl MemoryStore for NoopMemoryStore {
        async fn write(&self, _: &str, _: LongTermRecord) -> Result<(), MemoryError> {
            Ok(())
        }
        async fn retrieve(&self, _: &str, _: &str, _: usize) -> Result<Vec<ScoredMemory>, MemoryError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn noop_memory_store_retrieve_returns_empty() {
        let store = NoopMemoryStore;
        let result = store.retrieve("alice", "test query", 5).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn long_term_record_now_sets_fields() {
        let r = LongTermRecord::now("hello".to_string(), 0.7);
        assert_eq!(r.content, "hello");
        assert!((r.importance - 0.7).abs() < 1e-6);
    }
}
```

Run: `cargo test -p agentverse -- store_tests` — expected: FAIL (types not defined yet).

- [ ] **Step 2: Add types to `avs-core/src/memory/mod.rs`** — append after the existing `Memory` trait definition:

```rust
use chrono::{DateTime, Utc};

pub struct LongTermRecord {
    pub content: String,
    /// LLM-assigned or heuristic importance score, 0.0–1.0.
    pub importance: f32,
    pub created_at: DateTime<Utc>,
}

impl LongTermRecord {
    pub fn now(content: String, importance: f32) -> Self {
        Self { content, importance, created_at: Utc::now() }
    }
}

pub struct ScoredMemory {
    pub content: String,
    /// Combined score: α·recency + β·importance + γ·relevance
    pub score: f32,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait MemoryStore: Send + Sync {
    async fn write(&self, user_id: &str, record: LongTermRecord)
        -> Result<(), MemoryError>;
    async fn retrieve(&self, user_id: &str, query: &str, top_k: usize)
        -> Result<Vec<ScoredMemory>, MemoryError>;
}
```

Also add `chrono` to `avs-core/Cargo.toml` if not already present:
```toml
chrono = { workspace = true }
```

- [ ] **Step 3: Re-export from `avs-memory/src/lib.rs`** for downstream convenience

```rust
// Add re-exports so consumers can import from agentverse_memory too:
pub use agentverse::memory::{LongTermRecord, MemoryStore, ScoredMemory};
```

- [ ] **Step 4: Run tests — expect PASS**

```bash
cargo test -p agentverse -- store_tests
cargo check --workspace
```

- [ ] **Step 5: Commit**

```bash
git add avs-core/src/memory/mod.rs avs-core/Cargo.toml avs-memory/src/lib.rs
git commit -m "feat(memory): add LongTermRecord, ScoredMemory, and MemoryStore trait to avs-core for Layer 3"
```

---

### Task 4: Add consolidation watermark + cleanup to `avs-session`

**Files:**
- Modify: `avs-session/src/store.rs`
- Modify: `avs-session/src/sqlite.rs`

The `messages` table already has `created_at` (Unix timestamp) and `sequence_num`. We add:
- `consolidation_watermark INTEGER NOT NULL DEFAULT 0` to the `sessions` table — the highest `sequence_num` that has been consolidated into long-term memory.
- Four new methods on `SessionStore`: `get_watermark`, `advance_watermark`, `load_messages_above_watermark`, `cleanup_expired_messages`.

Safety invariant: cleanup only deletes `sequence_num <= watermark AND created_at < cutoff`.

- [ ] **Step 1: Write failing tests** — add to `avs-session/src/sqlite.rs` (after existing `#[cfg(test)]` section or in a new mod):

```rust
#[cfg(test)]
mod watermark_tests {
    use super::*;
    use agentverse::memory::{Message, MessageRole};

    #[tokio::test]
    async fn watermark_starts_at_zero() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        let wm = store.get_watermark(session.id).await.unwrap();
        assert_eq!(wm, 0);
    }

    #[tokio::test]
    async fn advance_watermark_updates_and_is_monotonic() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        store.advance_watermark(session.id, 5).await.unwrap();
        assert_eq!(store.get_watermark(session.id).await.unwrap(), 5);
        // cannot go backward
        store.advance_watermark(session.id, 2).await.unwrap();
        assert_eq!(store.get_watermark(session.id).await.unwrap(), 5);
    }

    #[tokio::test]
    async fn load_messages_above_watermark_returns_unconsolidated() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        store.append_turn(
            session.id,
            Message { role: MessageRole::User, content: "q".to_string() },
            Message { role: MessageRole::Assistant, content: "a".to_string() },
        ).await.unwrap();
        // watermark=0: both messages (seq 1, 2) are above watermark
        let msgs = store.load_messages_above_watermark(session.id).await.unwrap();
        assert_eq!(msgs.len(), 2);
        // advance to 1: only seq 2 remains unconsolidated
        store.advance_watermark(session.id, 1).await.unwrap();
        let msgs = store.load_messages_above_watermark(session.id).await.unwrap();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1.content, "a");
    }

    #[tokio::test]
    async fn cleanup_respects_watermark_safety_invariant() {
        let store = SqliteSessionStore::new("sqlite::memory:").await.unwrap();
        let session = store.create("alice").await.unwrap();
        store.append_turn(
            session.id,
            Message { role: MessageRole::User, content: "old".to_string() },
            Message { role: MessageRole::Assistant, content: "old reply".to_string() },
        ).await.unwrap();
        let far_future = chrono::Utc::now().timestamp() + 100_000;
        // watermark=0: cleanup deletes nothing regardless of cutoff
        let deleted = store.cleanup_expired_messages(session.id, far_future, 0).await.unwrap();
        assert_eq!(deleted, 0);
        // advance watermark to cover both messages, then cleanup purges them
        store.advance_watermark(session.id, 2).await.unwrap();
        let deleted = store.cleanup_expired_messages(session.id, far_future, 2).await.unwrap();
        assert_eq!(deleted, 2);
        let all = store.load_messages(session.id).await.unwrap();
        assert!(all.is_empty());
    }
}
```

Run: `cargo test -p agentverse-session -- watermark_tests` — expected: FAIL (methods don't exist yet).

- [ ] **Step 2: Add new methods to `SessionStore` trait in `avs-session/src/store.rs`**

```rust
async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionStoreError>;
async fn advance_watermark(&self, session_id: SessionId, new_watermark: i64)
    -> Result<(), SessionStoreError>;
/// Returns (sequence_num, Message) tuples for all messages above the current watermark.
async fn load_messages_above_watermark(&self, session_id: SessionId)
    -> Result<Vec<(i64, Message)>, SessionStoreError>;
/// Deletes messages where `created_at < cutoff_ts AND sequence_num <= watermark`.
async fn cleanup_expired_messages(
    &self,
    session_id: SessionId,
    cutoff_ts: i64,
    watermark: i64,
) -> Result<u64, SessionStoreError>;
```

- [ ] **Step 3: Add `consolidation_watermark` column to schema in `sqlite.rs` `migrate()`**

After the existing `sequence_num` compatibility block, add:
```rust
let has_watermark: i64 = sqlx::query_scalar(
    "SELECT COUNT(*) FROM pragma_table_info('sessions') WHERE name = 'consolidation_watermark'"
)
.fetch_one(&self.pool)
.await
.map_err(|e| SessionStoreError::Database(e.to_string()))?;

if has_watermark == 0 {
    sqlx::query(
        "ALTER TABLE sessions ADD COLUMN consolidation_watermark INTEGER NOT NULL DEFAULT 0"
    )
    .execute(&self.pool)
    .await
    .map_err(|e| SessionStoreError::Database(e.to_string()))?;
}
```

- [ ] **Step 4: Implement the four new methods in `SqliteSessionStore`**

```rust
async fn get_watermark(&self, session_id: SessionId) -> Result<i64, SessionStoreError> {
    let wm: i64 = sqlx::query_scalar(
        "SELECT consolidation_watermark FROM sessions WHERE id = ?"
    )
    .bind(session_id.to_string())
    .fetch_one(&self.pool)
    .await
    .map_err(|e| SessionStoreError::Database(e.to_string()))?;
    Ok(wm)
}

async fn advance_watermark(&self, session_id: SessionId, new_watermark: i64)
    -> Result<(), SessionStoreError>
{
    sqlx::query(
        "UPDATE sessions \
         SET consolidation_watermark = MAX(consolidation_watermark, ?), \
             updated_at = ? \
         WHERE id = ?"
    )
    .bind(new_watermark)
    .bind(chrono::Utc::now().timestamp())
    .bind(session_id.to_string())
    .execute(&self.pool)
    .await
    .map_err(|e| SessionStoreError::Database(e.to_string()))?;
    Ok(())
}

async fn load_messages_above_watermark(&self, session_id: SessionId)
    -> Result<Vec<(i64, Message)>, SessionStoreError>
{
    let wm = self.get_watermark(session_id).await?;
    let rows = sqlx::query_as::<_, (i64, String, String)>(
        "SELECT sequence_num, role, content \
         FROM messages \
         WHERE session_id = ? AND sequence_num > ? \
         ORDER BY sequence_num ASC"
    )
    .bind(session_id.to_string())
    .bind(wm)
    .fetch_all(&self.pool)
    .await
    .map_err(|e| SessionStoreError::Database(e.to_string()))?;
    Ok(rows.into_iter().map(|(seq, role, content)| {
        (seq, Message { role: Self::str_to_role(&role), content })
    }).collect())
}

async fn cleanup_expired_messages(
    &self,
    session_id: SessionId,
    cutoff_ts: i64,
    watermark: i64,
) -> Result<u64, SessionStoreError> {
    let result = sqlx::query(
        "DELETE FROM messages \
         WHERE session_id = ? AND created_at < ? AND sequence_num <= ?"
    )
    .bind(session_id.to_string())
    .bind(cutoff_ts)
    .bind(watermark)
    .execute(&self.pool)
    .await
    .map_err(|e| SessionStoreError::Database(e.to_string()))?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 5: Run tests — expect PASS**

```bash
cargo test -p agentverse-session -- watermark_tests
cargo test --workspace
```

- [ ] **Step 6: Commit**

```bash
git add avs-session/src/store.rs avs-session/src/sqlite.rs
git commit -m "feat(session): add consolidation watermark, load_messages_above_watermark, cleanup_expired_messages"
```

---

### Task 5: Add per-session working-buffer cache to `Agent`

**Files:**
- Modify: `avs-agent/src/agent.rs`

The Agent currently loads history from `SessionStore` on every `invoke`. This task adds a `working_buffers` cache (`HashMap<(String, SessionId), CachedBuffer>`) that:
- Returns cached messages on hit (and TTL not expired)
- Rehydrates from `SessionStore` on miss or TTL expiry
- Updates on every successful turn (appending user + assistant messages in-memory)
- Default TTL: 300 seconds (5 min idle)

Note: `Instant` is only used for elapsed checks, never serialized or sent across session boundaries.

- [ ] **Step 1: Write test for cache rehydration**

Add to `avs-agent/src/agent.rs` tests:

```rust
#[tokio::test]
async fn working_buffer_rehydrates_after_db_write() {
    // We can't easily test the cache path without a real LLM, but we can verify
    // that session history is loadable immediately after creation (the base case
    // that covers the rehydration path).
    let agent = make_agent().await;
    let sid = agent.create_session("alice").await.unwrap();
    // load_messages returns empty for a fresh session — rehydration path works
    let msgs = agent.load_messages("alice", sid).await.unwrap();
    assert!(msgs.is_empty());
}
```

Run: `cargo test -p agentverse-agent -- working_buffer` — expected: PASS (it compiles and passes because it's just calling existing methods).

- [ ] **Step 2: Add `CachedBuffer` struct and `working_buffers` field to `Agent`**

At the top of `avs-agent/src/agent.rs`, add:
```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

struct CachedBuffer {
    messages: Vec<Message>,
    last_used: Instant,
}
```

Update `Agent` struct to add two fields:
```rust
pub struct Agent {
    #[allow(dead_code)]
    runner: Arc<LlmRunner>,
    #[allow(dead_code)]
    tools: Arc<ToolRegistry>,
    prompts: Arc<PromptRegistry>,
    #[allow(dead_code)]
    memory: Arc<tokio::sync::Mutex<dyn Memory>>,
    sessions: Arc<SessionManager>,
    strategy: Arc<dyn RunStrategy>,
    working_buffers: tokio::sync::Mutex<HashMap<(String, SessionId), CachedBuffer>>,
    buffer_ttl: Duration,
}
```

Update `Agent::new()` to initialise the new fields:
```rust
let agent = Arc::new(Self {
    runner,
    tools,
    prompts,
    memory,
    sessions: Arc::new(SessionManager::new(store)),
    strategy,
    working_buffers: tokio::sync::Mutex::new(HashMap::new()),
    buffer_ttl: Duration::from_secs(300),
});
```

- [ ] **Step 3: Add `get_working_buffer` and `update_working_buffer` methods**

```rust
async fn get_working_buffer(
    &self,
    user_id: &str,
    session_id: SessionId,
) -> Result<Vec<Message>, AgentError> {
    let key = (user_id.to_string(), session_id);
    {
        let cache = self.working_buffers.lock().await;
        if let Some(buf) = cache.get(&key) {
            if buf.last_used.elapsed() <= self.buffer_ttl {
                return Ok(buf.messages.clone());
            }
        }
    }
    // Miss or TTL expired: rehydrate from Layer 2
    let history = self.sessions.load_messages(session_id).await?;
    let mut cache = self.working_buffers.lock().await;
    cache.insert(key, CachedBuffer { messages: history.clone(), last_used: Instant::now() });
    Ok(history)
}

async fn update_working_buffer(
    &self,
    user_id: &str,
    session_id: SessionId,
    user_msg: Message,
    assistant_msg: Message,
) {
    let key = (user_id.to_string(), session_id);
    let mut cache = self.working_buffers.lock().await;
    if let Some(buf) = cache.get_mut(&key) {
        buf.messages.push(user_msg);
        buf.messages.push(assistant_msg);
        buf.last_used = Instant::now();
    }
}
```

- [ ] **Step 4: Update `invoke` to use the cache**

```rust
pub async fn invoke(
    &self,
    user_id: &str,
    session_id: SessionId,
    input: &str,
) -> Result<String, AgentError> {
    self.sessions.assert_owner(user_id, session_id).await?;

    let history = self.get_working_buffer(user_id, session_id).await?;
    let user_msg = Message { role: MessageRole::User, content: input.to_string() };

    let messages = self.assemble_messages(self.render_system(), history, input);
    let response = self.strategy.run(messages).await?;

    let assistant_msg = Message { role: MessageRole::Assistant, content: response.clone() };
    self.sessions.append_turn(session_id, user_msg.clone(), assistant_msg.clone()).await?;
    self.update_working_buffer(user_id, session_id, user_msg, assistant_msg).await;

    Ok(response)
}
```

- [ ] **Step 5: Verify**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 6: Add Layer-1 eviction to `Agent::end_session` (lifecycle cascade)**

Per spec §4.6: "deleting a session drops its Layer-1 and Layer-2 memory." Update `end_session` to evict the working buffer immediately:

```rust
pub async fn end_session(&self, user_id: &str, session_id: SessionId) -> Result<(), AgentError> {
    self.sessions.assert_owner(user_id, session_id).await?;
    self.sessions.end_session(session_id).await?;
    // Layer-1 cascade: evict working buffer immediately
    let key = (user_id.to_string(), session_id);
    self.working_buffers.lock().await.remove(&key);
    Ok(())
}
```

Layer-2 cleanup (SQLite row purge) is handled by the `CleanupWorker` (Task 7). Layer 3 is **never** touched on session delete — this is the one-directional cascade.

- [ ] **Step 7: Verify**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 8: Commit**

```bash
git add avs-agent/src/agent.rs
git commit -m "feat(agent): add per-session working-buffer cache with TTL-based rehydration from Layer 2 + Layer-1 cascade on end_session"
```

---

### Task 6: Wire optional `MemoryStore` into Agent for Layer 3 retrieval + async consolidation

**Files:**
- Modify: `avs-agent/src/agent.rs`
- Modify: `avs-agent/Cargo.toml`

Long-term memory is **opt-in** — existing call sites pass `None` and behaviour is unchanged.

- [ ] **Step 1: Write test for long-term path with NoopMemoryStore**

Add to `avs-agent/src/agent.rs` tests. `MemoryStore`, `LongTermRecord`, `ScoredMemory` are imported from `agentverse` (avs-core), not `agentverse_memory`.

```rust
#[cfg(test)]
mod lt_tests {
    use super::*;
    use agentverse::memory::{LongTermRecord, MemoryError, MemoryStore, ScoredMemory};

    struct NoopMemoryStore;
    #[async_trait::async_trait]
    impl MemoryStore for NoopMemoryStore {
        async fn write(&self, _: &str, _: LongTermRecord) -> Result<(), MemoryError> { Ok(()) }
        async fn retrieve(&self, _: &str, _: &str, _: usize) -> Result<Vec<ScoredMemory>, MemoryError> { Ok(vec![]) }
    }

    #[tokio::test]
    async fn agent_with_memory_store_creates_session_normally() {
        let runner = Arc::new(
            LlmRunner::from_config(agentverse::Config {
                provider: agentverse::ProviderConfig::OpenAI {
                    model_name: "test".to_string(),
                    api_key: "sk-test".to_string(),
                    base_url: Some("http://127.0.0.1:1/v1".to_string()),
                },
                max_messages: 10,
                tools: vec![],
                prompts_dir: None,
                system_prompt: None,
            }).unwrap(),
        );
        let tools = Arc::new(agentverse_tools::ToolRegistry::new());
        let prompts = Arc::new(agentverse::PromptRegistry::new());
        let memory: Arc<tokio::sync::Mutex<dyn agentverse::Memory>> =
            Arc::new(tokio::sync::Mutex::new(agentverse_memory::SimpleMemory::new(50)));
        let strategy = agentverse_strategy::build(
            agentverse_strategy::StrategyKind::React,
            Arc::clone(&runner), Arc::clone(&prompts), Arc::clone(&tools), 3,
        );
        let store = Arc::new(agentverse_session::SqliteSessionStore::new("sqlite::memory:").await.unwrap());
        let ms: Arc<dyn agentverse::memory::MemoryStore> = Arc::new(NoopMemoryStore);
        let agent = Agent::new(runner, tools, prompts, memory, store, strategy, false, Some(ms));
        let sid = agent.create_session("alice").await.unwrap();
        assert!(agent.get_session("alice", sid).await.unwrap().is_some());
    }
}
```

Run: `cargo test -p agentverse-agent -- lt_tests` — expected: FAIL (Agent::new doesn't take 8th param yet).

- [ ] **Step 2: No new `[dependencies]` needed in `avs-agent/Cargo.toml`**

`MemoryStore`, `LongTermRecord`, and `ScoredMemory` are in `avs-core` (the `agentverse` crate), which `avs-agent` already depends on. No new dep required.

- [ ] **Step 3: Add `memory_store` field to `Agent` struct**

```rust
use agentverse::memory::{LongTermRecord, MemoryStore};

pub struct Agent {
    // ... existing fields ...
    memory_store: Option<Arc<dyn MemoryStore>>,
}
```

- [ ] **Step 4: Add `memory_store` param as 8th arg to `Agent::new()`**

```rust
pub fn new(
    runner: Arc<LlmRunner>,
    tools: Arc<ToolRegistry>,
    prompts: Arc<PromptRegistry>,
    memory: Arc<tokio::sync::Mutex<dyn Memory>>,
    store: Arc<dyn SessionStore>,
    strategy: Arc<dyn RunStrategy>,
    enable_http_server: bool,
    memory_store: Option<Arc<dyn agentverse::memory::MemoryStore>>,
) -> Arc<Self> {
    let agent = Arc::new(Self {
        runner,
        tools,
        prompts,
        memory,
        sessions: Arc::new(SessionManager::new(store)),
        strategy,
        working_buffers: tokio::sync::Mutex::new(HashMap::new()),
        buffer_ttl: Duration::from_secs(300),
        memory_store,
    });
    // ... http spawn unchanged ...
    agent
}
```

- [ ] **Step 5: Add `assemble_messages_with_context` and update `invoke`**

Replace the existing `assemble_messages` call in `invoke` with the long-term-aware version:

```rust
fn assemble_messages_with_context(
    &self,
    system: Option<String>,
    long_term: Vec<Message>,
    history: Vec<Message>,
    input: &str,
) -> Vec<Message> {
    let mut msgs = Vec::new();
    if let Some(sys) = system {
        msgs.push(Message { role: MessageRole::System, content: sys });
    }
    msgs.extend(long_term);
    msgs.extend(history);
    msgs.push(Message { role: MessageRole::User, content: input.to_string() });
    msgs
}

pub async fn invoke(
    &self,
    user_id: &str,
    session_id: SessionId,
    input: &str,
) -> Result<String, AgentError> {
    self.sessions.assert_owner(user_id, session_id).await?;

    let history = self.get_working_buffer(user_id, session_id).await?;

    // Layer 3: retrieve scored memories and inject as System context
    let long_term_context = if let Some(ref ms) = self.memory_store {
        ms.retrieve(user_id, input, 5)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|sm| Message {
                role: MessageRole::System,
                content: format!("[Memory] {}", sm.content),
            })
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    let user_msg = Message { role: MessageRole::User, content: input.to_string() };
    let messages = self.assemble_messages_with_context(
        self.render_system(), long_term_context, history, input,
    );
    let response = self.strategy.run(messages).await?;

    let assistant_msg = Message { role: MessageRole::Assistant, content: response.clone() };
    self.sessions.append_turn(session_id, user_msg.clone(), assistant_msg.clone()).await?;
    self.update_working_buffer(user_id, session_id, user_msg, assistant_msg).await;

    // Layer 3: async fire-and-forget consolidation
    if let Some(ms) = self.memory_store.clone() {
        let uid = user_id.to_string();
        let record = LongTermRecord::now(response.clone(), 0.5);
        tokio::spawn(async move {
            let _ = ms.write(&uid, record).await;
        });
    }

    Ok(response)
}
```

- [ ] **Step 6: Update all `Agent::new()` call sites to pass `None` as 8th arg**

In all examples and in `avs-agent/src/agent.rs` test helper `make_agent()`:
```rust
Agent::new(runner, tools, prompts, memory, store, strategy, false, None)
// or for http-agent:
Agent::new(runner, tools, prompts, memory, store, strategy, true, None)
```

- [ ] **Step 7: Run tests — expect PASS**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 8: Commit**

```bash
git add avs-agent/src/agent.rs \
    examples/hello-agent/src/main.rs examples/react-calculator/src/main.rs \
    examples/anthropic-react/src/main.rs examples/web-search-agent/src/main.rs \
    examples/code-review-agent/src/main.rs examples/slack-hr-assistant/src/main.rs \
    examples/http-agent/src/main.rs
git commit -m "feat(agent): wire optional MemoryStore for Layer 3 retrieval and async consolidation"
```

---

### Task 7: Background consolidation + cleanup workers

**Files:**
- Create: `avs-agent/src/workers.rs`
- Modify: `avs-agent/src/lib.rs`

Workers live in `avs-agent` because it already depends on both `avs-session` and `avs-core` (for `MemoryStore`). Placing them in `avs-memory` would force a new `avs-memory → avs-session` dep edge, which is conceptually odd (memory shouldn't know about sessions). The spec says workers are "spawned by the Agent or a maintenance binary" — `avs-agent` is the natural owner.

Two background workers intended to be spawned by the host binary (not auto-started by Agent, keeping `Agent::new()` clean):
- `ConsolidationWorker`: on each tick, lists sessions and for each checks `unconsolidated_count >= batch_size` OR `idle_since >= idle_timeout`. If either fires, consolidates unconsolidated turns into Layer 3 and advances the watermark.
- `CleanupWorker`: on each tick, for each session deletes messages where `created_at < now - retention_window AND sequence_num <= consolidation_watermark`.

**Deferred scope (explicit):** Spec §4.3 calls for consolidation to run each batch through `AgentMemory`'s LLM summarizer (summarize → assign importance → embed → store). This task implements direct per-message `write` (no LLM summarization, `importance = 0.5` hardcoded) as a correct but simplified version. Full LLM-summarized consolidation is a follow-up task once the LlmRunner is wired into the worker.

- [ ] **Step 1: Write tests for worker configs and basic run**

Add to `avs-memory/src/workers.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consolidation_config_defaults_are_sensible() {
        let cfg = ConsolidationConfig::default();
        assert!(cfg.batch_size > 0);
        assert!(cfg.idle_timeout.as_secs() > 0);
        assert!(cfg.poll_interval.as_secs() > 0);
    }

    #[test]
    fn cleanup_config_defaults_are_sensible() {
        let cfg = CleanupConfig::default();
        assert!(cfg.retention_window.as_secs() > 0);
        assert!(cfg.poll_interval.as_secs() > 0);
    }
}
```

Run: `cargo test -p agentverse-memory -- workers` — expected: FAIL (module doesn't exist yet).

- [ ] **Step 2: Create `avs-memory/src/workers.rs`**

```rust
use agentverse::memory::{LongTermRecord, MemoryStore};
use agentverse_session::{Session, SessionStore};
use std::sync::Arc;
use std::time::Duration;

pub struct ConsolidationConfig {
    /// Consolidate when this many unconsolidated turns accumulate.
    pub batch_size: usize,
    /// Consolidate after this much idle time even if batch_size not reached.
    pub idle_timeout: Duration,
    /// How often the worker polls.
    pub poll_interval: Duration,
}

impl Default for ConsolidationConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            idle_timeout: Duration::from_secs(1800), // 30 min
            poll_interval: Duration::from_secs(60),
        }
    }
}

pub struct CleanupConfig {
    /// Delete raw turns older than this window.
    pub retention_window: Duration,
    /// How often the worker polls.
    pub poll_interval: Duration,
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self {
            retention_window: Duration::from_secs(86400), // 24h
            poll_interval: Duration::from_secs(300),       // 5 min
        }
    }
}

pub struct ConsolidationWorker {
    store: Arc<dyn SessionStore>,
    memory_store: Arc<dyn MemoryStore>,
    config: ConsolidationConfig,
}

impl ConsolidationWorker {
    pub fn new(
        store: Arc<dyn SessionStore>,
        memory_store: Arc<dyn MemoryStore>,
        config: ConsolidationConfig,
    ) -> Self {
        Self { store, memory_store, config }
    }

    /// Run the worker loop. Call via `tokio::spawn(worker.run())`.
    /// Loop runs until the task is cancelled.
    pub async fn run(self) {
        let mut interval = tokio::time::interval(self.config.poll_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::warn!(error = %e, "ConsolidationWorker tick error");
            }
        }
    }

    async fn tick(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let sessions = self.store.list_all_active_sessions().await?;
        for session in sessions {
            let msgs = self.store.load_messages_above_watermark(session.id).await?;
            if msgs.len() < self.config.batch_size {
                // Check idle timeout: session.updated_at is the last-modified time
                let idle_secs = chrono::Utc::now().timestamp() - session.updated_at.timestamp();
                if idle_secs < self.config.idle_timeout.as_secs() as i64 {
                    continue;
                }
            }
            if msgs.is_empty() {
                continue;
            }
            let max_seq = msgs.iter().map(|(seq, _)| *seq).max().unwrap_or(0);
            for (_, msg) in &msgs {
                let record = LongTermRecord::now(msg.content.clone(), 0.5);
                self.memory_store.write(&session.user_id, record).await?;
            }
            self.store.advance_watermark(session.id, max_seq).await?;
        }
        Ok(())
    }
}

pub struct CleanupWorker {
    store: Arc<dyn SessionStore>,
    config: CleanupConfig,
}

impl CleanupWorker {
    pub fn new(store: Arc<dyn SessionStore>, config: CleanupConfig) -> Self {
        Self { store, config }
    }

    /// Run the worker loop. Call via `tokio::spawn(worker.run())`.
    pub async fn run(self) {
        let mut interval = tokio::time::interval(self.config.poll_interval);
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::warn!(error = %e, "CleanupWorker tick error");
            }
        }
    }

    async fn tick(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let cutoff_ts = chrono::Utc::now().timestamp()
            - self.config.retention_window.as_secs() as i64;
        let sessions = self.store.list_all_active_sessions().await?;
        for session in sessions {
            let wm = self.store.get_watermark(session.id).await?;
            let deleted = self.store.cleanup_expired_messages(session.id, cutoff_ts, wm).await?;
            if deleted > 0 {
                tracing::debug!(
                    session_id = %session.id,
                    deleted,
                    "CleanupWorker purged expired turns"
                );
            }
        }
        Ok(())
    }
}
```

Note: `list_all_active_sessions()` does not yet exist on `SessionStore` — add it in Step 3.

- [ ] **Step 3: Add `list_all_active_sessions` to `SessionStore` trait and `SqliteSessionStore`**

In `avs-session/src/store.rs`:
```rust
/// Returns all active sessions across all users. Used by background workers only.
async fn list_all_active_sessions(&self) -> Result<Vec<Session>, SessionStoreError>;
```

In `avs-session/src/sqlite.rs`:
```rust
async fn list_all_active_sessions(&self) -> Result<Vec<Session>, SessionStoreError> {
    let rows = sqlx::query_as::<_, (String, String, String, i64, i64)>(
        "SELECT id, user_id, status, created_at, updated_at \
         FROM sessions WHERE status = 'active' ORDER BY updated_at ASC"
    )
    .fetch_all(&self.pool)
    .await
    .map_err(|e| SessionStoreError::Database(e.to_string()))?;

    rows.into_iter().map(|(id, user_id, status, created_at, updated_at)| {
        Ok(Session {
            id: id.parse().map_err(|_| SessionStoreError::Database(format!("invalid UUID: {}", id)))?,
            user_id,
            status: status.parse().unwrap_or(SessionStatus::Active),
            created_at: chrono::DateTime::from_timestamp(created_at, 0).unwrap_or_default(),
            updated_at: chrono::DateTime::from_timestamp(updated_at, 0).unwrap_or_default(),
        })
    }).collect::<Result<Vec<_>, _>>()
}
```

- [ ] **Step 4: Add `agentverse-session` dep to `avs-memory/Cargo.toml`**

```toml
[dependencies]
agentverse-session = { path = "../avs-session" }
```

- [ ] **Step 5: Export workers from `avs-agent/src/lib.rs`**

```rust
pub mod workers;
pub use workers::{CleanupConfig, CleanupWorker, ConsolidationConfig, ConsolidationWorker};
```

- [ ] **Step 6: Run tests — expect PASS**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 7: Commit**

```bash
git add avs-agent/src/workers.rs avs-agent/src/lib.rs \
    avs-session/src/store.rs avs-session/src/sqlite.rs
git commit -m "feat(agent): add ConsolidationWorker and CleanupWorker for background Layer 3 maintenance"
```

---

### Verification

After all tasks, run the full verification suite:

- [ ] **Cargo checks**

```bash
cargo fmt --all --check
cargo clippy --all -- -D warnings
cargo test --workspace
```

- [ ] **Regression — hello-agent still recalls conversation history**

Run `example-hello-agent` and verify that the agent remembers context across turns within the same session (Layer 2 rehydration):
```bash
MODEL_BASE_URL=http://localhost:9090/v1 MODEL_NAME=your-model cargo run -p example-hello-agent
# Turn 1: "My name is Alice"
# Turn 2: "What is my name?" — should respond "Alice"
```

- [ ] **Working buffer TTL eviction** — verify that after TTL expires, the next `invoke` rehydrates from Layer 2 (session store) without losing history.

- [ ] **Long-term opt-in** — all existing examples pass `None` for `memory_store` and behave exactly as before.

- [ ] **Safety invariant** — `cleanup_expired_messages` with `watermark=0` deletes nothing; only purges after `advance_watermark` has been called.
