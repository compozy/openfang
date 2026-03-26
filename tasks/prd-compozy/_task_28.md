## markdown

## status: completed

<task_context>
<domain>domain/tasks/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task19</dependencies>
</task_context>

# Task 28.0: Task And Subtask Domain Schema And Repositories

## Overview

Create the `task` and `subtask` domain schema, migrations, and repositories
in `compozy.db`. This is the foundational product-domain work model: `task`
replaces the old Compozy `issue` concept and anchors durable work context;
`subtask` replaces the old nested task concept and carries local executable
state. The schema must exactly match the column set approved in
`DATABASE-SCHEMA.md` section 3 and the public shape frozen in
`API-SPEC.md` section 12.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add a `task` table to `compozy.db` with the full approved column set:
  `task_id`, `slug`, `source_run_id`, `title`, `description`, `status`,
  `priority`, `complexity`, `position`, `owner_kind`, `owner_ref`,
  `created_by_kind`, `created_by_ref`, `repository_refs_json`,
  `label_refs_json`, `artifact_refs_json`, `doc_refs_json`, `file_refs_json`,
  `metadata_json`, `created_at`, `updated_at`, `completed_at`. The `slug`
  column must be unique within the database. Status values are at minimum:
  `planned`, `in_progress`, `completed`, `cancelled`.
- Add a `subtask` table to `compozy.db` with the full approved column set:
  `subtask_id`, `task_id` (FK to `task`), `title`, `description`, `kind`,
  `status`, `complexity`, `position`, `assignee_kind`, `assignee_ref`,
  `depends_on_json`, `parallelizable`, `input_json`, `result_json`,
  `metadata_json`, `created_at`, `updated_at`, `completed_at`. The
  `depends_on_json` field holds an ordered array of `subtask_id` strings.
  The `parallelizable` field is a boolean column (SQLite integer 0/1).
- Implement a `TaskRepository` in `crates/openfang-kernel/src/task.rs` (or
  the appropriate new module) covering: `create`, `find_by_id`, `find_by_slug`,
  `list` (with cursor pagination and filters matching API-SPEC.md section 12),
  `update`, `delete`, and `complete`. Repository errors must use `thiserror`
  enums, not raw `rusqlite::Error` leaking across the module boundary.
- Implement a `SubtaskRepository` covering: `create`, `find_by_id`,
  `list_for_task` (with status, assignee, kind, ready/blocked filters from
  API-SPEC.md section 12), `update`, `delete`, and `complete`. The
  `list_for_task` query must resolve `ready` (all `depends_on` subtasks are
  `completed`) and `blocked` (any dependency is not `completed`) filters in
  SQL, not in application code.
- All ref columns (`repository_refs_json`, `artifact_refs_json`,
  `doc_refs_json`, `file_refs_json`, `label_refs_json`, `depends_on_json`,
  `input_json`, `result_json`, `metadata_json`) must store valid JSON. The
  repositories must validate the shape on write and return a typed domain
  error on malformed input, not a raw serialization panic.
- Migrations must follow the numbered per-database convention established in
  `INITIAL-RUNTIME-MIGRATIONS.md`. Task migrations belong in
  `migrations/compozy/` and must be idempotent at the runner level. They must
  not touch `runtime.db` tables.
- Per ADR-045, ADR-047, and DESIGN.md section 8: the `task` and `subtask`
  tables and repository types must be isolated from the old OpenFang task-queue
  concepts. No shared struct, table name, or storage key may collide with the
  legacy queue. Isolation is enforced by module boundary, not by renaming the
  public domain terms.
</requirements>

## Subtasks

- [x] 28.1 Write `compozy.db` migrations for `task` and `subtask` tables.
      Include: all approved columns from DATABASE-SCHEMA.md, `slug` uniqueness
      constraint on `task`, foreign-key constraint from `subtask.task_id` to
      `task.task_id` (enable `PRAGMA foreign_keys = ON` in the connection setup),
      and basic indexes on `task.status`, `task.priority`, `subtask.task_id`, and
      `subtask.status`. Migration numbers must continue the existing
      `migrations/compozy/` sequence.
- [x] 28.2 Define domain types for task and subtask in `crates/openfang-types/`
      or a new `crates/openfang-domain/` crate. Types must include: `TaskId`
      newtype, `SubtaskId` newtype, `TaskStatus` enum, `SubtaskStatus` enum,
      `SubtaskKind` enum, `Priority` enum, `Complexity` enum, `OwnerRef` struct
      (with `kind` and `ref` fields), `AssigneeRef` struct, and the ref-array
      types for artifacts, docs, files, repositories, and labels. All types must
      derive `serde::Serialize` and `serde::Deserialize` and use the snake_case
      naming convention from the project `.rustfmt.toml`.
- [x] 28.3 Implement `TaskRepository` with full CRUD, slug lookup, list with
      cursor pagination (using `position` as the stable sort key), and the
      `source_run_id` linkage that allows workflow runs to be traced back to tasks.
      Follow the `StructuredStore` pattern in
      `crates/openfang-memory/src/structured.rs` for shared-connection handling.
- [x] 28.4 Implement `SubtaskRepository` with full CRUD, `list_for_task` with
      the `ready` and `blocked` computed filters, and dependency validation on
      write (a subtask's `depends_on` entries must all reference subtasks that
      belong to the same parent task). Return a domain error, not a panic, when
      validation fails.
- [x] 28.5 Implement the `replan` operation as a transactional method on
      `TaskRepository` or a dedicated `TaskReplanner` type. It must apply
      `cancel_subtasks`, `create_subtasks`, and `update_subtasks` operations
      atomically within a single SQLite transaction, matching the replan request
      shape in API-SPEC.md section 12. On error the entire transaction rolls back.
- [x] 28.6 Write unit and integration tests as detailed in the Tests section.
      Tests must use `pretty_assertions::assert_eq` throughout (enforced by
      clippy). Use in-memory SQLite (`Connection::open_in_memory()`) for unit
      tests; use a temp-file database for integration tests that need restart
      semantics.
- [x] 28.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass with zero warnings before marking this
      task done.

## Implementation Details

The `task` domain model replaces the old Compozy `issue` concept entirely
(ADR-045). The old TypeScript schema called this entity an `issue`; in this
codebase it is `task` at every layer — table name, type name, repository name,
and public API path. There is no translation layer. See ADR-047 for why the
naming collision with the legacy OpenFang task queue is resolved by isolation,
not renaming.

The `task` table's ref columns (`artifact_refs_json`, `doc_refs_json`,
`file_refs_json`, `repository_refs_json`, `label_refs_json`) hold JSON arrays
of typed ref objects. For example, `artifact_refs_json` holds objects with
`artifact_id` and `type` fields. These are owned at the task level and are the
primary navigation point for linked context (see API-SPEC.md section 12:
`GET /api/v1/tasks/{id}/artifacts`, `…/docs`, `…/files`).

The `subtask.depends_on_json` field holds a JSON array of `subtask_id` strings.
The repository's `list_for_task` method must resolve `ready` (all deps
completed) and `blocked` (any dep incomplete) without loading all subtasks into
memory. A CTE or subquery approach in SQL is required.

The `source_run_id` column on `task` links a task to the `workflow_run` that
produced it, enabling lineage queries. This is nullable — tasks can be created
directly via the API without a source run.

The connection pattern follows `crates/openfang-memory/src/structured.rs`:
accept an `Arc<Mutex<rusqlite::Connection>>` in the constructor. Do not open a
new connection per repository call. Enable `PRAGMA journal_mode=WAL` and
`PRAGMA foreign_keys=ON` at connection setup.

The migration runner pattern follows
`crates/openfang-memory/src/migration.rs`: use `PRAGMA user_version` for
version tracking. Each migration is applied exactly once by checking the
current version against the migration threshold.

### Relevant Files

- `crates/openfang-memory/src/migration.rs` — migration runner pattern
- `crates/openfang-memory/src/structured.rs` — repository/store pattern
- `crates/openfang-memory/src/substrate.rs` — shared-connection initialization
- `crates/openfang-types/src/agent.rs` — newtype patterns to follow
- `crates/openfang-types/src/error.rs` — error type patterns
- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — approved columns
- `tasks/prd-compozy/docs/API-SPEC.md` section 12 — public shapes
- `tasks/prd-compozy/docs/adrs/045-task-subtask-domain-model.md`
- `tasks/prd-compozy/docs/adrs/047-keep-task-and-subtask-as-public-domain-names.md`
- `tasks/prd-compozy/docs/adrs/048-task-and-subtask-control-plane-surfaces.md`
- `tasks/prd-compozy/docs/adrs/006-reusable-compozy-domain-primitives.md`
- `migrations/compozy/` — migration sequence to extend (new files go here)

### Dependent Files

- task and subtask control-plane API handlers (task 32)
- looper runtime (task 34) — consumes `SubtaskRepository` for subtask selection
- artifact/doc versioning (task 37) — uses `artifact_refs_json` and `doc_refs_json`
- E2E integration test (task 43)

## Deliverables

- `migrations/compozy/XXXX_task_subtask.sql` (or equivalent numbered file)
- Domain types in `crates/openfang-types/src/` or new domain crate
- `TaskRepository` and `SubtaskRepository` implementations
- Transactional `replan` operation
- Full test suite as described below

## Tests

### Unit Tests (Required)

- [x] Creating a `task` with all required fields persists every column correctly;
      reading it back by `task_id` returns an identical record with no field loss.
- [x] Creating a `task` with a duplicate `slug` returns a domain-level error, not
      a raw rusqlite constraint violation.
- [x] Creating a `subtask` with a `depends_on` entry that references a subtask
      from a different parent task is rejected with a domain validation error.
- [x] `list_for_task` with `ready = true` returns only subtasks whose every
      `depends_on` entry has status `completed`; subtasks with any incomplete
      dependency are excluded.
- [x] `list_for_task` with `blocked = true` returns only subtasks that have at
      least one `depends_on` entry with a non-completed status.
- [x] `SubtaskRepository::update` on a subtask with `parallelizable = true`
      persists the boolean correctly and returns it unchanged on read.
- [x] `replan` with `cancel_subtasks` + `create_subtasks` + `update_subtasks`
      operations applies atomically: if `create_subtasks` fails mid-operation,
      neither the cancellations nor any partial creates persist.

### Integration Tests (Required)

- [x] A fresh `compozy.db` (file-backed temp path) bootstraps with the task and
      subtask migrations applied cleanly, and a subsequent migration run is
      idempotent (no error, no duplicate schema objects).
- [x] A task created before a simulated restart is queryable by `task_id` and by
      `slug` after the database connection is reopened, with all ref columns intact.
- [x] A subtask chain with `depends_on` ordering survives a connection close and
      reopen; the dependency array is preserved byte-for-byte.
- [x] `source_run_id` linkage: creating a task with a `source_run_id` value and
      then querying by that value returns the correct task set.
- [x] List pagination with `cursor` and `limit` returns the correct page of
      tasks sorted by `position`, and `next_cursor` is null on the last page.
- [x] Replan round-trip: a task with three subtasks can have one cancelled, one
      updated, and one new one created through a single replan call; the resulting
      subtask list matches the expected state exactly.

### Regression and Anti-Pattern Guards

- [x] Do not route canonical task/subtask state through the old OpenFang task
      queue or any legacy `openfang_tasks` storage path.
- [x] Do not reduce task context to runtime-only fields; all ref columns
      (`artifact_refs_json`, `doc_refs_json`, `file_refs_json`, etc.) must be
      persisted as first-class columns, not shoved into `metadata_json`.
- [x] Do not store subtask dependency structure as an opaque blob with no
      validation; the dependency invariant (same parent task) must be checked on
      write.
- [x] Do not use `unwrap()` in repository code; all fallible operations must
      propagate errors with `?` and typed error variants.
- [x] Do not let `rusqlite::Error` escape module boundaries; always convert to
      domain error types at the repository layer.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- `task` and `subtask` tables exist in `compozy.db` with every column from
  DATABASE-SCHEMA.md, including all ref-array columns and `depends_on_json`.
- `TaskRepository` and `SubtaskRepository` support full CRUD, pagination,
  filtered listing (ready/blocked), slug lookup, and source-run linkage.
- The transactional `replan` operation applies atomically with rollback on error.
- All domain types are isolated from the legacy OpenFang queue namespace.
- The migration is idempotent: applying it twice against the same database
  causes no error and no schema duplication.
- Later tasks (34, 37, 39, 43) can import and use these repositories directly
  without any further schema changes to the task/subtask tables.
- `cargo fmt --all`, `cargo clippy`, and `cargo test --workspace` all pass at
  zero warnings and zero failures.

---

## Prior Implementation Reference

The old TypeScript codebase has the prior task/subtask schema (called "issues" in the old model):

- `~/Dev/compozy/compozy-code/packages/backend/src/db/schema/tasks.ts` — Old `tasks` + `subtasks` table definitions with statuses, complexities, position ordering, and indexes
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/tasks/` — Task CRUD use cases and routes
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/subtasks/` — Subtask model and repository

Key differences from the old model:

- Old subtask has 3-value status; new model adds `depends_on`, `parallelizable`, `assignee`
- Old "issue" concept → renamed to "task" (see ADR-047)
- Old model uses PostgreSQL (Drizzle ORM); new model uses SQLite (`compozy.db`)

## Notes

- This task introduces the product-domain work model for real.
