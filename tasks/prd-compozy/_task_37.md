## markdown

## status: pending

<task_context>
<domain>domain/artifacts/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task32,task34</dependencies>
</task_context>

# Task 37.0: Artifact And Doc Versioning

## Overview

Implement the artifact and document versioning schema and repositories in
`compozy.db`. Artifacts and docs are versioned, content-addressable resources
linked to tasks, subtasks, and looper runs. Each version is immutable once
written — new content always produces a new version record, never mutates an
existing one. The `artifact` and `doc` tables hold stable identity and a
pointer to the latest version (`current_version_id`). The `artifact_version`
and `doc_version` tables are append-only. Provenance fields on each version
record link back to the run, step, or dispatch that produced it, supporting
full lineage tracing from artifact back to execution.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets ---D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add an `artifact` table and an `artifact_version` table to `compozy.db`
  with the full column sets from DATABASE-SCHEMA.md section 3. `artifact`:
  `artifact_id`, `type`, `current_version_id`, `metadata_json`, `created_at`,
  `updated_at`. `artifact_version`: `artifact_version_id`, `artifact_id` (FK
  to `artifact`), `version_no` (monotonically increasing integer per
  artifact), `content_json`, `content_hash` (SHA-256 hex of canonical
  `content_json` bytes for deduplication and integrity), `created_by_kind`,
  `created_by_ref`, `created_at`. The `content_hash` column enables
  content-addressable lookup: given a hash, return the matching version.
- Add a `doc` table and a `doc_version` table to `compozy.db` following the
  same pattern as `artifact`/`artifact_version`. `doc` columns: `doc_id`,
  `type`, `current_version_id`, `metadata_json`, `created_at`, `updated_at`.
  `doc_version` columns: `doc_version_id`, `doc_id` (FK to `doc`),
  `version_no`, `content_json`, `content_hash`, `created_by_kind`,
  `created_by_ref`, `created_at`.
- `version_no` must be assigned as `MAX(version_no) + 1` for the parent
  artifact or doc within a single transaction. It must start at 1 for the
  first version. The assignment and the version insert must be atomic.
- Implement an `ArtifactRepository` covering: `create` (creates the artifact
  record and first version in one transaction), `append_version` (adds a new
  immutable version, updates `current_version_id` and `artifact.updated_at`),
  `find_by_id`, `find_version_by_id`, `find_version_by_hash` (content-
  addressable lookup), `list_versions` (all versions for an artifact in
  ascending `version_no` order), `list` (with pagination and `type` filter).
- Implement a `DocRepository` with the same operations as `ArtifactRepository`
  applied to the `doc`/`doc_version` table family.
- Immutability invariant: the `ArtifactRepository` and `DocRepository` must
  never expose a method that mutates an existing version record. Any attempt
  to update `content_json` of an existing `artifact_version` or `doc_version`
  must be a compile-time impossibility (no such method exists) or a hard
  domain error. The only permitted mutation on version records is the internal
  `version_no` assignment during `append_version`.
- Provenance fields `created_by_kind` and `created_by_ref` on version records
  encode which runtime object produced the version. The `kind` is a string
  enum: `"run"`, `"dispatch"`, `"looper_run"`, `"api"`, `"agent"`. The `ref`
  is the corresponding ID. Both are nullable — direct API writes may have no
  runtime provenance. The repository must accept `Option<ProvenanceRef>` and
  store `NULL` when absent.
</requirements>

## Subtasks

- [ ] 37.1 Write `compozy.db` migrations for `artifact`, `artifact_version`,
      `doc`, and `doc_version` tables. Include all approved columns, FK constraints,
      uniqueness on `(artifact_id, version_no)` and `(doc_id, version_no)`,
      indexes on `artifact_version.artifact_id`, `artifact_version.content_hash`,
      `doc_version.doc_id`, `doc_version.content_hash`, and `artifact.type` /
      `doc.type` for list filtering. Migration files go in `migrations/compozy/`
      and must continue the existing numbering sequence.
- [ ] 37.2 Define domain types for artifacts and docs: `ArtifactId` newtype,
      `ArtifactVersionId` newtype, `DocId` newtype, `DocVersionId` newtype,
      `ArtifactType` enum (or open string type), `DocType` enum, `ContentHash`
      newtype wrapping `String` (hex SHA-256), and `ProvenanceRef` struct with
      `kind: ProvenanceKind` and `ref_id: String`. These types go in
      `crates/openfang-types/` or the domain crate introduced in task 28. All
      types derive `serde::Serialize` and `serde::Deserialize`.
- [ ] 37.3 Implement `ArtifactRepository` with `create`, `append_version`,
      `find_by_id`, `find_version_by_id`, `find_version_by_hash`, `list_versions`,
      and `list`. The `create` and `append_version` operations must use a single
      SQLite transaction each. `find_version_by_hash` must use a covering index
      scan, not a full table scan.
- [ ] 37.4 Implement `DocRepository` with the same operation set as
      `ArtifactRepository` applied to the `doc`/`doc_version` family.
- [ ] 37.5 Implement the `content_hash` computation helper: canonicalize the
      `content_json` value (sort object keys deterministically, strip insignificant
      whitespace), compute SHA-256, and return the lowercase hex string. This must
      be a pure function with a unit test — the same logical content must always
      produce the same hash regardless of JSON field ordering in the input.
- [ ] 37.6 Write unit and integration tests as detailed in the Tests section.
- [ ] 37.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass with zero warnings before marking done.

## Implementation Details

The artifact and doc versioning model follows DESIGN.md section 8 and
DATABASE-SCHEMA.md section 3. The core invariant is: version records are
write-once. The `artifact` and `doc` tables are the mutable head that always
point at the latest version via `current_version_id`. The version tables are
append-only ledgers.

The `version_no` assignment must be strictly monotonic and collision-free
under concurrent writes. The safest implementation in SQLite is a single
transaction that does:

```sql
INSERT INTO artifact_version (artifact_id, version_no, ...)
SELECT ?1, COALESCE(MAX(version_no), 0) + 1, ...
FROM artifact_version WHERE artifact_id = ?1;
```

followed by updating `artifact.current_version_id` in the same transaction.
Do not attempt optimistic concurrency with application-side `MAX` queries;
always compute `version_no` inside the transaction.

The `content_hash` field enables two use cases:

1. Content-addressable lookup: given a hash, find the version. This supports
   deduplication — before creating a new version, a caller may check whether
   the same content already exists.
2. Integrity verification: after loading a version, a consumer may verify the
   hash matches the content.

The canonical hash computation must sort JSON object keys before hashing to
ensure two JSON objects with the same keys and values but different field
orderings produce the same hash. Use `serde_json::to_string` with a
deterministic serializer, or sort keys explicitly. Document the canonical
form in a `//!` module comment.

The `created_by_kind` / `created_by_ref` provenance pair links version records
to runtime execution objects. For example, when a workflow step produces a PRD
artifact, the version record should have `created_by_kind = "dispatch"` and
`created_by_ref = "dispatch_456"`. This is how
`GET /api/v1/tasks/{id}/artifacts` can show lineage when task 38's API layer
uses these repositories. Provenance is nullable because API-direct writes have
no associated runtime object.

The connection pattern follows `crates/openfang-memory/src/structured.rs`:
accept `Arc<Mutex<rusqlite::Connection>>` in the constructor. Do not open a
new connection per call.

The `list` endpoint for artifacts in API-SPEC.md uses cursor pagination with
`type` filtering. The `list` method on the repository must support a `type`
filter and return results in `created_at` descending order with cursor-based
pagination (using `artifact_id` as the cursor anchor when `created_at` values
collide).

### Relevant Files

- `crates/openfang-memory/src/migration.rs` — migration runner pattern
- `crates/openfang-memory/src/structured.rs` — repository/store pattern
- `crates/openfang-types/src/agent.rs` — newtype patterns
- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — `artifact`, `artifact_version`, `doc`, `doc_version` columns
- `tasks/prd-compozy/docs/API-SPEC.md` section 12 — linked context shapes for task artifacts/docs
- `tasks/prd-compozy/docs/DESIGN.md` section 8 — domain primitives rationale
- `migrations/compozy/` — migration sequence to extend

### Dependent Files

- `crates/openfang-types/src/` or domain crate from task 28 — types land here
- Task 38 control-plane API — will expose artifact/doc version queries
- Task 43 E2E test — verifies artifact version provenance in a full run

## Deliverables

- `migrations/compozy/XXXX_artifact_doc_versioning.sql` (or equivalent numbered files)
- `ArtifactId`, `ArtifactVersionId`, `DocId`, `DocVersionId`, `ContentHash`, `ProvenanceRef` domain types
- `ArtifactRepository` with full version lifecycle
- `DocRepository` with full version lifecycle
- `content_hash` canonical computation helper
- Full test suite as described below

## Tests

### Unit Tests (Required)

- [ ] `ArtifactRepository::create` creates the artifact record and a first
      `artifact_version` with `version_no = 1` in a single transaction; the
      artifact's `current_version_id` points at the new version ID.
- [ ] `ArtifactRepository::append_version` on an artifact that already has two
      versions assigns `version_no = 3` and updates `current_version_id`; the
      previous two versions remain in the table unchanged and their `content_json`
      values are unmodified.
- [ ] `ArtifactRepository::find_version_by_hash` returns the correct version
      when queried by the SHA-256 hash of its canonical content; it returns `None`
      for a hash that does not exist.
- [ ] The `content_hash` helper produces identical output for two JSON values
      that have the same keys and values but different field ordering in the
      serialized string.
- [ ] Attempting to call any method that would mutate an existing `artifact_version`
      row (e.g., update `content_json` directly) is either absent from the public
      repository API or returns a domain-level immutability error.
- [ ] `DocRepository::append_version` with `created_by_kind = "dispatch"` and
      `created_by_ref = "dispatch_456"` persists both provenance fields and returns
      them unchanged on `find_version_by_id`.
- [ ] `DocRepository::create` with `provenance = None` stores `NULL` in
      `created_by_kind` and `created_by_ref`; reading the version back returns
      `provenance = None` without error.

### Integration Tests (Required)

- [ ] `ArtifactRepository` round-trips through a file-backed temp database:
      create, append three versions, close the connection, reopen, and verify all
      four version records (`version_no` 1–4 with the implicit first) are present
      and `content_json` is byte-identical to the original writes.
- [ ] `DocRepository` round-trips through the same pattern; verify `content_hash`
      values are preserved exactly across the connection close/reopen cycle.
- [ ] Provenance queries: given an artifact with three versions where versions
      2 and 3 have `created_by_kind = "dispatch"` and different `created_by_ref`
      values, a query filtering by `created_by_ref = "dispatch_456"` returns only
      the correct version.
- [ ] Concurrent `append_version` calls on the same artifact (simulated with
      two sequential calls in the same test process) produce strictly sequential
      `version_no` values with no duplicates.
- [ ] Content-addressable deduplication round-trip: append a version, compute
      the hash externally using the same canonical algorithm, and verify
      `find_version_by_hash` retrieves the correct version record.
- [ ] Retention policy guard (anticipating task 42): confirm that the
      `artifact_version` table has an index on `created_at` that is usable by a
      range-delete query; verify the index exists after migration.

### Regression and Anti-Pattern Guards

- [ ] Do not allow mutation of existing version records through any code path
      reachable from the public repository API.
- [ ] Do not store version content inline in the `artifact` or `doc` table;
      `content_json` must live in `artifact_version` and `doc_version` only.
- [ ] Do not compute `version_no` in application code before the SQL insert; it
      must be computed inside the transaction to prevent races.
- [ ] Do not break the schema contract defined in DATABASE-SCHEMA.md; no column
      may be omitted or renamed from the approved set.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `artifact`, `artifact_version`, `doc`, and `doc_version` tables are in
  `compozy.db` with every column from DATABASE-SCHEMA.md, plus `content_hash`
  as an indexed provenance-and-deduplication field.
- `ArtifactRepository` and `DocRepository` support full version lifecycle:
  create, append, find by ID, find by hash, list versions, and list with type
  filter and cursor pagination.
- Version records are provably immutable: no public repository method can
  mutate `content_json` on an existing version row.
- The `content_hash` canonical computation is deterministic: same logical
  content always produces the same hash regardless of input field ordering.
- Provenance fields are correctly stored and retrieved for both runtime-
  produced and direct-API versions.
- `cargo fmt --all`, `cargo clippy`, and `cargo test --workspace` all pass at
  zero warnings and zero failures.

---

## Prior Implementation Reference

The old TypeScript codebase has the prior artifact model:

- `~/Dev/compozy/compozy-code/packages/backend/src/modules/artifacts/` — Artifact model, repository, and routes
- `~/Dev/compozy/compozy-code/packages/backend/src/db/schema/` — Database schema including artifact tables
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/artifacts/` — Frontend artifact management

The new design adds immutable version history and content-addressing on top of the old model.
The old code shows naming conventions, the basic artifact lifecycle, and how artifacts were linked
to tasks and execution runs.

## Notes

- This task was extracted from old task 24 to keep artifact/doc versioning focused and independently testable.
