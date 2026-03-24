CREATE TABLE IF NOT EXISTS task (
    task_id TEXT PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    source_run_id TEXT,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('planned', 'in_progress', 'completed', 'failed', 'cancelled')
    ),
    priority TEXT NOT NULL CHECK (
        priority IN ('low', 'medium', 'high', 'critical')
    ),
    complexity TEXT NOT NULL CHECK (
        complexity IN ('low', 'medium', 'high')
    ),
    position INTEGER NOT NULL,
    owner_kind TEXT NOT NULL CHECK (
        owner_kind IN ('agent', 'agent_group', 'user')
    ),
    owner_ref TEXT NOT NULL,
    created_by_kind TEXT NOT NULL CHECK (
        created_by_kind IN ('agent', 'agent_group', 'user')
    ),
    created_by_ref TEXT NOT NULL,
    repository_refs_json TEXT NOT NULL CHECK (
        json_valid(repository_refs_json) AND json_type(repository_refs_json) = 'array'
    ),
    label_refs_json TEXT NOT NULL CHECK (
        json_valid(label_refs_json) AND json_type(label_refs_json) = 'array'
    ),
    artifact_refs_json TEXT NOT NULL CHECK (
        json_valid(artifact_refs_json) AND json_type(artifact_refs_json) = 'array'
    ),
    doc_refs_json TEXT NOT NULL CHECK (
        json_valid(doc_refs_json) AND json_type(doc_refs_json) = 'array'
    ),
    file_refs_json TEXT NOT NULL CHECK (
        json_valid(file_refs_json) AND json_type(file_refs_json) = 'array'
    ),
    metadata_json TEXT NOT NULL CHECK (
        json_valid(metadata_json) AND json_type(metadata_json) = 'object'
    ),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    CHECK (
        (status IN ('completed', 'failed', 'cancelled') AND completed_at IS NOT NULL)
        OR
        (status NOT IN ('completed', 'failed', 'cancelled') AND completed_at IS NULL)
    )
);

CREATE TABLE IF NOT EXISTS subtask (
    subtask_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL REFERENCES task(task_id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (
        kind IN (
            'doc_change',
            'code_change',
            'research',
            'review_item',
            'validation',
            'other'
        )
    ),
    status TEXT NOT NULL CHECK (
        status IN ('planned', 'ready', 'in_progress', 'completed', 'failed', 'cancelled')
    ),
    complexity TEXT NOT NULL CHECK (
        complexity IN ('low', 'medium', 'high')
    ),
    position INTEGER NOT NULL,
    assignee_kind TEXT CHECK (
        assignee_kind IS NULL OR assignee_kind IN ('agent', 'agent_group', 'user')
    ),
    assignee_ref TEXT,
    depends_on_json TEXT NOT NULL CHECK (
        json_valid(depends_on_json) AND json_type(depends_on_json) = 'array'
    ),
    parallelizable INTEGER NOT NULL CHECK (parallelizable IN (0, 1)),
    input_json TEXT NOT NULL CHECK (json_valid(input_json)),
    result_json TEXT CHECK (result_json IS NULL OR json_valid(result_json)),
    metadata_json TEXT NOT NULL CHECK (
        json_valid(metadata_json) AND json_type(metadata_json) = 'object'
    ),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT,
    CHECK (
        (assignee_kind IS NULL AND assignee_ref IS NULL)
        OR
        (assignee_kind IS NOT NULL AND assignee_ref IS NOT NULL)
    ),
    CHECK (
        (status IN ('completed', 'failed', 'cancelled') AND completed_at IS NOT NULL)
        OR
        (status NOT IN ('completed', 'failed', 'cancelled') AND completed_at IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_task_status
    ON task(status, position ASC, task_id ASC);

CREATE INDEX IF NOT EXISTS idx_task_priority
    ON task(priority, position ASC, task_id ASC);

CREATE INDEX IF NOT EXISTS idx_task_source_run
    ON task(source_run_id, position ASC, task_id ASC);

CREATE INDEX IF NOT EXISTS idx_subtask_task_id
    ON subtask(task_id, position ASC, subtask_id ASC);

CREATE INDEX IF NOT EXISTS idx_subtask_status
    ON subtask(status, task_id, position ASC, subtask_id ASC);
