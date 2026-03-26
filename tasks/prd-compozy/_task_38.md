## markdown

## status: done

<task_context>
<domain>api/artifacts</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task37</dependencies>
</task_context>

# Task 38.0: Artifact And Doc Standalone Read Endpoints

## Overview

Add standalone read endpoints for artifacts and documents that are not scoped to a specific task.
Task 37 (Artifact And Doc Versioning) creates the `artifact`, `artifact_version`, `doc`, and
`doc_version` tables in `compozy.db`. Task 32 (Task And Subtask Control-Plane) exposes task-scoped
artifact and doc listings via `GET /api/v1/tasks/{id}/artifacts` and `GET /api/v1/tasks/{id}/docs`.
However, the API spec defines standalone artifact and document endpoints for direct access,
independent of task context. This task adds those endpoints.

All endpoints are read-only. Artifacts and documents are created by agents and workflows through
domain primitives, not through direct API CRUD. Pagination follows `{ items, next_cursor }` per
ADR-034. Filtering by `artifact_type`/`doc_type` and `task_id` is supported.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement `GET /api/v1/artifacts` — paginated list of all artifacts. Each item includes `id`,
  `artifact_type`, `task_id`, `current_version_id`, `created_at`, and `updated_at`. Supports
  `limit` (default 50, max 200), `cursor`, and filters: `artifact_type`, `task_id`, `q`.
- Implement `GET /api/v1/artifacts/{id}` — artifact detail with the current version inline.
  Returns `id`, `artifact_type`, `task_id`, `current_version_id`, `current_version` (inline
  version object with `id`, `version_number`, `content_hash`, `created_at`), `created_at`,
  `updated_at`. Returns 404 with standard error envelope for unknown IDs.
- Implement `GET /api/v1/artifacts/{id}/versions` — paginated list of all versions for an
  artifact, ordered by `version_number` descending. Each item includes `id`, `version_number`,
  `content_hash`, `created_by`, `created_at`.
- Implement `GET /api/v1/docs` — paginated list of all documents. Each item includes `id`,
  `doc_type`, `task_id`, `current_version_id`, `created_at`, `updated_at`. Supports `limit`,
  `cursor`, and filters: `doc_type`, `task_id`, `q`.
- Implement `GET /api/v1/docs/{id}` — document detail with the current version inline. Same
  shape pattern as artifact detail. Returns 404 for unknown IDs.
- Implement `GET /api/v1/docs/{id}/versions` — paginated version history for a document, same
  pattern as artifact versions.
- All endpoints are read-only. No POST, PUT, or DELETE routes.
- All list endpoints return `{ items, next_cursor }` per ADR-034 conventions.
- Wire routes in `crates/openfang-api/src/server.rs` under `/api/v1/artifacts` and `/api/v1/docs`.
- Queries read from `compozy.db` tables created by task 37: `artifact`, `artifact_version`, `doc`,
  `doc_version`.
</requirements>

## Subtasks

- [x] 38.1 Define response types in `crates/openfang-types/`: `ArtifactSummary`, `ArtifactDetail`,
      `ArtifactVersionSummary`, `DocSummary`, `DocDetail`, `DocVersionSummary`. All derive
      `Serialize` and `Deserialize`. Place in `artifact.rs` and `doc.rs` respectively (create or
      extend existing modules from task 37).
- [x] 38.2 Add list and detail query methods to the artifact and doc stores in
      `crates/openfang-memory/src/artifact.rs` and `crates/openfang-memory/src/doc.rs`. Methods:
      `list_artifacts(filters, limit, cursor)`, `get_artifact(id)`,
      `list_artifact_versions(artifact_id, limit, cursor)`, and the equivalent for docs. Use the
      store interfaces created by task 37.
- [x] 38.3 Register the `/api/v1/artifacts` router group in `server.rs` with three routes:
      `GET /` (list), `GET /:id` (detail), `GET /:id/versions` (version history).
- [x] 38.4 Register the `/api/v1/docs` router group in `server.rs` with the same three routes:
      `GET /` (list), `GET /:id` (detail), `GET /:id/versions` (version history).
- [x] 38.5 Implement the `list_artifacts`, `get_artifact`, and `list_artifact_versions` handlers.
      Apply `artifact_type`, `task_id`, and `q` filters. Apply cursor-based pagination. The detail
      handler joins the current version inline.
- [x] 38.6 Implement the `list_docs`, `get_doc`, and `list_doc_versions` handlers. Same pattern as
      the artifact handlers but reading from `doc` and `doc_version` tables.
- [x] 38.7 Add route-level and handler-level tests. See the Tests section below.

## Implementation Details

Artifacts and documents in Compozy use a stable-identity-plus-immutable-versions model. The
`artifact` table holds the stable identity (`id`, `artifact_type`, `task_id`,
`current_version_id`). The `artifact_version` table holds immutable revisions
(`id`, `artifact_id`, `version_number`, `content_hash`, `created_by`, `created_at`). Documents
follow the same pattern with `doc` and `doc_version`.

### Artifact List Item Shape

```json
{
  "id": "artifact_001",
  "artifact_type": "prd",
  "task_id": "task_001",
  "current_version_id": "artifact_v3",
  "created_at": "2026-03-21T14:00:00Z",
  "updated_at": "2026-03-21T14:30:00Z"
}
```

### Artifact Detail Shape

```json
{
  "id": "artifact_001",
  "artifact_type": "prd",
  "task_id": "task_001",
  "current_version_id": "artifact_v3",
  "current_version": {
    "id": "artifact_v3",
    "version_number": 3,
    "content_hash": "sha256:abc123...",
    "created_by": {
      "kind": "agent",
      "ref": "prd-writer"
    },
    "created_at": "2026-03-21T14:30:00Z"
  },
  "created_at": "2026-03-21T14:00:00Z",
  "updated_at": "2026-03-21T14:30:00Z"
}
```

### Artifact Version List Item Shape

```json
{
  "id": "artifact_v3",
  "version_number": 3,
  "content_hash": "sha256:abc123...",
  "created_by": {
    "kind": "agent",
    "ref": "prd-writer"
  },
  "created_at": "2026-03-21T14:30:00Z"
}
```

Document shapes follow the same pattern, replacing `artifact_type` with `doc_type` and
`artifact_id` with `doc_id`.

### Relevant Files

- `crates/openfang-api/src/routes.rs` — add artifact and doc handlers
- `crates/openfang-api/src/server.rs` — router registration
- `crates/openfang-types/src/artifact.rs` — artifact types (from task 37)
- `crates/openfang-types/src/doc.rs` — doc types (from task 37)
- `crates/openfang-memory/src/artifact.rs` — artifact store (from task 37)
- `crates/openfang-memory/src/doc.rs` — doc store (from task 37)
- `tasks/prd-compozy/docs/API-SPEC.md` — section 2 (conventions), section 12 (tasks/artifacts)
- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — artifact/doc table definitions
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`

### Dependent Files

- `crates/openfang-kernel/src/kernel.rs` — kernel providing database pool access
- `crates/openfang-api/src/state.rs` or `server.rs` — `AppState` with compozy.db pool

## Deliverables

- `GET /api/v1/artifacts` endpoint with pagination and filters
- `GET /api/v1/artifacts/{id}` endpoint with current version inline
- `GET /api/v1/artifacts/{id}/versions` endpoint with paginated version history
- `GET /api/v1/docs` endpoint with pagination and filters
- `GET /api/v1/docs/{id}` endpoint with current version inline
- `GET /api/v1/docs/{id}/versions` endpoint with paginated version history
- Response types for all six endpoints
- Tests for all operations

## Tests

### Unit Tests (Required)

- [x] `ArtifactSummary` serialization produces the expected JSON shape with `id`, `artifact_type`,
      `task_id`, `current_version_id`, `created_at`, `updated_at`.
- [x] `ArtifactDetail` serialization includes the `current_version` object inline with
      `version_number` and `content_hash`.
- [x] `DocSummary` and `DocDetail` serialization mirrors the artifact shapes with `doc_type`
      instead of `artifact_type`.
- [x] Filter parsing: `artifact_type=prd` produces the correct filter predicate;
      `task_id=task_001` produces the correct filter predicate.

### Integration Tests (Required)

- [x] `GET /api/v1/artifacts` returns `{ items, next_cursor }` with status 200. Items match
      inserted test data.
- [x] `GET /api/v1/artifacts/{id}` returns status 200 with the artifact detail and current version
      inline.
- [x] `GET /api/v1/artifacts/{id}` with unknown ID returns status 404 with the standard error
      envelope.
- [x] `GET /api/v1/artifacts/{id}/versions` returns paginated version list ordered by
      `version_number` descending.
- [x] `GET /api/v1/artifacts?artifact_type=prd` returns only artifacts of type `prd`.
- [x] `GET /api/v1/artifacts?task_id=task_001` returns only artifacts linked to `task_001`.
- [x] `GET /api/v1/docs` returns `{ items, next_cursor }` with status 200.
- [x] `GET /api/v1/docs/{id}` returns status 200 with the doc detail and current version inline.
- [x] `GET /api/v1/docs/{id}` with unknown ID returns status 404.
- [x] `GET /api/v1/docs/{id}/versions` returns paginated version list.
- [x] `GET /api/v1/docs?doc_type=brief` filters correctly by doc type.

### Regression and Anti-Pattern Guards

- [x] No POST, PUT, or DELETE routes exist under `/api/v1/artifacts` or `/api/v1/docs`. Attempting
      them returns 405 Method Not Allowed.
- [x] Version lists are ordered by `version_number` descending, not by insertion order.
- [x] The detail endpoint joins the current version in a single query (or two bounded queries),
      not via N+1 loading.
- [x] Empty version history for a newly created artifact returns `{ items: [], next_cursor: null }`,
      not an error.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- All six endpoints are registered in the axum router and return correct status codes and payload
  shapes for both happy-path and error cases.
- List endpoints return `{ items, next_cursor }` with pagination and filter support.
- Detail endpoints return the resource with the current version inlined.
- Version history endpoints return paginated, descending-ordered version lists.
- 404 responses use the standard error envelope.
- All queries read from `compozy.db` artifact/doc tables; no filesystem access for these endpoints.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- Task-scoped artifact and doc listings (`GET /api/v1/tasks/{id}/artifacts` and
  `GET /api/v1/tasks/{id}/docs`) are handled by task 32. This task adds the standalone,
  non-task-scoped endpoints.
- The `created_by` field on version objects uses the same `{ kind, ref }` shape as task/subtask
  `source` and `owner` fields for consistency.
