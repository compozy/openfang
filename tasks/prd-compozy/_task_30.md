## markdown

## status: pending

<task_context>
<domain>domain/artifacts/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task26,task28</dependencies>
</task_context>

# Task 30.0: Artifact And Doc Versioning

## Overview

Implement the artifact and document versioning schema and repositories in
`compozy.db`. Artifacts and docs are versioned resources linked to tasks,
subtasks, and looper runs. Each version is immutable; new versions are appended.
The schema supports provenance tracking (which run/step/dispatch produced the
artifact) and content-addressable references.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Artifact and doc tables with version history.
- Repository layer for CRUD and version queries.
- Provenance linking to runs, steps, and dispatches.
</requirements>

## Subtasks

- [ ] 30.1 Define artifact and artifact_version tables in compozy.db migration.
- [ ] 30.2 Define doc and doc_version tables in compozy.db migration.
- [ ] 30.3 Implement repositories and tests for versioned CRUD, provenance queries, and content-addressable lookup.

## Implementation Details

Artifacts and docs follow the domain model described in DESIGN.md section 8.
Each artifact has a stable identity (`artifact_id`) and an append-only version
history. Versions are immutable once written. The `current_version_id` field on
the artifact record points to the latest version.

Provenance fields on each version record link back to the run, step, and
dispatch that produced it. Content-addressable references use a hash of the
version content for deduplication and integrity checks.

Doc versioning follows the same pattern as artifact versioning, with a separate
table family (`doc`, `doc_version`) and the same provenance and content-
addressing model.

### Relevant Files

- `crates/openfang-memory/src/migration.rs`
- `crates/openfang-kernel/src/artifact.rs` (new)
- `tasks/prd-compozy/reset-2026-03-21/DATABASE-SCHEMA.md`

### Dependent Files

- `crates/openfang-types/src/lib.rs`

## Deliverables

- Artifact and doc schema with version tables
- Repository layer for versioned CRUD
- Tests for version history, provenance, and content addressing

## Tests

### Unit Tests (Required)

- [ ] Artifact creation produces a first version with correct provenance.
- [ ] Appending a new version updates `current_version_id` without mutating previous versions.
- [ ] Content-addressable lookup returns the correct version by hash.

### Integration Tests (Required)

- [ ] Artifact versioning round-trips through the repository layer correctly.
- [ ] Doc versioning round-trips through the repository layer correctly.
- [ ] Provenance queries correctly resolve which run/step/dispatch produced a given version.

### Regression and Anti-Pattern Guards

- [ ] Do not allow mutation of existing version records.
- [ ] Do not store version content inline in the artifact table; use the version table.
- [ ] Do not break the schema contract defined in DATABASE-SCHEMA.md.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Artifact and doc versioning schema is in place with migration.
- Repository layer supports versioned CRUD, provenance, and content-addressable lookup.
- All version records are immutable once written.

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
