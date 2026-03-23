CREATE TABLE IF NOT EXISTS agent_session (
    session_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    label TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    message_count INTEGER NOT NULL DEFAULT 0,
    dispatch_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    compacted_at TEXT
);

CREATE TABLE IF NOT EXISTS agent_message (
    message_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    direction TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_agent_session_agent_id
    ON agent_session(agent_id);

CREATE INDEX IF NOT EXISTS idx_agent_message_agent_session
    ON agent_message(agent_id, session_id);

CREATE INDEX IF NOT EXISTS idx_agent_message_session
    ON agent_message(session_id);
