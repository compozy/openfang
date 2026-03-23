CREATE TABLE IF NOT EXISTS workflow_run (
    run_id TEXT PRIMARY KEY,
    workflow_id TEXT NOT NULL,
    workflow_version TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'pending',
            'running',
            'waiting',
            'completed',
            'failed',
            'cancelled',
            'interrupted'
        )
    ),
    input_json TEXT NOT NULL CHECK (json_valid(input_json)),
    vars_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(vars_json)),
    current_step_id TEXT,
    waiting_kind TEXT CHECK (
        waiting_kind IS NULL OR waiting_kind IN ('signal', 'hitl', 'dispatch')
    ),
    waiting_ref TEXT,
    active_dispatch_id TEXT,
    active_hitl_request_id TEXT,
    labels_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(labels_json)),
    metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata_json)),
    error_json TEXT CHECK (error_json IS NULL OR json_valid(error_json)),
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_workflow_run_workflow_id
    ON workflow_run(workflow_id);

CREATE INDEX IF NOT EXISTS idx_workflow_run_status
    ON workflow_run(status);

CREATE INDEX IF NOT EXISTS idx_workflow_run_updated_at
    ON workflow_run(updated_at);
