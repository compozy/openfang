## markdown

## status: pending

<task_context>
<domain>engine/workflows/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task2,task3</dependencies>
</task_context>

# Task 9.0: Initial compozy.db Workflow Core Schema

## Overview

Create the initial `compozy.db` schema for durable workflow execution:
`workflow_run`, `workflow_checkpoint`, and `workflow_signal`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Per ADR-050 and INITIAL-RUNTIME-MIGRATIONS.md section 1, Phase 1 covers exactly three workflow tables: `workflow_run`, `workflow_checkpoint`, and `workflow_signal`. No Phase 2 tables (`agent_dispatch`, `hitl_request`) and no Phase 3 tables (`task`, `subtask`) may be introduced in this task.
- Per ADR-050, the initial recovery semantics for `workflow_run` must be conservative: runs in `running` status are downgraded to `paused` on startup, not auto-resumed. This is enforced by the startup recovery scan, which is part of this task's scope.
- Per INITIAL-RUNTIME-MIGRATIONS.md section 5, `workflow_run` must include `waiting_kind` and `waiting_ref` columns to persist the signal-wait state so a `waiting_signal` run survives restart. These columns must not be omitted in the first migration cut.
- Per IMPLEMENTATION-PLAN.md section 6, the migration set must include at minimum: index on `workflow_run` by `workflow_id`; index on `workflow_run` by `status`; index on `workflow_run` by `updated_at`; index on `workflow_checkpoint` by `run_id` ordered by `created_at`; indexes on `workflow_signal` by `run_id`, by `run_id + consumed`, and by `run_id + name`. All indexes must be part of the migration SQL.
- Per INITIAL-RUNTIME-MIGRATIONS.md section 6, Phase 1 must support a minimum transition writer responsible for: mutating `workflow_run` and appending `workflow_checkpoint` in a single transaction where practical. A `WorkflowRunStore` adapter must expose this combined write.
- Per ADR-003, no cross-database SQL joins are permitted. `workflow_run`, `workflow_checkpoint`, and `workflow_signal` must not contain foreign keys referencing `runtime.db` tables. References to agents and sessions use stable string IDs resolved in application code.
- Per STORAGE-MODEL.md section 4, `compozy.db` is the authoritative source for durable workflow state. Workflow execution must not be considered started until a `workflow_run` row is created and a `run_created` checkpoint is written. In-memory-only workflow execution is not acceptable after this task.
- The `WorkflowRunStore`, `WorkflowCheckpointStore`, and `WorkflowSignalStore` adapter types must wrap the `compozy_db` handle from Task 2 and replace the raw `Arc<Mutex<Connection>>` field on `OpenFangKernel` with typed store access.
</requirements>

## Subtasks

- [ ] 9.1 Write the three `compozy.db` migration SQL files (or inline SQL constants) per INITIAL-RUNTIME-MIGRATIONS.md section 5: `0002_workflow_run_core`, `0003_workflow_checkpoint`, `0004_workflow_signal`. Register them as `MigrationStep` entries in the `compozy.db` migration slice passed to the Task 3 runner in `boot_with_config()`. Confirm the `schema_migration` bootstrap from Task 3 is already step one in the slice.
- [ ] 9.2 Define `WorkflowRunStore` wrapping `Arc<Mutex<Connection>>` for `compozy.db`. Implement: `create_run()`, `get_run()`, `list_runs()`, `update_run_status()`, `update_run_waiting_state()`. The `create_run()` and initial checkpoint write should be combined in a single transaction per INITIAL-RUNTIME-MIGRATIONS.md section 6 (Transition Writer).
- [ ] 9.3 Define `WorkflowCheckpointStore`. Implement: `append_checkpoint()`, `list_checkpoints_for_run()`. The checkpoint `kind` field must support at minimum the nine checkpoint kinds specified in INITIAL-RUNTIME-MIGRATIONS.md section 5: `run_created`, `run_started`, `step_selected`, `waiting_signal`, `signal_received`, `run_paused`, `run_resumed`, `run_completed`, `run_failed`.
- [ ] 9.4 Define `WorkflowSignalStore`. Implement: `insert_signal()`, `list_pending_signals_for_run()`, `mark_signal_consumed()`. A signal is pending when `consumed = 0`. Consuming a signal sets `consumed = 1` and `consumed_at = datetime('now')`.
- [ ] 9.5 Replace the raw `compozy_db: Arc<Mutex<Connection>>` handle on `OpenFangKernel` (introduced by Task 2) with a `WorkflowStores` composite type or individual named store fields. The workflow engine at `crates/openfang-kernel/src/workflow.rs` must receive typed store access, not a raw connection.
- [ ] 9.6 Implement the startup recovery scan: on boot, after migrations succeed, query `workflow_run WHERE status = 'running'` and downgrade each to `status = 'paused'`, writing a `run_recovered_needs_resume` checkpoint per INITIAL-RUNTIME-MIGRATIONS.md section 8. This scan runs once per boot before the workflow engine accepts new runs.
- [ ] 9.7 Write migration tests for schema creation and all required indexes. Write store adapter tests for `workflow_run` lifecycle, checkpoint ordering, and signal consumption. Write a recovery scan test confirming `running` runs are downgraded to `paused` on a simulated restart.
      </requirements>

## Implementation Details

This task creates the `compozy.db` schema foundation and the Phase 1 store
layer. The runtime write-path integration (wiring the existing workflow engine
to durably persist every transition) is the scope of the task that follows
this one in the implementation order.

### Current State

The workflow engine at `crates/openfang-kernel/src/workflow.rs` operates
entirely in memory. `WorkflowEngine`, `WorkflowId`, `WorkflowRunId`,
`StepAgent`, and `Workflow` types exist but write no durable state. Run state
is lost on restart.

The `compozy_db` handle on `OpenFangKernel` after Task 2 is a raw
`Arc<Mutex<rusqlite::Connection>>` pointing to a fresh, empty SQLite file with
only the `schema_migration` table (Task 3's bootstrap step). This task adds
the three workflow tables to that database and replaces the raw handle with
typed store adapters.

### Table Schemas (Minimum Fields)

Per DATABASE-SCHEMA.md section 3 and INITIAL-RUNTIME-MIGRATIONS.md section 5:

**`workflow_run`** (migration `0002`):

- `run_id TEXT PRIMARY KEY`
- `workflow_id TEXT NOT NULL`
- `workflow_version TEXT`
- `status TEXT NOT NULL` (values: `pending`, `running`, `waiting_signal`, `paused`, `completed`, `failed`, `cancelled`)
- `input_json TEXT NOT NULL DEFAULT '{}'`
- `vars_json TEXT NOT NULL DEFAULT '{}'`
- `current_step_id TEXT`
- `waiting_kind TEXT`
- `waiting_ref TEXT`
- `active_dispatch_id TEXT`
- `active_hitl_request_id TEXT`
- `labels_json TEXT NOT NULL DEFAULT '[]'`
- `metadata_json TEXT NOT NULL DEFAULT '{}'`
- `error_json TEXT`
- `started_at TEXT`
- `updated_at TEXT NOT NULL`
- `completed_at TEXT`

**`workflow_checkpoint`** (migration `0003`):

- `checkpoint_id TEXT PRIMARY KEY`
- `run_id TEXT NOT NULL`
- `step_id TEXT`
- `kind TEXT NOT NULL`
- `data_json TEXT NOT NULL DEFAULT '{}'`
- `created_at TEXT NOT NULL`

**`workflow_signal`** (migration `0004`):

- `signal_id TEXT PRIMARY KEY`
- `run_id TEXT NOT NULL`
- `name TEXT NOT NULL`
- `payload_json TEXT NOT NULL DEFAULT '{}'`
- `source TEXT`
- `consumed INTEGER NOT NULL DEFAULT 0`
- `created_at TEXT NOT NULL`
- `consumed_at TEXT`

### Required Indexes

Per IMPLEMENTATION-PLAN.md section 6:

For `workflow_run`:

- `CREATE INDEX idx_workflow_run_workflow_id ON workflow_run(workflow_id)`
- `CREATE INDEX idx_workflow_run_status ON workflow_run(status)`
- `CREATE INDEX idx_workflow_run_updated_at ON workflow_run(updated_at)`

For `workflow_checkpoint`:

- `CREATE INDEX idx_workflow_checkpoint_run ON workflow_checkpoint(run_id, created_at)`

For `workflow_signal`:

- `CREATE INDEX idx_workflow_signal_run ON workflow_signal(run_id)`
- `CREATE INDEX idx_workflow_signal_run_consumed ON workflow_signal(run_id, consumed)`
- `CREATE INDEX idx_workflow_signal_run_name ON workflow_signal(run_id, name)`

### Store Adapter Pattern

Follow the pattern from `crates/openfang-memory/src/structured.rs` and the
`AgentRuntimeStore` introduced in Task 6:

- Wrap `Arc<Mutex<rusqlite::Connection>>` for the `compozy.db` connection
- Methods are synchronous; `spawn_blocking` wrapping goes at the call site
- Each method maps `rusqlite::Error` to a typed `WorkflowStoreError` using `thiserror`
- Transactions are explicit: `create_run()` opens a transaction, inserts the
  `workflow_run` row, inserts the `run_created` checkpoint row, then commits

### Transition Writer Contract

Per INITIAL-RUNTIME-MIGRATIONS.md section 6, the transition writer is
responsible for:

1. `UPDATE workflow_run SET status = ..., current_step_id = ..., updated_at = ... WHERE run_id = ?`
2. `INSERT INTO workflow_checkpoint (checkpoint_id, run_id, step_id, kind, data_json, created_at) VALUES (...)`

Both writes happen in a single SQLite transaction. `WorkflowRunStore` must
expose a method with this signature (or equivalent):

```rust
pub fn transition(
    &self,
    run_id: &str,
    new_status: &str,
    step_id: Option<&str>,
    checkpoint_kind: &str,
    checkpoint_data: &serde_json::Value,
) -> Result<(), WorkflowStoreError>
```

### Recovery Scan Contract

Per INITIAL-RUNTIME-MIGRATIONS.md section 8, the startup recovery policy:

- `pending` — no change
- `waiting_signal` — no change (the signal will resume the run when delivered)
- `completed`, `failed`, `cancelled` — no change
- `running` — downgraded to `paused`; a `run_recovered_needs_resume` checkpoint is appended

The recovery scan runs once in `boot_with_config()` after `compozy.db`
migrations succeed, before the workflow engine starts accepting new runs.

### Checkpoint Kinds

Per INITIAL-RUNTIME-MIGRATIONS.md section 5, the minimum required checkpoint
kinds for Phase 1 are:

- `run_created` — written when the `workflow_run` row is first created
- `run_started` — written when the run transitions to `running`
- `step_selected` — written when the current step changes
- `waiting_signal` — written when the run enters `waiting_signal` status
- `signal_received` — written when a signal is consumed and the run resumes
- `run_paused` — written on manual pause or policy pause
- `run_resumed` — written when a paused run is explicitly resumed
- `run_completed` — written when the run reaches `completed`
- `run_failed` — written when the run reaches `failed`
- `run_recovered_needs_resume` — written by the startup recovery scan

These are string values in the `kind` column. A `CheckpointKind` enum or
newtype may be defined for compile-time safety, serializing to the strings above.

### Integration Points

- `crates/openfang-kernel/src/kernel.rs` — `boot_with_config()` registers the
  `compozy.db` migration slice (now four steps: bootstrap + three workflow tables),
  runs migrations, runs the recovery scan, and initializes the three workflow
  store handles.
- `crates/openfang-kernel/src/workflow.rs` — `WorkflowEngine` receives typed
  store handles. This task does not yet wire every engine transition to use the
  stores (that is the next task); it wires the store construction and makes the
  handles available on the engine.
- `crates/openfang-api/src/routes.rs` — workflow-related endpoints (run list,
  run detail) can begin using `WorkflowRunStore` for durable reads. This task
  enables the reads; full write-path wiring is the next task.
- `crates/openfang-kernel/src/error.rs` — no new error variants needed;
  `KernelError::BootFailed` covers migration and recovery scan failures.

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow.rs` — existing in-memory workflow engine; receives typed store handles from this task
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — `boot_with_config()` and `OpenFangKernel` struct; `compozy_db` field replaced by workflow store handles
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/error.rs` — `KernelError`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/structured.rs` — reference store adapter pattern
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` — workflow API endpoints
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — table and column spec (section 3)
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/INITIAL-RUNTIME-MIGRATIONS.md` — migration file spec (sections 5, 6, 7, 8)
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/STORAGE-MODEL.md` — ownership rules (section 4, 6)

### Dependent Files

- Future workflow write-path task that wires every engine state transition to `WorkflowRunStore::transition()`
- Future workflow API task that adds signal submission and run list endpoints using the store adapters introduced here
- `agent_dispatch` and `hitl_request` tables (Phase 2) will reference `run_id` from `workflow_run` by stable ID

## Deliverables

- Three `compozy.db` migration steps (SQL) covering `workflow_run`, `workflow_checkpoint`, `workflow_signal`
- `WorkflowRunStore`, `WorkflowCheckpointStore`, `WorkflowSignalStore` adapter types
- Startup recovery scan that downgrades `running` runs to `paused`
- `OpenFangKernel` updated with typed workflow store handles replacing the raw `compozy_db` connection
- Unit and integration tests for schema creation, store round-trips, and recovery scan

## Tests

### Unit Tests (Required)

- [ ] `compozy_db_migration_should_create_workflow_run_table()` — run the `compozy.db` migration slice against an in-memory connection; query `sqlite_master` and confirm `workflow_run` exists with the expected columns.
- [ ] `compozy_db_migration_should_create_workflow_checkpoint_table()` — same for `workflow_checkpoint`.
- [ ] `compozy_db_migration_should_create_workflow_signal_table()` — same for `workflow_signal`.
- [ ] `compozy_db_migration_should_create_all_required_indexes()` — query `sqlite_master WHERE type='index'` and confirm each required index name is present.
- [ ] `workflow_run_store_should_create_run_and_write_run_created_checkpoint()` — call `create_run()`, query `workflow_run` and `workflow_checkpoint`; confirm one run row and one `run_created` checkpoint row exist, inserted in the same transaction.
- [ ] `workflow_run_store_transition_should_write_run_and_checkpoint_atomically()` — call `transition()`, simulate a failure mid-transaction; confirm neither the status update nor the checkpoint row appears.
- [ ] `workflow_signal_store_should_insert_and_consume_signals()` — insert two signals for the same run, consume one, confirm `consumed = 1` and `consumed_at IS NOT NULL` for the consumed signal and `consumed = 0` for the other.
- [ ] `workflow_checkpoint_store_should_return_checkpoints_in_created_at_order()` — insert checkpoints out of chronological order by manipulating timestamps; confirm `list_checkpoints_for_run()` returns them ordered by `created_at` ascending.
- [ ] `recovery_scan_should_downgrade_running_to_paused_and_write_checkpoint()` — seed a `workflow_run` row with `status = 'running'`, run the recovery scan, confirm `status = 'paused'` and a `run_recovered_needs_resume` checkpoint row exists.
- [ ] `recovery_scan_should_not_modify_waiting_signal_runs()` — seed a run with `status = 'waiting_signal'`, run the recovery scan, confirm status is unchanged.

### Integration Tests (Required)

- [ ] `boot_should_create_compozy_db_with_all_phase_1_tables()` — after `boot_with_config()`, open `compozy.db` directly and confirm `workflow_run`, `workflow_checkpoint`, `workflow_signal` exist alongside `schema_migration`.
- [ ] `boot_should_run_recovery_scan_before_workflow_engine_starts()` — seed a `running` run in `compozy.db`, reboot, confirm the run is `paused` before any new runs are accepted.
- [ ] `workflow_run_should_survive_restart()` — create a `workflow_run` row, shut down (simulate by closing connections), reopen the database, confirm the row is present with the same `run_id` and fields.
- [ ] `waiting_signal_run_should_survive_restart_and_remain_resumable()` — create a run in `waiting_signal` status with `waiting_kind` and `waiting_ref` populated, reboot, confirm the run is still `waiting_signal` with the same waiting state fields.
- [ ] `compozy_db_and_runtime_db_should_coexist_after_both_boot()` — after full boot, confirm both `runtime.db` and `compozy.db` exist in `data_dir`, both have `schema_migration` tables, and their schemas do not overlap.

### Regression and Anti-Pattern Guards

- [ ] No Phase 2 tables (`agent_dispatch`, `hitl_request`) and no Phase 3 tables (`task`, `subtask`) may appear in the `compozy.db` migration slice for this task. Confirm by reviewing all SQL strings in the migration step definitions.
- [ ] The `workflow_run` table must not contain definition fields (workflow TOML content, step definitions). Per ADR-037, these remain file-backed. Confirm by reviewing the migration SQL columns — `workflow_id` and `workflow_version` are references, not definition copies.
- [ ] No migration step for `compozy.db` may reference a table from `runtime.db` via `ATTACH DATABASE` or any cross-database SQL syntax.
- [ ] The recovery scan must not auto-resume `running` runs. Confirm by asserting that after the recovery scan, no run has `status = 'running'` — they must all be `paused` or in a terminal state.
- [ ] Schema assumptions must not be hardcoded only in runtime code without corresponding migration SQL. If a store adapter references a column name, that column must appear in the migration SQL for the table.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `compozy.db` has the Phase 1 workflow-core schema with the three tables and all required indexes after a fresh boot.
- `WorkflowRunStore`, `WorkflowCheckpointStore`, and `WorkflowSignalStore` adapters provide typed read/write access to the new tables.
- The combined transition write (run status + checkpoint) is atomic via a single SQLite transaction.
- The startup recovery scan downgrades `running` runs to `paused` and records the recovery checkpoint.
- `waiting_signal` runs survive restart with their waiting state intact.
- `OpenFangKernel` holds typed workflow store handles rather than a raw `compozy.db` connection.
- The next task can wire the workflow engine's state transitions to these store adapters without modifying the migration slice or the store adapter API.

---

## Notes

- Keep this task limited to Phase 1 workflow tables.
