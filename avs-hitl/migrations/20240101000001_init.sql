CREATE TABLE IF NOT EXISTS hitl_approvals (
    id          TEXT    PRIMARY KEY NOT NULL,
    session_id  TEXT    NOT NULL,
    kind_json   TEXT    NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'pending',
    decision    TEXT,
    created_at  INTEGER NOT NULL,
    resolved_at INTEGER,
    expires_at  INTEGER
);
