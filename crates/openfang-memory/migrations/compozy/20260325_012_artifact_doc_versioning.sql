CREATE TABLE IF NOT EXISTS artifact (
    artifact_id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    current_version_id TEXT NOT NULL
        REFERENCES artifact_version(artifact_version_id)
        DEFERRABLE INITIALLY DEFERRED,
    metadata_json TEXT NOT NULL CHECK (
        json_valid(metadata_json) AND json_type(metadata_json) = 'object'
    ),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS artifact_version (
    artifact_version_id TEXT PRIMARY KEY,
    artifact_id TEXT NOT NULL REFERENCES artifact(artifact_id) ON DELETE CASCADE,
    version_no INTEGER NOT NULL CHECK (version_no >= 1),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    content_hash TEXT NOT NULL,
    created_by_kind TEXT CHECK (
        created_by_kind IS NULL OR created_by_kind IN (
            'run',
            'dispatch',
            'looper_run',
            'api',
            'agent'
        )
    ),
    created_by_ref TEXT,
    created_at TEXT NOT NULL,
    CHECK (
        (created_by_kind IS NULL AND created_by_ref IS NULL)
        OR
        (created_by_kind IS NOT NULL AND created_by_ref IS NOT NULL)
    )
);

CREATE TABLE IF NOT EXISTS doc (
    doc_id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    current_version_id TEXT NOT NULL
        REFERENCES doc_version(doc_version_id)
        DEFERRABLE INITIALLY DEFERRED,
    metadata_json TEXT NOT NULL CHECK (
        json_valid(metadata_json) AND json_type(metadata_json) = 'object'
    ),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS doc_version (
    doc_version_id TEXT PRIMARY KEY,
    doc_id TEXT NOT NULL REFERENCES doc(doc_id) ON DELETE CASCADE,
    version_no INTEGER NOT NULL CHECK (version_no >= 1),
    content_json TEXT NOT NULL CHECK (json_valid(content_json)),
    content_hash TEXT NOT NULL,
    created_by_kind TEXT CHECK (
        created_by_kind IS NULL OR created_by_kind IN (
            'run',
            'dispatch',
            'looper_run',
            'api',
            'agent'
        )
    ),
    created_by_ref TEXT,
    created_at TEXT NOT NULL,
    CHECK (
        (created_by_kind IS NULL AND created_by_ref IS NULL)
        OR
        (created_by_kind IS NOT NULL AND created_by_ref IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_artifact_created_at
    ON artifact(created_at DESC, artifact_id DESC);

CREATE INDEX IF NOT EXISTS idx_artifact_type_created_at
    ON artifact(type, created_at DESC, artifact_id DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_artifact_version_artifact_id_version_no
    ON artifact_version(artifact_id, version_no);

CREATE INDEX IF NOT EXISTS idx_artifact_version_content_hash
    ON artifact_version(content_hash, artifact_version_id);

CREATE INDEX IF NOT EXISTS idx_artifact_version_created_at
    ON artifact_version(created_at);

CREATE INDEX IF NOT EXISTS idx_doc_created_at
    ON doc(created_at DESC, doc_id DESC);

CREATE INDEX IF NOT EXISTS idx_doc_type_created_at
    ON doc(type, created_at DESC, doc_id DESC);

CREATE UNIQUE INDEX IF NOT EXISTS idx_doc_version_doc_id_version_no
    ON doc_version(doc_id, version_no);

CREATE INDEX IF NOT EXISTS idx_doc_version_content_hash
    ON doc_version(content_hash, doc_version_id);

CREATE INDEX IF NOT EXISTS idx_doc_version_created_at
    ON doc_version(created_at);
