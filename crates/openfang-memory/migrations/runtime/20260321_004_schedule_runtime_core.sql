CREATE TABLE IF NOT EXISTS schedule_runtime (
    schedule_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1,
    last_run TEXT,
    next_run TEXT,
    last_status TEXT,
    consecutive_errors INTEGER NOT NULL DEFAULT 0,
    one_shot INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schedule_execution (
    execution_id TEXT PRIMARY KEY,
    schedule_id TEXT NOT NULL,
    fired_at TEXT NOT NULL,
    status TEXT NOT NULL,
    effect_json TEXT,
    error TEXT
);

CREATE INDEX IF NOT EXISTS idx_schedule_execution_schedule
    ON schedule_execution(schedule_id);

CREATE INDEX IF NOT EXISTS idx_schedule_execution_fired
    ON schedule_execution(fired_at);
