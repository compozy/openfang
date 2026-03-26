CREATE TABLE IF NOT EXISTS pack (
    pack_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    source_kind TEXT NOT NULL CHECK (source_kind IN ('bundled', 'external')),
    installed INTEGER NOT NULL DEFAULT 0 CHECK (installed >= 0),
    managed INTEGER NOT NULL DEFAULT 0 CHECK (managed >= 0),
    installed_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    objects_json TEXT NOT NULL DEFAULT '{}' CHECK (
        json_valid(objects_json) AND json_type(objects_json) = 'object'
    )
);

CREATE INDEX IF NOT EXISTS idx_pack_source_kind
    ON pack(source_kind, pack_id);

CREATE INDEX IF NOT EXISTS idx_pack_updated_at
    ON pack(updated_at DESC, pack_id ASC);
