-- Task 42 hardening: ensure append-only indexes exist and add retention-friendly index.
CREATE INDEX IF NOT EXISTS idx_workflow_checkpoint_run
    ON workflow_checkpoint(run_id, created_at);

CREATE UNIQUE INDEX IF NOT EXISTS idx_artifact_version_artifact_id_version_no
    ON artifact_version(artifact_id, version_no);

CREATE INDEX IF NOT EXISTS idx_artifact_version_content_hash
    ON artifact_version(content_hash, artifact_version_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_doc_version_doc_id_version_no
    ON doc_version(doc_id, version_no);

CREATE INDEX IF NOT EXISTS idx_doc_version_content_hash
    ON doc_version(content_hash, doc_version_id);

CREATE INDEX IF NOT EXISTS idx_workflow_run_status_completed_at
    ON workflow_run(status, completed_at);
