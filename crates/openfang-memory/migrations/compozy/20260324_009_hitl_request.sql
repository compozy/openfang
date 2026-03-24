CREATE TABLE IF NOT EXISTS hitl_request (
    hitl_request_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES workflow_run(run_id) ON DELETE CASCADE,
    step_id TEXT NOT NULL,
    dispatch_id TEXT REFERENCES agent_dispatch(dispatch_id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (
        kind IN ('clarification', 'approval', 'choice', 'freeform')
    ),
    status TEXT NOT NULL CHECK (
        status IN ('pending', 'answered', 'cancelled', 'timed_out')
    ),
    question TEXT NOT NULL,
    context_json TEXT NOT NULL CHECK (json_valid(context_json)),
    response_json TEXT CHECK (response_json IS NULL OR json_valid(response_json)),
    sequence_no INTEGER NOT NULL CHECK (sequence_no >= 1),
    created_at TEXT NOT NULL,
    answered_at TEXT,
    timeout_at TEXT,
    CHECK (
        (status = 'answered' AND response_json IS NOT NULL AND answered_at IS NOT NULL)
        OR
        (status != 'answered' AND response_json IS NULL AND answered_at IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_hitl_run
    ON hitl_request(run_id);

CREATE INDEX IF NOT EXISTS idx_hitl_dispatch
    ON hitl_request(dispatch_id);

CREATE INDEX IF NOT EXISTS idx_hitl_status
    ON hitl_request(status);

CREATE INDEX IF NOT EXISTS idx_hitl_run_step_sequence
    ON hitl_request(run_id, step_id, sequence_no);
