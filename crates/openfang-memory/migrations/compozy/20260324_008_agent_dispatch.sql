CREATE TABLE IF NOT EXISTS agent_dispatch (
    dispatch_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    step_id TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('call', 'send', 'spawn')),
    target_agent TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'pending',
            'running',
            'waiting_hitl',
            'completed',
            'failed',
            'cancelled'
        )
    ),
    input_json TEXT NOT NULL CHECK (json_valid(input_json)),
    result_json TEXT CHECK (result_json IS NULL OR json_valid(result_json)),
    error_json TEXT CHECK (error_json IS NULL OR json_valid(error_json)),
    attempt INTEGER NOT NULL DEFAULT 1 CHECK (attempt >= 1),
    parent_dispatch_id TEXT,
    spawned_agent_id TEXT,
    provider_driver TEXT,
    session_id TEXT,
    provider_resume_token TEXT,
    started_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    CHECK (
        (status IN ('completed', 'failed', 'cancelled') AND completed_at IS NOT NULL)
        OR
        (status NOT IN ('completed', 'failed', 'cancelled') AND completed_at IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_dispatch_run
    ON agent_dispatch(run_id, updated_at DESC, dispatch_id DESC);

CREATE INDEX IF NOT EXISTS idx_dispatch_parent
    ON agent_dispatch(parent_dispatch_id, updated_at DESC, dispatch_id DESC);

CREATE INDEX IF NOT EXISTS idx_dispatch_status
    ON agent_dispatch(status, updated_at DESC, dispatch_id DESC);
