CREATE TABLE IF NOT EXISTS workflow_checkpoint (
    checkpoint_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_id TEXT,
    kind TEXT NOT NULL,
    data_json TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_workflow_checkpoint_run
    ON workflow_checkpoint(run_id, created_at);
