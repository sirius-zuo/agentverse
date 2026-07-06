CREATE TABLE IF NOT EXISTS sessions (
    id                      TEXT    PRIMARY KEY NOT NULL,
    user_id                 TEXT    NOT NULL,
    status                  TEXT    NOT NULL DEFAULT 'active',
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL,
    consolidation_watermark INTEGER NOT NULL DEFAULT 0,
    skill_context_json      TEXT,
    phase_opening_context   TEXT,
    interrupted_state       TEXT
);

CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);

CREATE TABLE IF NOT EXISTS messages (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id   TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role         TEXT    NOT NULL,
    content      TEXT    NOT NULL,
    sequence_num INTEGER NOT NULL,
    created_at   INTEGER NOT NULL,
    UNIQUE(session_id, sequence_num)
);

CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id, sequence_num);
