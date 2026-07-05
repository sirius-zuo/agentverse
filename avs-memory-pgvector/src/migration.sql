-- Requires: CREATE EXTENSION IF NOT EXISTS vector;
-- NOTE: vector(1536) must match your Embedder's dimensions; adjust before applying.

CREATE TABLE IF NOT EXISTS agent_memory (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id TEXT NOT NULL,
    content TEXT NOT NULL,
    importance REAL NOT NULL DEFAULT 0.5,
    embedding vector(1536),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_memory_user ON agent_memory (user_id);
CREATE INDEX IF NOT EXISTS idx_memory_embedding ON agent_memory
    USING ivfflat (embedding vector_cosine_ops) WITH (lists = 100);
