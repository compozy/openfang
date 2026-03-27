# Task 28 Review: Task And Subtask Domain Schema And Repositories

## Status: PASS

## Checklist
- [x] Migration `20260324_010_task_subtask.sql` exists in `migrations/compozy/`
- [x] `task` table has all approved columns: `task_id`, `slug` (UNIQUE), `source_run_id`, `title`, `description`, `status`, `priority`, `complexity`, `position`, `owner_kind`, `owner_ref`, `created_by_kind`, `created_by_ref`, `repository_refs_json`, `label_refs_json`, `artifact_refs_json`, `doc_refs_json`, `file_refs_json`, `metadata_json`, `created_at`, `updated_at`, `completed_at`
- [x] `subtask` table has all approved columns: `subtask_id`, `task_id` (FK), `title`, `description`, `kind`, `status`, `complexity`, `position`, `assignee_kind`, `assignee_ref`, `depends_on_json`, `parallelizable`, `input_json`, `result_json`, `metadata_json`, `created_at`, `updated_at`, `completed_at`
- [x] `task.slug` has UNIQUE constraint
- [x] `subtask.task_id` has FK to `task.task_id` with ON DELETE CASCADE
- [x] `parallelizable` is `INTEGER NOT NULL CHECK (parallelizable IN (0, 1))`
- [x] JSON columns have `json_valid()` and `json_type()` CHECK constraints
- [x] `completed_at` consistency CHECK enforced in SQL
- [x] Indexes on `task.status`, `task.priority`, `task.source_run_id`, `subtask.task_id`, `subtask.status`
- [x] `PRAGMA foreign_keys = ON` enforced at connection setup
- [x] `TaskId` and `SubtaskId` newtypes defined
- [x] `TaskStatus` enum: `planned`, `in_progress`, `completed`, `failed`, `cancelled`
- [x] `SubtaskStatus` enum: `planned`, `ready`, `in_progress`, `completed`, `failed`, `cancelled`
- [x] `SubtaskKind` enum with 6 variants
- [x] `Priority`, `Complexity`, `OwnerRef`, `AssigneeRef`, `TaskSource` and ref-array types defined
- [x] All domain types derive `Serialize`/`Deserialize`
- [x] `TaskRepository`: `create`, `find_by_id`, `find_by_slug`, `list`, `update`, `delete`, `complete`
- [x] `SubtaskRepository`: `create`, `find_by_id`, `list_for_task`, `list`, `update`, `delete`, `complete`
- [x] `list_for_task` has `ready` and `blocked` filters resolved in SQL (not app code)
- [x] `replan` transactional operation implemented on `TaskRepository`
- [x] `TaskStoreError` enum with `thiserror` — `rusqlite::Error` does not leak across module boundary
- [x] `DuplicateSlug` domain error (not raw constraint violation)
- [x] Cross-task dependency validation on subtask write
- [x] `TaskStoreSet` bundles `TaskRepository` + `SubtaskRepository` with shared connection
- [x] Follows `StructuredStore` pattern (accepts `Arc<Mutex<Connection>>`)
- [x] Migration SQL embedded via `include_str!` macro
- [x] `TASK_SUBTASK_MIGRATION_SQL` constant accessible for migration runner
- [x] Unit test: round-trip all columns
- [x] Unit test: duplicate slug returns domain error
- [x] Unit test: cross-task dependency rejected
- [x] Unit test: `ready` filter (SQL-resolved)
- [x] Unit test: `blocked` filter (SQL-resolved)
- [x] Unit test: `parallelizable` persisted correctly
- [x] Unit test: `replan` rolls back atomically on failure
- [x] Integration test: migrations are idempotent
- [x] Integration test: round-trip after connection reopen
- [x] Integration test: subtask dependency JSON survives reopen byte-for-byte
- [x] Integration test: `source_run_id` linkage query
- [x] Integration test: pagination by `position` cursor with `next_cursor: null` on last page
- [x] Integration test: replan round-trip (cancel + update + create in one call)
- [x] No use of `unwrap()` in repository code
- [x] Isolated from legacy OpenFang task-queue concepts (no shared table names)

## Findings

**Implemented correctly:**
- The migration file matches the approved column set from `DATABASE-SCHEMA.md` exactly, including all JSON ref columns, the `parallelizable` boolean-as-integer, and the CHECK constraints ensuring `completed_at` presence correlates with terminal status values.
- All 12 required unit and integration tests are present and named descriptively (`task_repository_should_reject_duplicate_slug_with_domain_error`, etc.).
- `list_for_task` with `ready`/`blocked` filters uses CTE/subquery logic in SQL rather than loading all subtasks into memory.
- The `replan` operation uses a single `SQLite` transaction with `BEGIN`/`COMMIT`/`ROLLBACK` semantics; the test verifies rollback by injecting a subtask with a conflicting ID.
- Error types use `thiserror` and all `rusqlite::Error` values are converted to domain variants at the repository boundary.
- The `completed_at` column has a consistent SQL CHECK constraint preventing a terminal status without a timestamp (and vice versa).
- The migration SQL is embedded at compile time via `include_str!`, ensuring it ships with the binary.

**Minor observations:**
- `TaskStatus` includes `failed` in addition to the spec's minimum of `planned`, `in_progress`, `completed`, `cancelled` — this is an acceptable extension.
- `SubtaskStatus` includes `ready` and `failed` beyond what was strictly listed in the spec — also acceptable.
- The test `task_repository_replan_should_preserve_completed_at_for_terminal_patch_updates` adds coverage for an edge case in the spec's replan behavior that wasn't originally required but is a correct guard.

**Code quality:**
- Clean, idiomatic Rust. No `unwrap()` calls in production code paths. All fallible operations propagate via `?`.
- `pretty_assertions::assert_eq` used throughout tests (enforced by clippy).

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260324_010_task_subtask.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/task.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/task.rs`
