CREATE TABLE IF NOT EXISTS trigger_runtime (
    trigger_id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL DEFAULT 1,
    fire_count INTEGER NOT NULL DEFAULT 0,
    max_fires INTEGER NOT NULL DEFAULT 0,
    cooldown_secs INTEGER NOT NULL DEFAULT 0,
    last_fired_at TEXT,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_trigger_runtime_updated_at
    ON trigger_runtime(updated_at);
