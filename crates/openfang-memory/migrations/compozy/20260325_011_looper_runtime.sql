CREATE TABLE IF NOT EXISTS looper_run (
    looper_run_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES task(task_id) ON DELETE CASCADE,
    source_run_id TEXT,
    status TEXT NOT NULL CHECK (
        status IN (
            'pending',
            'running',
            'paused',
            'completed',
            'failed',
            'cancelled'
        )
    ),
    execution_policy_json TEXT NOT NULL CHECK (json_valid(execution_policy_json)),
    current_subtask_id TEXT,
    progress_json TEXT NOT NULL CHECK (json_valid(progress_json)),
    error_json TEXT CHECK (error_json IS NULL OR json_valid(error_json)),
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    CHECK (
        (status IN ('completed', 'failed', 'cancelled') AND completed_at IS NOT NULL)
        OR
        (status NOT IN ('completed', 'failed', 'cancelled') AND completed_at IS NULL)
    )
);

CREATE TABLE IF NOT EXISTS looper_subtask (
    looper_subtask_id TEXT PRIMARY KEY,
    looper_run_id TEXT NOT NULL REFERENCES looper_run(looper_run_id) ON DELETE CASCADE,
    subtask_id TEXT NOT NULL REFERENCES subtask(subtask_id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (
        status IN (
            'pending',
            'running',
            'completed',
            'failed',
            'cancelled'
        )
    ),
    dispatch_id TEXT,
    result_json TEXT CHECK (result_json IS NULL OR json_valid(result_json)),
    error_json TEXT CHECK (error_json IS NULL OR json_valid(error_json)),
    updated_at TEXT NOT NULL,
    UNIQUE(looper_run_id, subtask_id)
);

CREATE INDEX IF NOT EXISTS idx_looper_run_task_id
    ON looper_run(task_id, updated_at DESC, looper_run_id DESC);

CREATE INDEX IF NOT EXISTS idx_looper_run_status
    ON looper_run(status, updated_at DESC, looper_run_id DESC);

CREATE INDEX IF NOT EXISTS idx_looper_subtask_looper_run_id
    ON looper_subtask(looper_run_id, updated_at DESC, looper_subtask_id DESC);

CREATE INDEX IF NOT EXISTS idx_looper_subtask_status
    ON looper_subtask(status, updated_at DESC, looper_subtask_id DESC);

CREATE INDEX IF NOT EXISTS idx_looper_subtask_subtask_id
    ON looper_subtask(subtask_id, updated_at DESC, looper_subtask_id DESC);
