## markdown

## status: completed

<task_context>
<domain>engine/dispatch/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task9,task19</dependencies>
</task_context>

# Task 23.0: agent_dispatch Schema And Persistence Layer

## Overview

Add the durable schema and persistence layer for `agent_dispatch` in `compozy.db`. This table is
the foundational record that makes agent delegation observable, restartable, and traceable across
workflow runs. Every invocation of `agent.call`, `agent.send`, or `agent.spawn` inside a durable
workflow run must produce a row in this table before execution begins, as mandated by ADR-009
(Persisted Agent Delegation and Lineage).

The schema and repository created here are the storage substrate that tasks 29 and 33 build on.
No runtime wiring happens in this task — only the durable storage layer.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add the `agent_dispatch` table to `compozy.db` via a versioned SQLite migration following the
  same pattern already established in `crates/openfang-memory/src/migration.rs` — sequential
  version-gated functions, idempotent `CREATE TABLE IF NOT EXISTS`, and a migration-tracking row.
- The schema must include all columns described in `DATABASE-SCHEMA.md` section 3: `dispatch_id`,
  `run_id`, `step_id`, `kind`, `target_agent`, `status`, `input_json`, `result_json`, `error_json`,
  `attempt`, `parent_dispatch_id`, `spawned_agent_id`, `started_at`, `updated_at`, `completed_at`.
  Additional columns needed for provider/session resume (task 29) should be reserved as nullable
  columns now: `provider_driver`, `session_id`, `provider_resume_token`.
- Implement all three dispatch modes — `call`, `send`, and `spawn` — as distinct `kind` values
  with documented semantics: `call` blocks and returns a result; `send` fires and does not wait;
  `spawn` creates a long-lived agent with a stable `spawned_agent_id`.
- Implement the full status lifecycle as a typed Rust enum: `pending`, `running`, `waiting_hitl`,
  `completed`, `failed`, `cancelled`. Status transitions must be validated — only legal transitions
  (e.g., `running -> waiting_hitl`, `running -> completed`) are accepted by the repository.
- Implement a dispatch repository with operations for: `create`, `find_by_id`, `find_by_run`,
  `find_children`, `update_status`, `update_result`, `update_error`, `mark_completed`, and
  `mark_failed`. All operations must use `thiserror`-based errors and return `Result<T, E>`.
- Parent-child dispatch lineage must be preserved: the `parent_dispatch_id` field enables tree
  reconstruction and `find_children` must query by parent ID efficiently via an index.
- The `attempt` counter must increment correctly on retry and be queryable so the control plane
  can surface it. Orphaned dispatches (no parent run) must remain queryable and must not be
  silently deleted by background cleanup.
</requirements>

## Subtasks

- [x] 23.1 Add a new `compozy.db` migration function that creates the `agent_dispatch` table with
      all required columns, proper indexes (`idx_dispatch_run`, `idx_dispatch_parent`,
      `idx_dispatch_status`), and records itself in the migration log. Verify the migration is
      idempotent and upgrades cleanly from a pre-existing `compozy.db` with earlier schema versions.

- [x] 23.2 Define the `DispatchKind` enum (`Call`, `Send`, `Spawn`) and `DispatchStatus` enum
      (`Pending`, `Running`, `WaitingHitl`, `Completed`, `Failed`, `Cancelled`) in the appropriate
      types module. Implement `Display`, `FromStr`, and `serde` derives. Enforce legal status
      transitions in the repository layer, not in callers.

- [x] 23.3 Implement the `DispatchRecord` struct and the `DispatchRepository` trait in a new
      module (e.g., `crates/openfang-memory/src/dispatch.rs` or a new `compozy-db` crate following
      the project's crate-split conventions). The trait must be `Send + Sync` and async.

- [x] 23.4 Implement the SQLite-backed `DispatchRepository` using `rusqlite` in a pattern
      consistent with `crates/openfang-memory/src/structured.rs`. All JSON columns (`input_json`,
      `result_json`, `error_json`) must serialize/deserialize via `serde_json`. Use parameterized
      queries throughout; no string interpolation in SQL.

- [x] 23.5 Add indexes for common query patterns: `(run_id)` for run-scoped dispatch lists,
      `(parent_dispatch_id)` for lineage traversal, `(status)` for status-filtered queries. Verify
      index presence in migration tests.

- [x] 23.6 Write repository-layer unit tests using an in-memory SQLite connection. Cover create,
      find, status transitions, parent-child linkage, and attempt increment. Verify that illegal status
      transitions return an error rather than silently succeeding.

- [x] 23.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

The current OpenFang workflow engine (`crates/openfang-kernel/src/workflow.rs`) runs steps
in-memory with no durable dispatch record. When a step dispatches to an agent, execution happens
through the agent loop in `crates/openfang-runtime/src/agent_loop.rs` with no persistent identity
for that delegation. This makes restarts destructive — any in-flight dispatch is lost.

The `agent_dispatch` table fixes this by creating a durable identity for each delegation before
execution starts. The flow after this task is in place becomes:

1. Workflow step resolves to a dispatch operation (`call`, `send`, or `spawn`).
2. A `DispatchRecord` row is inserted with `status = pending` and the relevant input captured in
   `input_json`.
3. Runtime execution begins and immediately transitions the record to `status = running`.
4. On completion, the record is updated with `result_json` and `status = completed`.
5. On HITL pause (task 30), the record transitions to `status = waiting_hitl` and resumes later.
6. On failure, `error_json` is written and `status = failed`.

This task delivers only steps 1 and 2 in schema form — the repository operations for steps 3-6
are wired by tasks 29 and 30.

The `parent_dispatch_id` field enables multi-level delegation graphs. A spawned agent (kind =
`spawn`) may itself dispatch children; those children reference the spawned dispatch as their
parent. The `find_children` repository method must support this recursively for the API's
`GET /api/v1/dispatches/{id}/children` endpoint (implemented in task 33).

The `spawned_agent_id` field is only populated for `kind = spawn` records. It references a stable
agent identity (not a session) that persists beyond a single workflow run.

Nullable provider resume columns (`provider_driver`, `session_id`, `provider_resume_token`) are
reserved now so that task 29 can populate them without a schema migration. Their presence in the
schema is intentional and documented — they are not yet read by anything in this task.

### Relevant Files

- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — canonical column set for
  `agent_dispatch`
- `tasks/prd-compozy/docs/API-SPEC.md` section 10 — dispatch detail shape and
  endpoints that this schema must back
- `crates/openfang-memory/src/migration.rs` — migration versioning pattern to follow
- `crates/openfang-memory/src/structured.rs` — repository implementation pattern
- `crates/arky-session/src/store.rs` — async trait pattern to follow for the repository trait

### Dependent Files

- `crates/openfang-memory/src/migration.rs` — gains a new version step
- new dispatch module in the appropriate memory/db crate
- task 29: dispatch runtime integration reads and writes these records
- task 30: HITL pause/resume updates `status` to `waiting_hitl` and back
- task 33: API handlers expose these records through `/api/v1/dispatches`

## Deliverables

- `compozy.db` migration adding the `agent_dispatch` table with all required columns and indexes
- `DispatchKind` and `DispatchStatus` typed enums with serde and display support
- `DispatchRecord` struct and `DispatchRepository` async trait
- SQLite-backed repository implementation
- Legal status-transition enforcement in the repository layer
- Full unit test coverage for schema, CRUD operations, lineage queries, and transition guards

## Tests

### Unit Tests (Required)

- [x] `dispatch_record_should_persist_all_required_fields` — create a dispatch record and reload
      it; verify every column round-trips correctly including JSON payloads and nullable fields.
- [x] `dispatch_parent_child_linkage_should_persist_and_be_queryable` — insert a parent dispatch
      and two child dispatches referencing its ID; `find_children(parent_id)` must return both
      children and no others.
- [x] `dispatch_status_transitions_should_enforce_legality` — attempt all illegal transitions
      (e.g., `completed -> running`, `failed -> waiting_hitl`) and verify each returns an error;
      attempt all legal transitions and verify they succeed.
- [x] `dispatch_attempt_counter_should_increment_on_retry` — create a dispatch at attempt 1,
      increment to attempt 2, and verify the stored value is 2.
- [x] `dispatch_kind_spawn_should_store_spawned_agent_id` — create a `spawn` dispatch with a
      `spawned_agent_id` and verify it is stored and retrievable.
- [x] `dispatch_find_by_run_should_return_all_run_dispatches` — insert dispatches for two
      different run IDs and verify `find_by_run` returns only those belonging to the queried run.
- [x] `dispatch_of_kind_send_should_not_require_result` — insert a `send` dispatch that completes
      without a result payload and verify this is stored without error.

### Integration Tests (Required)

- [x] `compozy_db_migration_should_add_dispatch_table_cleanly` — open an in-memory `compozy.db`,
      run all migrations, and verify the `agent_dispatch` table and all required indexes exist.
- [x] `compozy_db_migration_should_be_idempotent` — run migrations twice on the same database
      and verify no error and no duplicate tables or indexes.
- [x] `dispatch_repository_should_survive_connection_restart` — write a dispatch record, drop and
      re-open the SQLite connection, and verify the record is still present and queryable.
- [x] `dispatch_repository_should_handle_concurrent_status_updates` — simulate two concurrent
      tasks attempting to transition the same dispatch status and verify the outcome is consistent
      and non-corrupting.

### Regression and Anti-Pattern Guards

- [x] Do not encode dispatch state only as workflow checkpoint payloads — the `agent_dispatch`
      table must exist as its own first-class table, not as JSON inside `workflow_checkpoint.data_json`.
- [x] Do not skip parent-child lineage fields for convenience — `parent_dispatch_id` must be
      nullable but never omitted from the schema.
- [x] Do not let dispatch lifecycle depend on runtime-only in-memory structures — every status
      transition must be reflected immediately in the database row.
- [x] Do not use `unwrap()` in repository code — all SQLite errors must propagate as typed errors.
- [x] Do not allow the `approval` subsystem (the old `ApprovalManager` in
      `crates/openfang-kernel/src/approval.rs`) to be confused with dispatch persistence — these are
      entirely different concepts and must not share code paths.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- The `agent_dispatch` table exists in `compozy.db` with all columns and indexes defined in
  `DATABASE-SCHEMA.md` section 3, plus the reserved provider resume columns.
- All three dispatch modes (`call`, `send`, `spawn`) are represented as typed enum variants with
  correct serde serialization.
- The full six-state status lifecycle is enforced at the repository boundary, not only in callers.
- Parent-child dispatch lineage is queryable in O(1) via index, not via full table scan.
- Task 29 can begin immediately without any additional schema work — the nullable provider columns
  are already present.
- The migration is idempotent: running it twice on the same database produces no error.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Notes

- This is the storage half of dispatch durability. Runtime wiring lands in task 29.
- The `provider_driver`, `session_id`, and `provider_resume_token` columns are added now as
  nullable so task 29 does not need a schema migration — do not populate them in this task.
- The `compozy.db` migration numbering must be coordinated with whoever implements tasks 14-17
  to avoid version collisions. Check the existing highest migration version before assigning the
  next version number.
- The old OpenFang `approval.rs` subsystem is a completely different concept — interactive
  per-tool approval gates — and must not be used or referenced for dispatch persistence.
