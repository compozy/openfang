CREATE TABLE IF NOT EXISTS workflow_signal (
    signal_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES workflow_run(run_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload_json)),
    source TEXT NOT NULL,
    consumed INTEGER NOT NULL DEFAULT 0 CHECK (consumed IN (0, 1)),
    created_at TEXT NOT NULL,
    consumed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_workflow_signal_run
    ON workflow_signal(run_id);

CREATE INDEX IF NOT EXISTS idx_workflow_signal_run_consumed
    ON workflow_signal(run_id, consumed);

CREATE INDEX IF NOT EXISTS idx_workflow_signal_run_name
    ON workflow_signal(run_id, name);
