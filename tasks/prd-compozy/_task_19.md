## markdown

## status: pending

<task_context>
<domain>engine/workflows/recovery</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task6,task16,task17</dependencies>
</task_context>

# Task 19.0: Restart Recovery And Durable Run Control Surfaces

## Overview

Implement conservative startup recovery for durable workflow runs and route all
run control-plane read surfaces to `compozy.db` instead of in-memory state.
When the daemon restarts, any run whose last persisted status was `running` must
be downgraded to `paused` because execution context is gone and the run cannot
resume automatically in Phase 1. Runs in `waiting_signal` or `waiting_hitl`
status must survive restart unchanged — their waiting state is durable and
meaningful. The recovery decision must be recorded as a checkpoint so the full
state history remains auditable. After recovery, `GET /api/v1/runs`,
`GET /api/v1/runs/{id}`, `GET /api/v1/runs/{id}/checkpoints`, and
`GET /api/v1/runs/{id}/signals` must all return data sourced exclusively from
`compozy.db`. This task finishes the first durable workflow-core slice
established in ADR-021.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- On daemon startup, scan `workflow_run` in `compozy.db` for all rows whose
  `status` is `running`. For each such run, downgrade `status` to `paused` and
  insert a `run_recovered_needs_resume` checkpoint into `workflow_checkpoint`.
  This conservative policy is mandatory in Phase 1 per ADR-021: auto-resuming
  arbitrary in-flight execution is explicitly deferred. No run should remain in
  `running` status after the recovery scan completes.
- Runs whose `status` is `waiting_signal` must not be downgraded. They have
  durable waiting state (`waiting_kind`, `waiting_ref`) that is intact and
  meaningful. The recovery scan must skip these rows. A signal submitted after
  restart must still be able to resume a `waiting_signal` run correctly.
- Runs whose `status` is `waiting_hitl` must similarly not be downgraded. Their
  active `hitl_request_id` remains valid and the HITL request record persists
  independently.
- Runs in terminal statuses (`completed`, `failed`, `cancelled`) must not be
  touched by the recovery scan. The scan must build an explicit allowlist of
  statuses that trigger downgrade (`running` only in Phase 1) rather than
  downgradeing everything that is not terminal.
- The `run_recovered_needs_resume` checkpoint must record the original status
  before downgrade in its `data` field so operators can reconstruct the full
  timeline. The checkpoint shape must follow the API-SPEC.md §9 checkpoint
  model: `{ "id", "run_id", "step_id", "kind", "data", "created_at" }` with
  `kind = "run_recovered_needs_resume"` and `data = { "previous_status": "running" }`.
- The `POST /api/v1/runs/{id}/pause`, `POST /api/v1/runs/{id}/resume`, and
  `POST /api/v1/runs/{id}/cancel` control-plane actions (API-SPEC.md §9) must
  be backed by `compozy.db` writes, not in-memory mutations. Pause must set
  `status = paused`; resume must set `status = running` (only from `paused`);
  cancel must set `status = cancelled` and `completed_at`. Each action must
  emit the corresponding checkpoint kind.
- All run read surfaces — `GET /api/v1/runs`, `GET /api/v1/runs/{id}`,
  `GET /api/v1/runs/{id}/checkpoints`, `GET /api/v1/runs/{id}/dispatches`,
  `GET /api/v1/runs/{id}/signals` — must query `compozy.db` exclusively. No
  route handler may fall back to in-memory state when the database row exists.
  This is the API exposure rule stated in ADR-026 and DESIGN.md §20.
- The list endpoint `GET /api/v1/runs` must support the `status` and
  `waiting_kind` filters defined in API-SPEC.md §9 List Filters, so operators
  can enumerate paused runs that need manual resume after a crash recovery.
</requirements>

## Subtasks

- [ ] 19.1 Implement the startup recovery scan as a dedicated async function
      called during kernel boot, after the `compozy.db` connection is
      established but before the HTTP server begins accepting requests. The
      function must: open `compozy.db`, query all `workflow_run` rows with
      `status = 'running'`, update each to `status = 'paused'`, and insert a
      `run_recovered_needs_resume` checkpoint. Use a SQLite transaction so the
      scan is atomic: either all affected runs are downgraded together or none
      are.
- [ ] 19.2 Extend the `WorkflowRunStatus` enum to include `Paused` and
      `Cancelled` variants alongside the `WaitingSignal` variant introduced in
      Task 17. Update all match arms across `workflow.rs`, any repository code,
      and route handlers. Ensure `Paused` serializes to `"paused"` and
      `Cancelled` serializes to `"cancelled"` in JSON responses to match the
      run detail shape in API-SPEC.md §9.
- [ ] 19.3 Implement `WorkflowRunRepository` with at minimum: `find_by_id`,
      `list` (with `status` and `waiting_kind` filters), `update_status`,
      `insert_checkpoint`, and `find_checkpoints_for_run`. All methods must
      operate on `compozy.db`. Follow the `Arc<Mutex<Connection>>` with WAL
      mode pattern from `crates/openfang-memory/src/substrate.rs`.
- [ ] 19.4 Implement run control-plane action handlers. Wire
      `POST /api/v1/runs/{id}/pause`, `POST /api/v1/runs/{id}/resume`, and
      `POST /api/v1/runs/{id}/cancel` through `WorkflowRunRepository`. Pause
      must only succeed from `running` or `waiting_signal` status. Resume must
      only succeed from `paused`. Cancel must succeed from any non-terminal
      status. Return HTTP 409 with a structured error if the transition is
      invalid.
- [ ] 19.5 Route all run read surfaces to `WorkflowRunRepository`. Replace any
      in-memory lookups in existing route handlers for `GET /api/v1/runs`,
      `GET /api/v1/runs/{id}`, `GET /api/v1/runs/{id}/checkpoints`, and
      `GET /api/v1/runs/{id}/dispatches` with repository calls. Register any
      missing routes in `crates/openfang-api/src/server.rs`.
- [ ] 19.6 Add the `run_paused`, `run_resumed`, and `run_cancelled` checkpoint
      kinds. Each control-plane action (pause, resume, cancel) must insert a
      checkpoint with the corresponding kind and a `data` field recording the
      actor source (`"api"` or `"system"`). The checkpoint must be inserted in
      the same transaction as the status update.
- [ ] 19.7 Integrate the recovery scan into the daemon boot sequence. The scan
      must execute before `ShutdownCoordinator` moves out of `ShutdownPhase::Running`
      and before the HTTP server begins accepting requests. Log the count of
      recovered runs at `info` level and each recovered `run_id` at `debug` level.

## Implementation Details

The current state of the codebase has no recovery scan. The existing
`ShutdownCoordinator` in `crates/openfang-runtime/src/graceful_shutdown.rs`
manages ordered teardown phases (`Draining`, `WaitingForAgents`, `FlushingAudit`,
`ClosingDatabase`, `Complete`) but has no corresponding startup recovery hook.
The boot sequence in `crates/openfang-kernel/src/kernel.rs` assembles all
subsystems (registry, scheduler, workflow engine, triggers, background
executor, audit log, metering) but performs no durable-state recovery because
the workflow engine is currently in-memory only.

The current `WorkflowEngine` in `crates/openfang-kernel/src/workflow.rs` holds
`runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>`. After this task,
the canonical run state must live in `compozy.db` and the in-memory map
becomes a transient execution cache only — if it is populated at all for
non-recovered runs.

The conservative recovery policy, as specified in ADR-021 and DESIGN.md §14
(Foundational / durable workflow runtime), is:

```
running         -> paused    (checkpoint: run_recovered_needs_resume)
waiting_signal  -> (no change, Task 17 signal delivery still works)
waiting_hitl    -> (no change, HITL request record still valid)
paused          -> (no change, already at rest)
completed       -> (no change, terminal)
failed          -> (no change, terminal)
cancelled       -> (no change, terminal)
```

This is Phase 1 only. Auto-resume for `running` runs (e.g., re-dispatching the
last in-flight step) is explicitly deferred to a later phase per ADR-021.

The run detail shape from API-SPEC.md §9 that read surfaces must return:

```json
{
  "id": "run_123",
  "workflow_id": "sdlc",
  "workflow_version": "1.0.0",
  "status": "paused",
  "input": {},
  "vars": {},
  "current_step_id": "write-prd",
  "waiting_kind": null,
  "waiting_ref": null,
  "active_dispatch_id": null,
  "active_hitl_request_id": null,
  "labels": [],
  "metadata": {},
  "error": null,
  "started_at": "2026-03-21T14:05:00Z",
  "updated_at": "2026-03-21T14:06:00Z",
  "completed_at": null
}
```

The list endpoint must support `?status=paused` to let operators enumerate all
runs that need manual resume after a crash, and `?waiting_kind=signal` to find
all parked `waiting_signal` runs.

The checkpoint shape for recovery (inserted per recovered run):

```json
{
  "id": "chk_recovery_001",
  "run_id": "run_123",
  "step_id": "write-prd",
  "kind": "run_recovered_needs_resume",
  "data": { "previous_status": "running" },
  "created_at": "2026-03-23T09:00:00Z"
}
```

The control-plane action endpoints specified in API-SPEC.md §9:

- `POST /api/v1/runs/{id}/pause` — valid from `running`, `waiting_signal`
- `POST /api/v1/runs/{id}/resume` — valid from `paused` only
- `POST /api/v1/runs/{id}/cancel` — valid from any non-terminal status

The `compozy runs list --status paused` CLI surface (API-SPEC.md §14 CLI Mirror)
should reflect the recovery output so operators can discover and manually resume
any run that was downgraded.

### Relevant Files

- `crates/openfang-kernel/src/kernel.rs` — boot sequence, subsystem assembly
- `crates/openfang-kernel/src/workflow.rs` — in-memory engine to be extended with durable read paths
- `crates/openfang-api/src/routes.rs` — existing route handlers
- `crates/openfang-api/src/server.rs` — route registration
- `crates/openfang-runtime/src/graceful_shutdown.rs` — `ShutdownCoordinator` and `ShutdownPhase` for boot-phase reference
- `crates/openfang-memory/src/substrate.rs` — SQLite connection and WAL pattern to follow
- `tasks/prd-compozy/docs/API-SPEC.md` §9 — Runs, control actions, checkpoint shape
- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` §3 — `workflow_run`, `workflow_checkpoint`
- `tasks/prd-compozy/docs/DESIGN.md` §14 — priorities, durable runtime first
- ADR-021, ADR-026, ADR-032

### Dependent Files

- API integration tests for run list and detail
- Daemon boot tests verifying recovery scan fires before request acceptance
- Task 17 `WorkflowSignalRepository` — must not be disrupted by recovery scan

## Deliverables

- Startup recovery scan function integrated into the kernel boot sequence
- `WorkflowRunStatus::Paused` and `WorkflowRunStatus::Cancelled` variants
- `WorkflowRunRepository` backed by `compozy.db`
- Run control-plane action handlers (pause, resume, cancel) with checkpoint emission
- All run read surfaces routed to `compozy.db`
- Tests covering all paths described below

## Tests

### Unit Tests (Required)

- [ ] `running_runs_downgrade_to_paused_on_recovery_scan`: Insert a `running`
      run into `compozy.db`, execute the recovery scan, assert the row now has
      `status = paused` and a `run_recovered_needs_resume` checkpoint exists
      with `data.previous_status = "running"`.
- [ ] `waiting_signal_runs_survive_recovery_scan_unchanged`: Insert a
      `waiting_signal` run, execute the recovery scan, assert the row status,
      `waiting_kind`, and `waiting_ref` are all unchanged.
- [ ] `waiting_hitl_runs_survive_recovery_scan_unchanged`: Insert a
      `waiting_hitl` run, execute the recovery scan, assert the row is
      unchanged.
- [ ] `terminal_runs_are_not_touched_by_recovery_scan`: Insert runs with
      `completed`, `failed`, and `cancelled` statuses, execute the scan, assert
      none are mutated and no extra checkpoints are inserted.
- [ ] `recovery_scan_is_atomic`: Inject a connection failure midway through the
      recovery transaction, assert the database is not in a partially-downgraded
      state after reconnect (all runs remain `running` or all are `paused`).
- [ ] `recovery_checkpoints_record_previous_status`: Assert the `data` JSON on
      every `run_recovered_needs_resume` checkpoint contains
      `"previous_status": "running"` exactly.
- [ ] `pause_action_rejects_invalid_source_status`: Attempt to pause a
      `completed` run; assert `update_status` returns an error without writing
      any row or checkpoint.
- [ ] `resume_action_only_valid_from_paused`: Attempt to resume a `running` run
      directly; assert the operation is rejected and the status remains `running`.

### Integration Tests (Required)

- [ ] `restart_after_in_flight_run_preserves_durable_state`: Start a run,
      simulate an abrupt restart (drop in-memory engine state, re-open
      `compozy.db`), assert the run is now `paused` and
      `GET /api/v1/runs/{id}` returns the `paused` status with a
      `run_recovered_needs_resume` checkpoint visible at
      `GET /api/v1/runs/{id}/checkpoints`.
- [ ] `get_run_list_reflects_recovered_state`: After recovery, call
      `GET /api/v1/runs?status=paused`; assert all previously-running runs
      appear in the response.
- [ ] `signal_and_checkpoint_history_intact_after_restart`: Insert signals and
      checkpoints before restart, simulate restart, assert both
      `GET /api/v1/runs/{id}/signals` and `GET /api/v1/runs/{id}/checkpoints`
      return the pre-restart records without loss.
- [ ] `pause_resume_cancel_round_trip_through_db`: Call pause, then resume, then
      cancel on a run; assert each intermediate status is reflected in
      `GET /api/v1/runs/{id}` and each action produces the expected checkpoint.
- [ ] `waiting_signal_run_still_accepts_signal_after_restart`: Park a run at a
      `wait_signal` step, simulate restart, submit a signal via
      `POST /api/v1/runs/{id}/signals`, assert the run resumes correctly.

### Regression and Anti-Pattern Guards

- [ ] Do not auto-resume arbitrary in-flight `running` execution in Phase 1 —
      any code path that transitions a recovered run from `paused` back to
      `running` without an explicit operator `resume` action is a test failure.
- [ ] Do not keep API reads pointed at in-memory state after durability lands —
      any route handler that reads `WorkflowEngine.runs` directly (bypassing
      the repository) must be flagged by a test that clears the in-memory map
      and asserts the API still returns data.
- [ ] Do not hide recovery decisions from checkpoint history — a run with no
      `run_recovered_needs_resume` checkpoint after restart, despite having been
      `running` at the time, is a test failure.
- [ ] Do not apply the downgrade to `waiting_signal` or `waiting_hitl` runs —
      a test must assert that the count of `waiting_signal` runs before and
      after the recovery scan is identical.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- After a daemon restart, no `workflow_run` row remains in `running` status.
  Every run that was `running` at crash time is `paused` with a
  `run_recovered_needs_resume` checkpoint, confirmed by reading `compozy.db`
  directly after boot.
- `waiting_signal` and `waiting_hitl` runs survive restart with their status
  and waiting metadata intact, confirmed by both a direct DB assertion and a
  round-trip through the API.
- `GET /api/v1/runs`, `GET /api/v1/runs/{id}`, `GET /api/v1/runs/{id}/checkpoints`,
  and `GET /api/v1/runs/{id}/signals` all return correct data after in-memory
  state is dropped, confirming they read exclusively from `compozy.db`.
- `POST /api/v1/runs/{id}/pause`, `POST /api/v1/runs/{id}/resume`, and
  `POST /api/v1/runs/{id}/cancel` each produce the correct status transition
  and a matching checkpoint in `compozy.db`.
- The recovery scan is idempotent: running it twice on the same database
  produces no additional downgrade or duplicate checkpoints.
- The recovery scan completes and logs its results before the HTTP server
  accepts the first request, confirmed by a boot-sequence ordering test.
- All cargo verification commands pass with zero warnings and zero errors.

---

## Notes

- This task finishes the first durable workflow-core slice per ADR-021. Future
  phases may introduce auto-resume logic (re-dispatching the last in-flight
  step) but that is explicitly out of scope here.
- The `run_recovered_needs_resume` checkpoint kind introduces a recoverable
  lineage pattern that later tasks can build on for audit and observability.
- Operators can discover all paused-from-recovery runs with
  `GET /api/v1/runs?status=paused` or `compozy runs list --status paused`, then
  manually resume each with `POST /api/v1/runs/{id}/resume` once they have
  confirmed the run is safe to continue.
