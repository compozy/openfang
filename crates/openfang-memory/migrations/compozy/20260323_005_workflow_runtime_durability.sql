DROP TABLE IF EXISTS workflow_checkpoint__task16_old;
DROP TABLE IF EXISTS workflow_signal__task16_old;
DROP TABLE IF EXISTS workflow_run__task16_old;

DROP INDEX IF EXISTS idx_workflow_checkpoint_run;
DROP INDEX IF EXISTS idx_workflow_signal_run;
DROP INDEX IF EXISTS idx_workflow_signal_run_consumed;
DROP INDEX IF EXISTS idx_workflow_signal_run_name;
DROP INDEX IF EXISTS idx_workflow_run_workflow_id;
DROP INDEX IF EXISTS idx_workflow_run_status;
DROP INDEX IF EXISTS idx_workflow_run_updated_at;

ALTER TABLE workflow_checkpoint RENAME TO workflow_checkpoint__task16_old;
ALTER TABLE workflow_signal RENAME TO workflow_signal__task16_old;
ALTER TABLE workflow_run RENAME TO workflow_run__task16_old;

CREATE TABLE workflow_run (
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

INSERT INTO workflow_run (
    run_id,
    workflow_id,
    workflow_version,
    status,
    input_json,
    vars_json,
    current_step_id,
    waiting_kind,
    waiting_ref,
    active_dispatch_id,
    active_hitl_request_id,
    labels_json,
    metadata_json,
    error_json,
    started_at,
    updated_at,
    completed_at
)
SELECT
    run_id,
    workflow_id,
    COALESCE(NULLIF(workflow_version, ''), 'legacy'),
    CASE status
        WHEN 'waiting_signal' THEN 'waiting'
        WHEN 'paused' THEN 'interrupted'
        ELSE status
    END,
    COALESCE(input_json, '{}'),
    COALESCE(vars_json, '{}'),
    current_step_id,
    waiting_kind,
    waiting_ref,
    active_dispatch_id,
    active_hitl_request_id,
    COALESCE(labels_json, '[]'),
    COALESCE(metadata_json, '{}'),
    error_json,
    COALESCE(started_at, updated_at, datetime('now')),
    COALESCE(updated_at, started_at, datetime('now')),
    completed_at
FROM workflow_run__task16_old;

CREATE INDEX idx_workflow_run_workflow_id
    ON workflow_run(workflow_id);

CREATE INDEX idx_workflow_run_status
    ON workflow_run(status);

CREATE INDEX idx_workflow_run_updated_at
    ON workflow_run(updated_at);

CREATE TABLE workflow_checkpoint (
    checkpoint_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES workflow_run(run_id) ON DELETE CASCADE,
    step_id TEXT,
    kind TEXT NOT NULL,
    data_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(data_json)),
    created_at TEXT NOT NULL
);

INSERT INTO workflow_checkpoint (
    checkpoint_id,
    run_id,
    step_id,
    kind,
    data_json,
    created_at
)
SELECT
    checkpoint_id,
    run_id,
    step_id,
    kind,
    COALESCE(data_json, '{}'),
    COALESCE(created_at, datetime('now'))
FROM workflow_checkpoint__task16_old
WHERE EXISTS (
    SELECT 1
    FROM workflow_run
    WHERE workflow_run.run_id = workflow_checkpoint__task16_old.run_id
);

CREATE INDEX idx_workflow_checkpoint_run
    ON workflow_checkpoint(run_id, created_at);

CREATE TABLE workflow_signal (
    signal_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES workflow_run(run_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload_json)),
    source TEXT NOT NULL,
    consumed INTEGER NOT NULL DEFAULT 0 CHECK (consumed IN (0, 1)),
    created_at TEXT NOT NULL,
    consumed_at TEXT
);

INSERT INTO workflow_signal (
    signal_id,
    run_id,
    name,
    payload_json,
    source,
    consumed,
    created_at,
    consumed_at
)
SELECT
    signal_id,
    run_id,
    name,
    COALESCE(payload_json, '{}'),
    COALESCE(source, 'api'),
    COALESCE(consumed, 0),
    COALESCE(created_at, datetime('now')),
    consumed_at
FROM workflow_signal__task16_old
WHERE EXISTS (
    SELECT 1
    FROM workflow_run
    WHERE workflow_run.run_id = workflow_signal__task16_old.run_id
);

CREATE INDEX idx_workflow_signal_run
    ON workflow_signal(run_id);

CREATE INDEX idx_workflow_signal_run_consumed
    ON workflow_signal(run_id, consumed);

CREATE INDEX idx_workflow_signal_run_name
    ON workflow_signal(run_id, name);

DROP TABLE workflow_checkpoint__task16_old;
DROP TABLE workflow_signal__task16_old;
DROP TABLE workflow_run__task16_old;
