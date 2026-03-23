CREATE TABLE IF NOT EXISTS workflow_run (
    run_id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL,
    workflow_version TEXT,
    status TEXT NOT NULL CHECK (
        status IN (
            'pending',
            'running',
            'waiting_signal',
            'paused',
            'completed',
            'failed',
            'cancelled'
        )
    ),
    input_json TEXT NOT NULL DEFAULT '{}',
    vars_json TEXT NOT NULL DEFAULT '{}',
    current_step_id TEXT,
    waiting_kind TEXT,
    waiting_ref TEXT,
    active_dispatch_id TEXT,
    active_hitl_request_id TEXT,
    labels_json TEXT NOT NULL DEFAULT '[]',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    error_json TEXT,
    started_at TEXT,
    updated_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_workflow_run_workflow_id
    ON workflow_run(workflow_id);

CREATE INDEX IF NOT EXISTS idx_workflow_run_status
    ON workflow_run(status);

CREATE INDEX IF NOT EXISTS idx_workflow_run_updated_at
    ON workflow_run(updated_at);
