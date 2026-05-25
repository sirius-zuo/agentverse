# AgentVerse Memory Architecture — Research & Design

**Date:** 2026-05-25
**Status:** Design approved; implementation to follow in a separate plan.

---

## 1. Background & Motivation

AgentVerse currently has **no clear boundary between `Memory` and `Session`**, and the two overlap in confusing ways:

- The `Memory` trait (`avs-core/src/memory/mod.rs`: `append`, `last_n`, `pin`, `prime_from_long_term`, `flush`, `clear`) is carried into every strategy as `Arc<Mutex<dyn Memory>>` but is **never called** — both `ReActStrategy` and `PlanStrategy` mark it *"reserved for future per-step memory integration."* It is a phantom parameter.
- `SessionStore` / `SessionManager` (`avs-session`) is the **actual** conversation-history source. `Agent::invoke` loads history from it, assembles the prompt, and persists the turn.
- `LongTermBackend` + `AgentMemory` (summarizer + vector backend) + the `avs-memory-pgvector` / `avs-memory-lancedb` crates exist but are **wired to nothing**.

The root cause: the `Memory` trait tries to be *both* working memory (`last_n`, `pin`) *and* the long-term interface (`prime_from_long_term`) at once, then is threaded through strategies that ignore it. There is no named "long-term semantic" layer, so the half-built trait collapses two distinct responsibilities.

This document surveys how mature agent frameworks and dedicated memory systems draw the boundary, then specifies a finalized memory model for AgentVerse.

---

## 2. How the Field Designs Memory

### 2.1 The universal pattern: transcript vs. distilled knowledge

Every framework surveyed separates two things AgentVerse conflates:

- **Short-term / session transcript** — the literal message history for one thread; auto-managed, scoped by a thread/session id.
- **Long-term / distilled knowledge** — extracted facts, summaries, embeddings; cross-session, cross-user; explicitly written and retrieved.

### 2.2 Reference designs

| Framework | Short-term (transcript) | Long-term (knowledge) | Long-term write owner |
|---|---|---|---|
| **LangGraph** (clearest model) | **Checkpointer** — thread-scoped automatic state snapshots (`InMemorySaver` / `PostgresSaver`) | **Store** — cross-thread, namespaced KV + vector search (`put` / `get` / `search`) | Explicit calls |
| **Letta / MemGPT** | **Recall memory** (browsable history) | **Core memory** (in-context block) + **Archival memory** (vector store) | **Agent**, via memory tools |
| **Mem0** | conversation layer (kept separate) | vector + optional graph fact store | **Framework**, auto-extraction on `add()` |
| **Zep / Graphiti** | **Episode subgraph** (verbatim, timestamped) | **Semantic / community subgraphs** (bi-temporal) | **Framework**, async |
| **CrewAI** | per-run RAG buffer | SQLite long-term + entity memory | Framework, LLM-curated |
| **OpenAI Agents SDK** | **Thread / Session** (server or SQLite) | external RAG | App-managed |

Two takeaways:

- **LangGraph's checkpointer-vs-store split is the de-facto modern standard.** Checkpointer = automatic, thread-scoped transcript; Store = explicit, cross-thread, namespaced, vector-searchable knowledge. This maps almost 1:1 onto fixing AgentVerse: `SessionStore` ≈ checkpointer; AgentVerse is **missing** the Store equivalent (which the phantom `Memory` trait was half-meant to be).
- **MemGPT's three tiers** (core / recall / archival) map almost exactly onto the finalized model below (working / session / long-term).

### 2.3 Cognitive taxonomy

From the 2024 survey *"A Survey on the Memory Mechanism of LLM-based Agents"* (Zhang et al.) and the Stanford Generative Agents line of work, agent memory is commonly organized as:

| Type | Meaning | AgentVerse finalized layer |
|---|---|---|
| **Working** | what's in the context window right now | Layer 1 (working buffer) |
| **Episodic** | past interactions / events, timestamped | Layer 2 (session memory) |
| **Semantic** | distilled facts / knowledge across episodes | Layer 3 (long-term) |
| **Procedural** | how-to / skills / routing | system prompt (implicit) |

### 2.4 Seminal mechanisms worth borrowing

- **MemGPT** (Packer et al., 2023): treat the context window like RAM and external stores like disk; page information in and out under context pressure. Working memory is finite; everything else is retrieved on demand. → informs **Layer 1's eviction / compaction**.
- **Generative Agents** (Park et al., 2023): retrieve memories by a weighted score `recency + importance + relevance`, and periodically **reflect** to distill raw observations into higher-level memories. → informs **Layer 3's retrieval and consolidation**.
- **Memory operations vocabulary**: write/consolidate, read/retrieve, summarize/compress, forget/decay, reflect/synthesize. Recent work stresses that *management* (pruning, decay, consolidation) — not raw storage — is the real differentiator.
- **Consolidation timing**: read long-term *during* a turn; write/consolidate **asynchronously** *after* the turn, off the latency path. Preserve raw transcript as ground truth; keep a compressed semantic index alongside.

---

## 3. Current AgentVerse State

AgentVerse already owns most of the building blocks — they are simply mis-scoped and unassembled:

- `Memory` trait + `SimpleMemory` / `AgentMemory` (`avs-memory`) → an in-process window + summarizer. **This is the working-buffer layer**, but it was wired into strategies (where it is dead) instead of being an Agent-owned per-session cache.
- `SessionStore` / `SessionManager` (`avs-session`) → the persistent **session-memory** layer. Already correct.
- `LongTermBackend` (`avs-memory/src/traits.rs`) + `avs-memory-pgvector` / `avs-memory-lancedb` → the **long-term** storage substrate, unconnected.
- `AgentMemory` (`avs-memory/src/agent.rs`) → already implements window + summarization + backend store/search: the **consolidation pipeline**, but unused.

---

## 4. Finalized Memory Model

**Design principle: Session is an identity + lifecycle aggregate; Memory is a content store addressed by keys. They couple only at the key.** The session entity manipulates no memory; the `Agent` (the single LLM access point) orchestrates all layers using the session's `(user_id, session_id)`.

```
Session subsystem (avs-session)          Memory layers (Agent-orchestrated)
  Session { id, user_id,                   ├─ Layer 1 Working    key (user_id, session_id)  ephemeral / RAM
    created_at, status, ... }      ──key──▶ ├─ Layer 2 Session    key (user_id, session_id)  persistent (~24h)
  = identity + lifecycle only              └─ Layer 3 Long-term  key (user_id)              persistent
```

### 4.1 Layer 1 — Working memory (ephemeral, in-process)

- **Key:** `(user_id, session_id)`. **Storage:** in-process RAM buffer; never persisted.
- **Eviction:** TTL (configurable, ~5 min idle) **or** size — when the buffer reaches a configurable percentage of the model's context window (e.g. 80%), run a compaction strategy (summarize / drop oldest) to shrink it.
- **Rehydration:** when cold (TTL expired, evicted, or fresh process), rebuild from Layer 2.
- **Role:** the live context the strategy operates on for a turn.
- **Maps to:** repurposed `Memory` trait + `SimpleMemory` / `AgentMemory`; `AgentMemory`'s summarizer becomes the compaction strategy.

### 4.2 Layer 2 — Session memory (persistent but time-bounded transcript)

- **Key:** `(user_id, session_id)`. **Storage:** durable (`SessionStore`, SQLite / Postgres).
- **Retention:** raw turns are kept for a **cleanup window (~24h, configurable)**, then purged — not forever. Within the window it holds the complete transcript (exact recall, working-buffer rehydration); beyond it, older knowledge is represented only by long-term summaries.
- **Role:** short-term ground truth for the conversation; rehydrates Layer 1.
- **Maps to:** `avs-session` `SessionStore` — keep, add retention/cleanup. Cleanup is driven by a maintenance task, not the `Session` entity.

### 4.3 Layer 3 — Long-term memory (persistent, distilled)

- **Key:** `(user_id)` only. **Storage:** durable vector backend (`LongTermBackend` + pgvector / lancedb). Stores **summaries / distilled facts only — never raw turns**.
- **Write:** framework auto-extraction — the Agent runs an **async** consolidation pipeline after a turn (reusing `AgentMemory`'s summarizer), writing records with `created_at`, an LLM-assigned `importance`, and an embedding.
- **Read:** `score = α·recency + β·importance + γ·relevance` (exponential recency decay; stored importance; cosine relevance). Top-k results are injected into Layer 1 at prompt-assembly time.
- **Lifecycle:** outlives every session; deleting a session never deletes long-term memory.
- **Maps to:** a new `MemoryStore` over `LongTermBackend` — wire up.

### 4.4 Session retention & consolidation cadence (couples Layers 2 & 3)

Two decoupled windows plus one safety rule:

- **Cleanup window (~24h):** raw session turns are purged after this age — **per-turn-age, rolling**. This applies even to still-active long sessions, which therefore keep only a rolling 24h of raw turns plus permanent long-term memory.
- **Consolidation cadence (size-or-time, whichever fires first):** trigger an async consolidation pass when **N new turns** accumulate past the watermark (batch size **N** = the summarization sliding window) **or** after **T idle** (e.g. 30 min, which flushes the tail). Both run far inside the 24h window so long-term stays fresh enough for cross-session continuity. (Consolidating only at the 24h trailing edge would break that continuity — a new session within 24h would not yet see the prior session's knowledge.)
  - Each pass feeds the **existing long-term summary + the N new turns** to the summarizer, producing coherent, deduplicated updates (Mem0-style), then advances the watermark under a **per-session lock** so the count and idle paths never double-consolidate.
  - This is **distinct** from Layer-1 working-buffer compaction. They may share a summarizer implementation, but they have different triggers and destinations. Name this the consolidation **batch size N** — `last_n` is already the `Memory` retrieval method.
- **Safety invariant:** a turn is purged **only after** it has been consolidated into long-term, tracked by a per-session **consolidation watermark**. If consolidation lags or fails, cleanup defers — no data is ever lost.
- **Operationalized by** background maintenance tasks (a consolidation worker + a cleanup worker), not by the `Session` entity.

### 4.5 Session entity (identity + lifecycle only)

- `Session { session_id, user_id, created_at, status, ... }` — pure bookkeeping; **manipulates no memory**.
- Provides the `(user_id, session_id)` key plus lifecycle / authorization. The Agent uses the key to address Layers 1 and 2.
- **Maps to:** `avs-session` `Session` / `SessionManager` — keep.

### 4.6 Orchestration (Agent, per turn)

1. Get / rehydrate the Layer-1 buffer for `(user, session)` (rebuild from Layer 2 if cold).
2. Retrieve scored Layer-3 memories for the input (recency + importance + relevance).
3. Assemble the prompt: system + long-term context + working buffer + input.
4. If the buffer exceeds the context threshold → compact before sending.
5. `strategy.run(messages)` — the strategy is memory-agnostic.
6. Append the turn to Layer 1 and Layer 2.
7. Asynchronously consolidate into Layer 3 (per the cadence above).

**Lifecycle cascade (one-directional):** deleting a session drops its Layer-1 and Layer-2 memory; it **never** touches Layer 3.

### 4.7 The boundary, in one line each

- **Session** = identity / lifecycle (the folder and its metadata).
- **Working memory** = the live in-RAM context (a cache, evicted by TTL or size).
- **Session memory** = the verbatim transcript (a persistent, time-bounded log).
- **Long-term memory** = distilled cross-session knowledge (the user's profile).

---

## 5. Implementation Direction (mapping to crates)

A separate implementation plan will detail the task-by-task changes. The shape:

- `avs-strategy/src/lib.rs`, `avs-react/src/react.rs`, `avs-plan/src/plan.rs` — **drop the `memory` param**; strategies become pure `Vec<Message> -> String`.
- `avs-core/src/memory/` — repurpose `Memory` into the **working-buffer** interface (append, view, compact, eviction hooks); remove `prime_from_long_term` (a Layer-3 concern). Define a separate long-term `MemoryStore` trait: `retrieve(user_id, query, top_k) -> Vec<ScoredMemory>` + `write(user_id, record)`.
- `avs-memory` — `SimpleMemory` / `AgentMemory` become working-buffer impls with TTL + context-percentage eviction and a compaction strategy; a new scoring module (`α·recency + β·importance + γ·relevance`); `AgentMemory`'s summarizer recast as the Layer-3 consolidation pipeline (summarize → assign importance → embed → store).
- `avs-session` — keep `Session` / `SessionStore`; add a per-turn timestamp + per-session **consolidation watermark**; add a cleanup operation that purges turns older than the retention window **and** below the watermark. No memory policy in the `Session` entity itself.
- background workers — a **consolidation worker** (session → long-term, per the cadence) and a **cleanup worker** (purge consolidated + expired turns), spawned by the Agent or a maintenance binary.
- `avs-agent/src/agent.rs` — the Agent owns a per-`(user, session)` working-buffer cache (a map with TTL eviction) plus an optional `MemoryStore`; implements the orchestration above and the one-directional delete cascade. `invoke_stateless` = working buffer with no persistence; `invoke` = the full pipeline.
- examples — long-term memory is opt-in via a builder flag (default off, so existing examples behave unchanged).

---

## 6. References

Seminal papers:

- **MemGPT: Towards LLMs as Operating Systems** — Packer et al., 2023 (arXiv:2310.08560).
- **Generative Agents: Interactive Simulacra of Human Behavior** — Park et al., Stanford, 2023.
- **A Survey on the Memory Mechanism of Large Language Model-based Agents** — Zhang et al., 2024 (arXiv:2404.13501).
- **Mem0: Building Production-Ready AI Agents with Scalable Long-Term Memory** — 2025 (arXiv:2504.19413).
- **Zep: A Temporal Knowledge Graph Architecture for Agent Memory** — 2025 (arXiv:2501.13956).

Framework documentation:

- LangGraph persistence (checkpointers) and the cross-thread Store — LangChain/LangGraph docs.
- LlamaIndex `Memory` / `ChatMemoryBuffer` / `VectorMemory` — LlamaIndex docs.
- CrewAI memory (short-term / long-term / entity / contextual) — CrewAI docs.
- OpenAI Agents SDK sessions — OpenAI Agents SDK docs.
- AutoGen memory & RAG — Microsoft AutoGen docs.
- Letta / MemGPT memory tiers (core / recall / archival) — Letta docs.
- Cognee ECL (extract–cognify–load) pipeline — Cognee docs.
