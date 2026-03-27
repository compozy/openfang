# Task 38 Review: Artifact And Doc Standalone Read Endpoints

## Status: PASS

## Checklist

- [x] 38.1 Response types `ArtifactSummary`, `ArtifactDetail`, `ArtifactVersionSummary` defined in `openfang-types/src/artifact.rs`; `DocSummary`, `DocDetail`, `DocVersionSummary` defined in `openfang-types/src/doc.rs` — all derive `Serialize` and `Deserialize`
- [x] 38.2 `list_artifacts`, `get_artifact`, `list_artifact_versions` added to `ArtifactRepository`; equivalent methods added to `DocRepository` in `openfang-memory/src/doc.rs`
- [x] 38.3 `/api/v1/artifacts` router group registered in `server.rs` with three routes: `GET /` (list), `GET /:id` (detail), `GET /:id/versions`
- [x] 38.4 `/api/v1/docs` router group registered in `server.rs` with the same three routes
- [x] 38.5 `list_artifacts_v1`, `get_artifact_v1`, `list_artifact_versions_v1` handlers implemented in `routes.rs`
- [x] 38.6 `list_docs_v1`, `get_doc_v1`, `list_doc_versions_v1` handlers implemented in `routes.rs`
- [x] 38.7 Full integration test suite in `openfang-api/tests/artifact_doc_v1_api_test.rs`

## Findings

### Correctly Implemented

- All six endpoints wired in `server.rs` — confirmed at lines 620-635
- `GET /api/v1/artifacts/{id}` returns the current version inline via a single LEFT JOIN query — no N+1
- `GET /api/v1/artifacts/{id}/versions` returns versions descending by `version_number`, with cursor pagination
- 404 responses use the standard error envelope with `error.code = "not_found"` (verified in test `artifact_detail_should_inline_current_version_and_unknown_should_404`)
- Read-only guard tested: POST/PUT/DELETE all return 405 on artifact and doc routes (`artifact_and_doc_routes_should_be_read_only`)
- `artifact_type` and `task_id` filter parameters work — verified by integration tests
- `q` (search) filter tested against task ID substring
- Empty version page returns `{ items: [], next_cursor: null }` — verified in test `artifact_versions_should_paginate_descend_and_allow_empty_page`
- `task_id` on `ArtifactDetail` / `DocDetail` is resolved via a correlated subquery against the `task` table `artifact_refs_json` — indirect but works correctly

### Minor Observations

- Task 38 status is marked `done` in the file header, not `completed` like other tasks. The implementation is complete; this is just a status label inconsistency.
- The detail endpoint's `task_id` resolution uses `MIN(t.task_id)` which returns an arbitrary task when multiple tasks share the same artifact — acceptable per the spec which says "linked durable task identifier when one is known."

## Files Reviewed

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs` (lines 620–635)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (lines 2330–2550)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/artifact.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/doc.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/artifact.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/doc.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/artifact_doc_v1_api_test.rs`
