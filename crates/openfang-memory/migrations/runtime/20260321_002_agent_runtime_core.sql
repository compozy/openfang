CREATE TABLE IF NOT EXISTS agent_runtime (
    agent_id TEXT PRIMARY KEY,
    loaded INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    mode TEXT NOT NULL,
    healthy INTEGER NOT NULL DEFAULT 1,
    active_session_id TEXT,
    active_dispatches INTEGER NOT NULL DEFAULT 0,
    last_active_at TEXT,
    updated_at TEXT NOT NULL
);
