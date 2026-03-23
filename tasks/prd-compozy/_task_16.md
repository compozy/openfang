## markdown

## status: pending

<task_context>
<domain>engine/workflows/runtime</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task6,task8,task9</dependencies>
</task_context>

# Task 16.0: Durable Workflow Run Repository And Transition Writer

## Overview

Replace the purely in-memory workflow run lifecycle with a durable repository
and transition writer built on the `workflow_run` and `workflow_checkpoint`
tables in `compozy.db`, per ADR-005 (durable workflow runtime) and ADR-021
(runtime-first workflow hardening).

The current `WorkflowEngine` in `crates/openfang-kernel/src/workflow.rs` stores
all run state in `runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>`. Every
`WorkflowRun` lives exclusively in memory: `state`, `step_results`, `output`,
`error`, and `completed_at` are all transient. The `MAX_RETAINED_RUNS` cap (200)
means old runs are evicted silently. There is no checkpoint trail, no signal
table, and no dispatch record. A daemon restart erases all run history — a run
in the `Running` state at shutdown cannot be recovered.

Per ADR-005, the first durable cut must persist: `workflow_run`,
`workflow_checkpoint`, `agent_dispatch`, `hitl_request`, `workflow_signal`, and
`looper_run`. This task focuses on the two foundational objects — `workflow_run`
and `workflow_checkpoint` — and the transition writer that keeps them coherent.
`agent_dispatch` and `hitl_request` persistence are covered in subsequent tasks.
`workflow_signal` is introduced here as a supporting table because `wait_signal`
step execution (from task 13) requires it.

Per ADR-021, runtime durability is the highest-priority Phase 1 concern. No
durable product feature can build on a transient-only run model.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Workflow run creation must persist a `workflow_run` row in `compozy.db` before execution begins. A run that exists only in memory is not acceptable for any part of the lifecycle.
- Every major state transition must be recorded as a `workflow_checkpoint` row immediately before the in-memory state is updated. The checkpoint trail must be complete enough to reconstruct the run's lifecycle history from the database alone.
- The transition writer must be the single code path for all `workflow_run` status updates. Scattered direct writes to the in-memory `HashMap` must be eliminated. No code outside the transition writer may mutate `workflow_run.status`, `workflow_run.current_step_id`, `workflow_run.waiting_kind`, or `workflow_run.error`.
- Signals (`workflow_signal` table) must be persisted before being consumed by a `wait_signal` step. A signal that is consumed but not persisted cannot be replayed after a restart.
- After a restart, the durable run repository must be able to reconstruct the set of non-terminal runs (`pending`, `running`, `waiting`) from `compozy.db` so the runtime can resume or report their interrupted state.
- The in-memory run cache must be kept in sync with `compozy.db` at all times. The canonical state of a run is always what is stored in the database. The in-memory cache is a performance projection, not the source of truth.
- The `GET /api/v1/runs/{id}` and `GET /api/v1/workflows/{id}/runs` endpoints must read from the durable repository, not from the in-memory `HashMap`. The `MAX_RETAINED_RUNS` eviction cap must not affect the database.
</requirements>

## Subtasks

- [ ] 16.1 Write the `compozy.db` migration adding the `workflow_run`, `workflow_checkpoint`, and `workflow_signal` tables per `DATABASE-SCHEMA.md`. The migration must be idempotent and must use the existing migration infrastructure in `crates/openfang-memory/src/migration.rs` or the equivalent `compozy.db` migration path.
- [ ] 16.2 Implement `WorkflowRunRepository` in a new file (e.g., `crates/openfang-memory/src/workflow_run.rs` or a new `crates/compozy-store/` crate): CRUD operations for `workflow_run` rows, checkpoint append, signal insert and consume, and a `list_non_terminal` query for restart recovery.
- [ ] 16.3 Implement `WorkflowCheckpointRepository` with `append(run_id, step_id, kind, data)` and `list_for_run(run_id)` operations. Checkpoint kinds must cover the full lifecycle: `run_created`, `run_started`, `step_started`, `step_completed`, `step_failed`, `step_skipped`, `signal_received`, `run_completed`, `run_failed`, `run_cancelled`.
- [ ] 16.4 Implement the `TransitionWriter` struct that wraps `WorkflowRunRepository` and `WorkflowCheckpointRepository` and exposes named transition methods: `record_run_created`, `record_run_started`, `record_step_started`, `record_step_completed`, `record_step_failed`, `record_run_completed`, `record_run_failed`, `record_run_cancelled`. Each method must write the checkpoint row first, then update the `workflow_run` row, and finally update the in-memory cache.
- [ ] 16.5 Replace all direct `runs.write().await.get_mut(&run_id)` mutations in `WorkflowEngine::execute_run` and `WorkflowEngine::create_run` with `TransitionWriter` calls. No direct HashMap mutation of run state may remain after this subtask.
- [ ] 16.6 Update `WorkflowEngine::create_run` to persist a `workflow_run` row via `WorkflowRunRepository` before inserting into the in-memory cache. If the database write fails, the in-memory insertion must not proceed and an error must be returned.
- [ ] 16.7 Update `GET /api/v1/runs/{id}`, `GET /api/v1/runs`, and `GET /api/v1/workflows/{id}/runs` in `crates/openfang-api/src/routes.rs` to read from `WorkflowRunRepository` rather than `WorkflowEngine::list_runs` (which reads from the in-memory HashMap).
- [ ] 16.8 Implement restart recovery: on daemon boot, after `bootstrap_workflow_definitions` (task 8) completes, call `WorkflowRunRepository::list_non_terminal` and populate the in-memory run cache from the database rows. Runs that were `running` at shutdown must be transitioned to a new `interrupted` status or reported via the API as `status: "interrupted"`.

## Implementation Details

### `workflow_run` Table

Per `DATABASE-SCHEMA.md`, the representative columns are:

- `run_id TEXT PRIMARY KEY`
- `workflow_id TEXT NOT NULL`
- `workflow_version TEXT NOT NULL`
- `status TEXT NOT NULL` — `pending`, `running`, `waiting`, `completed`, `failed`, `cancelled`, `interrupted`
- `input_json TEXT NOT NULL`
- `vars_json TEXT NOT NULL DEFAULT '{}'`
- `current_step_id TEXT`
- `waiting_kind TEXT` — `signal`, `hitl`, `dispatch`
- `waiting_ref TEXT` — reference to the signal name, hitl ID, or dispatch ID being awaited
- `active_dispatch_id TEXT`
- `active_hitl_request_id TEXT`
- `labels_json TEXT NOT NULL DEFAULT '[]'`
- `metadata_json TEXT NOT NULL DEFAULT '{}'`
- `error_json TEXT`
- `started_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`
- `completed_at TEXT`

The `status` field must use string-encoded values matching the API-SPEC.md run
detail shape. All JSON columns must store valid JSON, never raw strings.

### `workflow_checkpoint` Table

Per `DATABASE-SCHEMA.md`:

- `checkpoint_id TEXT PRIMARY KEY`
- `run_id TEXT NOT NULL REFERENCES workflow_run(run_id)`
- `step_id TEXT` — null for run-level checkpoints (`run_created`, `run_completed`, etc.)
- `kind TEXT NOT NULL`
- `data_json TEXT NOT NULL DEFAULT '{}'`
- `created_at TEXT NOT NULL`

Checkpoint kinds and their required `data_json` fields:

| kind              | required data fields                                      |
| ----------------- | --------------------------------------------------------- |
| `run_created`     | `workflow_id`, `workflow_version`, `input`                |
| `run_started`     | `initial_step_id`                                         |
| `step_started`    | `step_id`, `kind`, `agent` or `primitive` (if applicable) |
| `step_completed`  | `step_id`, `save_as` (if applicable), `output_summary`    |
| `step_failed`     | `step_id`, `error`, `attempt`                             |
| `step_skipped`    | `step_id`, `reason`                                       |
| `signal_received` | `signal_name`, `signal_id`, `payload_summary`             |
| `run_completed`   | `final_output_summary`                                    |
| `run_failed`      | `error`, `failing_step_id`                                |
| `run_cancelled`   | `cancelled_by`, `reason`                                  |

### `workflow_signal` Table

Per `DATABASE-SCHEMA.md`:

- `signal_id TEXT PRIMARY KEY`
- `run_id TEXT NOT NULL REFERENCES workflow_run(run_id)`
- `name TEXT NOT NULL`
- `payload_json TEXT NOT NULL DEFAULT '{}'`
- `source TEXT NOT NULL`
- `consumed INTEGER NOT NULL DEFAULT 0`
- `created_at TEXT NOT NULL`
- `consumed_at TEXT`

Signal submission (`POST /api/v1/runs/{id}/signals`) must insert a row here
before the signal is dispatched to any waiting `wait_signal` step. Signal
consumption must update `consumed = 1` and `consumed_at` atomically with the
step resumption checkpoint.

### `TransitionWriter` Design

The `TransitionWriter` must expose methods that enforce the correct write order:

1. Write `workflow_checkpoint` row (append-only, never fails silently).
2. Update `workflow_run` row (status, current_step_id, vars_json, etc.).
3. Update in-memory cache (in-memory `HashMap` or equivalent projection).

If step 1 or step 2 fails, the transition must return an error. Step 3 is
performed only after steps 1 and 2 succeed. This ensures the database is always
ahead of or equal to the in-memory cache — never behind.

The `TransitionWriter` must also expose a `load_run` method that reads the
canonical run state from the database (not from memory) and returns it as a
`WorkflowRunRecord`. This is the correct read path for the API endpoints.

The in-memory HashMap in `WorkflowEngine` transitions from being the run
store to being a performance projection cache. It may be removed entirely in a
later task once all reads go through `WorkflowRunRepository`.

### Run Status State Machine

Valid status transitions that the `TransitionWriter` must enforce:

```
pending → running → completed
pending → running → failed
pending → running → waiting → running (signal received)
pending → running → cancelled
running → interrupted (on daemon restart)
interrupted → pending (on explicit re-queue, future task)
```

Any transition not in this table must be rejected with a `TransitionError`.
The `TransitionWriter` must check the current `status` before writing the new
status and return an error if the transition is invalid.

### API-SPEC.md Run Detail Shape

The `GET /api/v1/runs/{id}` response must match the shape defined in
API-SPEC.md section 9:

```json
{
  "id": "run_123",
  "workflow_id": "sdlc",
  "workflow_version": "1.0.0",
  "status": "running",
  "input": { "issue_id": "ISSUE-123" },
  "vars": { "issue": { "id": "ISSUE-123" } },
  "current_step_id": "write-prd",
  "waiting_kind": null,
  "waiting_ref": null,
  "active_dispatch_id": "dispatch_456",
  "active_hitl_request_id": null,
  "labels": ["manual"],
  "metadata": { "source": "api" },
  "error": null,
  "started_at": "2026-03-21T14:05:00Z",
  "updated_at": "2026-03-21T14:06:00Z",
  "completed_at": null
}
```

The `GET /api/v1/runs/{id}/checkpoints` endpoint must return a list of
`workflow_checkpoint` rows per API-SPEC.md section 9.

### Relevant Files

- `crates/openfang-kernel/src/workflow.rs` — `WorkflowEngine`, `WorkflowRun`, `WorkflowRunState`, `create_run`, `execute_run`, `get_run`, `list_runs`
- `crates/openfang-kernel/src/kernel.rs` — `run_workflow`, `start_background_agents` (for restart recovery hook)
- `crates/openfang-memory/src/` — existing migration and substrate infrastructure
- `crates/openfang-api/src/routes.rs` — `run_workflow`, `list_workflow_runs`, `get_workflow` handlers
- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — `workflow_run`, `workflow_checkpoint`, `workflow_signal` table outlines
- `tasks/prd-compozy/docs/API-SPEC.md` — section 9 (Runs: endpoints, detail shape, checkpoint shape, signal submission)
- `tasks/prd-compozy/docs/INITIAL-RUNTIME-MIGRATIONS.md` — Phase 0 and Phase 1 migration slice for `compozy.db`
- `tasks/prd-compozy/docs/adrs/005-durable-workflow-runtime.md`
- `tasks/prd-compozy/docs/adrs/021-runtime-first-workflow-hardening.md`
- `tasks/prd-compozy/docs/adrs/037-file-backed-definitions-and-db-ownership.md`
- `tasks/prd-compozy/docs/adrs/003-separate-compozy-domain-database.md`

### Dependent Files

- `crates/openfang-api/src/routes.rs` — all run-related API handlers must be updated to read from `WorkflowRunRepository`
- Task 14's `WorkflowIr` — `workflow_run` rows must store `workflow_version` from the compiled IR, not from the raw definition

## Deliverables

- `compozy.db` migration adding `workflow_run`, `workflow_checkpoint`, and `workflow_signal` tables.
- `WorkflowRunRepository` with full CRUD, checkpoint append, signal lifecycle, and `list_non_terminal` for restart recovery.
- `WorkflowCheckpointRepository` with `append` and `list_for_run`.
- `TransitionWriter` with named transition methods and enforced write order (checkpoint → run row → memory cache).
- Updated `WorkflowEngine::create_run` persisting to `compozy.db` before memory insertion.
- Updated `execute_run` routing all state mutations through `TransitionWriter`.
- Updated API read paths using `WorkflowRunRepository` instead of the in-memory HashMap.
- Restart recovery: `list_non_terminal` called at boot to populate the in-memory cache.
- Tests covering run creation, all checkpoint kinds, signal lifecycle, terminal state persistence, and restart recovery.

## Tests

### Unit Tests (Required)

- [ ] `run_creation_persists_workflow_run_row`: call `create_run`, then query `compozy.db` directly, assert a `workflow_run` row exists with `status = 'pending'` and correct `workflow_id`, `workflow_version`, and `input_json`.
- [ ] `run_creation_appends_run_created_checkpoint`: after `create_run`, assert a `workflow_checkpoint` row exists with `kind = 'run_created'` and `data_json` containing `workflow_id` and `input`.
- [ ] `transition_writer_step_completed_updates_run_and_appends_checkpoint`: call `record_step_completed` on the writer, assert the `workflow_run.current_step_id` is updated in the database and a `step_completed` checkpoint row is appended.
- [ ] `transition_writer_rejects_invalid_status_transition`: attempt to call `record_run_completed` on a run in `pending` state (not `running`), assert a `TransitionError` is returned and no database mutation occurred.
- [ ] `terminal_state_run_failed_persists_error_json`: force a step failure through `execute_run`, assert the `workflow_run` row has `status = 'failed'` and `error_json` contains the error message and the failing step ID.
- [ ] `terminal_state_run_completed_sets_completed_at`: complete a run successfully, assert `workflow_run.status = 'completed'` and `completed_at` is set to a non-null RFC 3339 timestamp.
- [ ] `signal_insert_persists_before_consumption`: submit a signal, assert a `workflow_signal` row exists with `consumed = 0`, then consume it, assert `consumed = 1` and `consumed_at` is set.
- [ ] `signal_consumption_appends_signal_received_checkpoint`: when a `wait_signal` step consumes a signal, assert a `signal_received` checkpoint is appended to the run's checkpoint trail.
- [ ] `list_non_terminal_returns_pending_and_running_runs`: insert one `completed` run, one `running` run, and one `pending` run, call `list_non_terminal`, assert only the `running` and `pending` runs are returned.

### Integration Tests (Required)

- [ ] `starting_workflow_creates_durable_run_record_immediately`: submit `POST /api/v1/workflows/{id}/runs`, assert the response contains a `run_id`, then query `GET /api/v1/runs/{run_id}` and assert `status` is not empty and the run row is present in `compozy.db`.
- [ ] `run_list_after_execution_reflects_durable_state`: execute a workflow to completion, then call `GET /api/v1/workflows/{id}/runs`, assert the completed run appears in the list with the correct `status` even after the in-memory HashMap has been cleared.
- [ ] `restart_retains_run_identity_and_transitions`: create a run, simulate a restart by reinitializing `WorkflowRunRepository` and calling `list_non_terminal`, assert the run is re-populated in the in-memory cache with its original `run_id` and `status`.
- [ ] `checkpoint_list_reflects_full_run_lifecycle`: execute a two-step sequential workflow to completion, call `GET /api/v1/runs/{id}/checkpoints`, assert checkpoints include `run_created`, `run_started`, two `step_started`, two `step_completed`, and `run_completed` in that order.
- [ ] `interrupted_run_is_visible_after_restart`: start a run, simulate a daemon restart without completing the run, call the restart recovery path, assert the run appears with `status: "interrupted"` in `GET /api/v1/runs/{id}`.

### Regression and Anti-Pattern Guards

- [ ] No `WorkflowRun` state exists only in the in-memory HashMap after any successful mutation — every mutation that changes `status`, `current_step_id`, `vars`, or `error` must also write to `compozy.db`.
- [ ] No direct `runs.write().await.get_mut(&run_id)` mutation of `status` or `current_step_id` remains in `WorkflowEngine::execute_run` after the `TransitionWriter` is wired in.
- [ ] The `MAX_RETAINED_RUNS` eviction cap must not apply to the database. Eviction from the in-memory cache is acceptable; eviction from `compozy.db` is not.
- [ ] The `GET /api/v1/runs` and `GET /api/v1/runs/{id}` handlers must not call `WorkflowEngine::list_runs` or `WorkflowEngine::get_run` (which read from memory). They must call `WorkflowRunRepository` methods.
- [ ] The `TransitionWriter` must enforce the checkpoint-before-run-update write order in all code paths — no shortcut writes that update the run row without a preceding checkpoint append.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Workflow runs are durable from creation onward: every run that the API acknowledges with a `run_id` has a corresponding row in `compozy.db` that survives a daemon restart.
- The major lifecycle transitions (`run_created`, `run_started`, `step_started`, `step_completed`, `step_failed`, `run_completed`, `run_failed`) are each recorded as `workflow_checkpoint` rows in order, forming a complete audit trail.
- Signals are persisted before consumption. A `wait_signal` step that receives a signal always has a corresponding `workflow_signal` row with `consumed = 1`.
- After a restart, `list_non_terminal` correctly identifies all non-terminal runs and populates the in-memory cache. Runs that were `running` at shutdown appear as `interrupted`.
- The `TransitionWriter` is the single mutation path for all run status changes. No scattered direct HashMap writes remain.
- `GET /api/v1/runs/{id}` reads from `WorkflowRunRepository` and returns the same data that is stored in `compozy.db`.
- `GET /api/v1/runs/{id}/checkpoints` returns the full ordered checkpoint trail for any run.
- The `MAX_RETAINED_RUNS` eviction only affects the in-memory cache, never the `compozy.db` rows.
- `cargo test --workspace` passes with zero failures and `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.

---

## Notes

- This task is the most important runtime pivot in the whole PRD. ADR-005 explicitly names `workflow_run` and `workflow_checkpoint` as the foundational durable objects. Everything else in Phase 1 (dispatch, HITL, looper) depends on these being stable.
- ADR-021 says: "early implementation work should prioritize state models, transitions, recovery, and dispatch/HITL handling." The `TransitionWriter` and `WorkflowRunRepository` are the concrete deliverables of that mandate.
- The `compozy.db` database is separate from `runtime.db` per ADR-003 and ADR-037. Do not co-locate workflow run tables in `runtime.db`. The migration must target `compozy.db` specifically.
- Do not introduce `workflow_step_run` as part of this task. Per ADR-005, it is a valid later addition but is not required to begin the durable runtime refactor. The `workflow_checkpoint` table provides sufficient step-level observability for Phase 1.
- The `agent_dispatch` and `hitl_request` tables are referenced in DATABASE-SCHEMA.md but are not the focus of this task. Their foreign key references from `workflow_run` (via `active_dispatch_id` and `active_hitl_request_id`) should be present as nullable columns but the tables themselves may be stubbed until the dispatch and HITL tasks (subsequent to task 14) land.
