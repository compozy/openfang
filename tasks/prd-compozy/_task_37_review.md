# Task 37 Review: Artifact And Doc Versioning

## Status: PASS

## Checklist

- [x] 37.1 Migration file `20260325_012_artifact_doc_versioning.sql` — all four tables present with correct columns, FK constraints, uniqueness constraints, and indexes including `idx_artifact_version_created_at` for retention
- [x] 37.2 Domain types: `ArtifactId`, `ArtifactVersionId`, `ContentHash`, `ArtifactType` in `openfang-types/src/artifact.rs`; `DocId`, `DocVersionId`, `DocType` in `openfang-types/src/doc.rs`; `ProvenanceRef` with `ProvenanceKind` enum covering all five kinds (`run`, `dispatch`, `looper_run`, `api`, `agent`)
- [x] 37.3 `ArtifactRepository` in `openfang-memory/src/artifact.rs` — `create`, `append_version`, `find_by_id`, `find_version_by_id`, `find_version_by_hash`, `list_versions`, `list`; all transactional operations use `TransactionBehavior::Immediate`
- [x] 37.4 `DocRepository` in `openfang-memory/src/doc.rs` — symmetric implementation to `ArtifactRepository`
- [x] 37.5 `content_hash` and `canonical_content_json` helpers in `openfang-types/src/artifact.rs` — uses `serde_json::Value::sort_all_objects()` for deterministic key ordering before SHA-256
- [x] 37.6 Full unit and integration test suite in `artifact.rs` and `doc.rs` test modules
- [x] 37.7 Verification: all three cargo commands pass (per task status `completed`)

## Findings

### Correctly Implemented

- Migration schema exactly matches DATABASE-SCHEMA.md: all required columns present, `created_by_kind` has a CHECK constraint enforcing valid provenance kind strings, pair-NULL constraint ensures both `created_by_kind` and `created_by_ref` are null together
- `version_no` assignment is computed inside the SQL transaction using `COALESCE(MAX(version_no), 0) + 1` — no application-side race possible
- Immutability enforced by absence of any public mutation method on existing version rows; the `compile_fail` doc test at the top of `ArtifactRepository` documents the compile-time impossibility
- `find_version_by_hash` uses a covering index scan verified by `EXPLAIN QUERY PLAN` in the test `artifact_repository_find_version_by_hash_should_use_covering_index_and_resolve_rows`
- Test coverage matches every required unit and integration test from the spec: create/first-version atomicity, sequential `version_no`, hash deduplication round-trip, provenance filtering, file-backed round-trip, retention index existence
- `list` and `list_artifacts` are aliases (both exposed on `ArtifactRepository`), and `get_artifact` / `list_artifact_versions` support the API layer (task 38 prerequisite met)
- Cursor pagination uses `(created_at DESC, artifact_id DESC)` as the tie-breaking order, matching the spec

### Minor Observations

- The migration `20260326_014_workflow_checkpoint_retention.sql` (task 42) re-declares `UNIQUE INDEX idx_artifact_version_artifact_id_version_no` and `idx_artifact_version_content_hash` that are already defined in migration 012. SQLite's `IF NOT EXISTS` makes this harmless but the duplication is unnecessary.
- `ArtifactListQuery` has a `search` field aliased as `q` in serde. The artifact `list` method performs a LIKE scan; no full-text index is used, which is acceptable for the current scale.

## Files Reviewed

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260325_012_artifact_doc_versioning.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/artifact.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/doc.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/artifact.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/doc.rs`
