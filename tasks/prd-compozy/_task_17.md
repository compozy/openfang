## markdown

## status: pending

<task_context>
<domain>engine/workflows/signals</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task9,task16</dependencies>
</task_context>

# Task 17.0: Workflow Signal Persistence And Waiting-State Integration

## Overview

Implement durable persistence for `workflow_signal` records in `compozy.db` and
wire signal delivery into the `waiting_signal` status of `workflow_run`. A run
entering a `wait_signal` step must write its waiting state to the database
before suspending, so that any signal arriving — whether before or after the
engine parks the run — is matched and consumed correctly. This task covers the
signal submission surface, the repository operations that back it, the
state-machine transitions that govern waiting, and the API routes that expose
both submission and listing. Recovery semantics for waiting runs across restart
are handled in Task 19.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Persist every signal independently of in-memory delivery. A signal written to
  `workflow_signal` in `compozy.db` must survive a restart with its `consumed`
  flag and `consumed_at` timestamp intact. This is the first-class durability
  contract specified in ADR-005 and reflected in the `workflow_signal` table
  schema (columns: `signal_id`, `run_id`, `name`, `payload_json`, `source`,
  `consumed`, `created_at`, `consumed_at`).
- When a run reaches a `wait_signal` step it must set its `waiting_kind` to
  `signal` and write the `waiting_ref` (the expected signal name) to
  `workflow_run` before yielding control. This ensures the state machine
  position is durable even if the process crashes while the run is parked.
- Signal delivery to a waiting run must be a transactional operation: mark the
  `workflow_signal` row as consumed (`consumed = true`, `consumed_at` set),
  clear `waiting_kind` and `waiting_ref` on the `workflow_run` row, and emit a
  `run_resumed_from_signal` checkpoint — all in one logical write, never
  partially applied.
- A signal submitted to a run that is not yet waiting must be persisted and
  held as an unconsumed record. When the run later enters the matching
  `wait_signal` step it must check for an already-persisted unconsumed signal
  with the matching name and consume it immediately rather than parking.
- The signal submission endpoint `POST /api/v1/runs/{id}/signals` (see
  API-SPEC.md §9, Signal Submission) must accept the payload `{ "name",
  "payload", "source", "idempotency_key" }` and return the signal detail shape
  `{ "id", "run_id", "name", "payload", "source", "consumed", "created_at",
  "consumed_at" }`. Idempotency must be enforced: submitting the same
  `idempotency_key` twice returns the existing signal record without inserting
  a duplicate.
- The signal listing endpoint `GET /api/v1/runs/{id}/signals` must read
  exclusively from `compozy.db` (not from any in-memory map) and support
  filtering by `consumed` status. The API surface for runs is defined in
  ADR-026 and API-SPEC.md §9.
- Signal sources must be tracked. Valid sources include `"api"`, `"trigger"`,
  and `"schedule"`. The `source` column in `workflow_signal` must be stored and
  returned verbatim. Trigger and schedule paths that later emit signals can
  inject any of these source values; the signal layer must not hardcode a
  single source.
- The `wait_signal` step kind (defined in DESIGN.md §17 and §21 as a supported
  workflow v2 step) must be validated at the IR level so that a workflow
  lacking an expected signal name in its `wait_signal` step is rejected at
  compile time rather than at runtime. This prevents dangling signal subscriptions.
</requirements>

## Subtasks

- [ ] 17.1 Add `WorkflowSignalRepository` with insert, find-unconsumed, consume,
      and list operations targeting `compozy.db`. Model the full `workflow_signal`
      table from DATABASE-SCHEMA.md (columns: `signal_id`, `run_id`, `name`,
      `payload_json`, `source`, `consumed`, `created_at`, `consumed_at`).
      Enforce unique constraint on `(run_id, idempotency_key)` for duplicate
      submission guard.
- [ ] 17.2 Extend the `WorkflowRun` state machine to support `waiting_signal`
      status. When the engine reaches a `wait_signal` step it must write
      `status = waiting_signal`, `waiting_kind = "signal"`, and
      `waiting_ref = <signal_name>` to `workflow_run` before suspending. Add
      `WorkflowRunStatus::WaitingSignal` to the status enum and update all
      match arms across the codebase.
- [ ] 17.3 Implement transactional signal consumption. When a signal arrives for
      a waiting run: atomically set `workflow_signal.consumed = true` and
      `consumed_at`, clear `workflow_run.waiting_kind` and `waiting_ref`, set
      `workflow_run.status` back to `running`, and insert a
      `run_resumed_from_signal` checkpoint into `workflow_checkpoint`. All four
      writes must succeed or all must be rolled back.
- [ ] 17.4 Implement the eager-consume path. When a run's `wait_signal` step is
      reached the engine must first query `WorkflowSignalRepository` for an
      existing unconsumed signal matching `(run_id, name)`. If one exists,
      consume it immediately (subtask 17.3 path) and advance the run without
      parking. If none exists, write the waiting state (subtask 17.2 path) and
      return control.
- [ ] 17.5 Implement API route handlers for signal submission and listing. Wire
      `POST /api/v1/runs/{id}/signals` and `GET /api/v1/runs/{id}/signals`
      through the repository layer. Both endpoints must read and write through
      `compozy.db` exclusively. Register the routes in `crates/openfang-api/src/server.rs`.
- [ ] 17.6 Validate `wait_signal` steps at workflow compile time. The IR
      compiler must reject a `wait_signal` step that does not specify an
      explicit signal name, and must warn if the same signal name appears in
      two separate `wait_signal` steps in the same workflow without a branching
      structure that makes them mutually exclusive.
- [ ] 17.7 Emit `signal_received` and `signal_consumed` checkpoints into
      `workflow_checkpoint` for every signal lifecycle event. Checkpoint `kind`
      values must match the checkpoint shape from API-SPEC.md §9 (`{ "id",
  "run_id", "step_id", "kind", "data", "created_at" }`).

## Implementation Details

The current workflow engine in `crates/openfang-kernel/src/workflow.rs` is
fully in-memory. `WorkflowEngine` holds `runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>` and signals are not modeled at all. There is no `waiting_signal` status variant in `WorkflowRunState` — it has only `Pending`, `Running`, `Completed`, and `Failed`.

The `MemorySubstrate` in `crates/openfang-memory/src/substrate.rs` shows the
existing SQLite storage pattern: a shared `Arc<Mutex<Connection>>` with WAL
mode and `PRAGMA busy_timeout=5000`. The new `WorkflowSignalRepository` and
`WorkflowRunRepository` must follow this same pattern for `compozy.db`, keeping
the two databases (`runtime.db` for platform state, `compozy.db` for durable
workflow and product-domain state) strictly separate per DESIGN.md §4 and
DATABASE-SCHEMA.md §2–3.

The signal submission contract from API-SPEC.md §9:

```
POST /api/v1/runs/{id}/signals
Body: { "name": "artifact_approved", "payload": {...}, "source": "api", "idempotency_key": "..." }
Response: signal detail shape with "consumed", "created_at", "consumed_at"
```

The signal detail shape returned on `GET /api/v1/runs/{id}/signals` must match
the checkpoint shape in API-SPEC.md §9 precisely. The list endpoint must
support a `consumed` boolean query filter.

The `waiting_kind` and `waiting_ref` columns on `workflow_run` (DATABASE-SCHEMA.md §3)
carry the durable waiting context. Only the `waiting_signal` status variant uses
`waiting_ref` to name the expected signal. Other waiting kinds (`waiting_hitl`,
etc.) are introduced in subsequent tasks.

The checkpoint kind `run_resumed_from_signal` is a new kind alongside the
`dispatch_created` example shown in API-SPEC.md §9. The `data` field for this
kind should carry `{ "signal_id": "...", "signal_name": "..." }`.

ADR-005 establishes `workflow_signal` as a first-class durable object in the
initial durable-cut objects list. ADR-026 establishes that `workflow_signal`
is treated as a first-class execution object even though it surfaces under
`/api/v1/runs/{id}/signals` rather than as a top-level collection.

### Relevant Files

- `crates/openfang-kernel/src/workflow.rs` — current in-memory engine to be extended
- `crates/openfang-kernel/src/kernel.rs` — kernel assembly, subsystem wiring
- `crates/openfang-api/src/routes.rs` — existing route handlers for reference patterns
- `crates/openfang-api/src/server.rs` — route registration
- `crates/openfang-memory/src/substrate.rs` — SQLite connection and migration pattern
- `tasks/prd-compozy/docs/API-SPEC.md` §9 — Runs, signal shapes
- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` §3 — `workflow_signal`, `workflow_run`, `workflow_checkpoint`
- `tasks/prd-compozy/docs/DESIGN.md` §17, §21 — `wait_signal` step kind
- ADR-005, ADR-026

### Dependent Files

- `crates/openfang-api/src/server.rs` — must register the new signal routes
- Future trigger and schedule integration that emits signals with `source = "trigger"` or `source = "schedule"`
- Task 19 recovery scan that must handle `waiting_signal` status runs on restart

## Deliverables

- `WorkflowSignalRepository` backed by `compozy.db` with full CRUD and idempotency
- `WorkflowRunStatus::WaitingSignal` variant and associated state machine transitions
- Transactional signal consumption with checkpoint emission
- Eager-consume path for pre-arrived signals
- `POST /api/v1/runs/{id}/signals` and `GET /api/v1/runs/{id}/signals` route handlers
- `wait_signal` compile-time validation in the IR compiler
- Tests covering all paths described below

## Tests

### Unit Tests (Required)

- [ ] `signal_insert_persists_payload_and_source`: Insert a signal row and read
      it back; assert `name`, `payload_json`, `source`, `consumed = false`,
      and `consumed_at = null` match exactly.
- [ ] `signal_idempotency_key_prevents_duplicate`: Submit the same signal twice
      with the same `idempotency_key`; assert only one row exists in
      `workflow_signal` and the second call returns the existing row.
- [ ] `waiting_run_transitions_status_to_waiting_signal`: Drive a run to a
      `wait_signal` step; assert `workflow_run.status = waiting_signal`,
      `waiting_kind = "signal"`, and `waiting_ref` equals the step's signal name.
- [ ] `signal_consumption_is_transactional`: Simulate a crash mid-consumption
      (connection closed after marking consumed but before clearing
      `waiting_kind`); assert the database is not in a partially-consumed state
      after reconnect.
- [ ] `consumed_flag_and_timestamp_update_atomically`: Consume a signal and
      assert `consumed = true`, `consumed_at` is set, `workflow_run.waiting_kind`
      is cleared, and a `run_resumed_from_signal` checkpoint row exists.
- [ ] `eager_consume_fires_when_signal_arrived_before_wait_step`: Insert an
      unconsumed signal for a run before the run reaches its `wait_signal`
      step; assert the run advances immediately without parking.
- [ ] `list_signals_returns_only_for_requested_run`: Insert signals for two
      different runs; assert listing signals for one run does not return the
      other run's signals.
- [ ] `list_signals_consumed_filter_works`: Insert one consumed and one
      unconsumed signal for the same run; assert `?consumed=true` returns only
      the consumed one and `?consumed=false` returns only the unconsumed one.

### Integration Tests (Required)

- [ ] `post_run_signal_persists_and_affects_run_state`: Call
      `POST /api/v1/runs/{id}/signals` for a run in `waiting_signal` status;
      assert the response carries the signal detail shape, the run transitions
      to `running`, and `GET /api/v1/runs/{id}` reflects the cleared
      `waiting_kind`.
- [ ] `waiting_workflow_resumes_after_durable_signal_delivery`: Start a
      workflow with a `wait_signal` step, park it, submit a matching signal via
      the API, and assert the run advances to the next step and eventually
      completes.
- [ ] `restart_preserves_waiting_state_and_outstanding_signals`: Park a run at
      a `wait_signal` step; simulate a restart (drop in-memory state, reload
      from `compozy.db`); assert the run is still in `waiting_signal` status
      with correct `waiting_ref` and that the outstanding unconsumed signal is
      visible via `GET /api/v1/runs/{id}/signals`.
- [ ] `get_run_signals_reads_from_compozy_db_not_memory`: Call
      `GET /api/v1/runs/{id}/signals` immediately after a simulated in-memory
      cache flush; assert the response still returns the correct persisted
      signal rows.
- [ ] `concurrent_signal_delivery_does_not_double_consume`: Submit two signals
      with the same name to a waiting run concurrently; assert exactly one
      consumption occurs and the second signal remains unconsumed.

### Regression and Anti-Pattern Guards

- [ ] Do not process workflow signals only in memory — any signal that passes
      through the delivery path without a `workflow_signal` insert must fail
      the test with a missing-row assertion.
- [ ] Do not bypass `workflow_run.waiting_kind` persistence when signal delivery
      occurs — consuming a signal while `waiting_kind` is still set after
      commit is a test failure.
- [ ] Do not overfit signal handling to one source — at least one test must use
      `source = "trigger"` and one must use `source = "schedule"` to confirm
      the source column is not hardcoded.
- [ ] Do not allow a `wait_signal` step with no signal name to reach the
      execution path — the compile-time validation test must assert an error is
      returned before any run is created.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Every signal submitted via `POST /api/v1/runs/{id}/signals` is readable via
  `GET /api/v1/runs/{id}/signals` after a process restart with no data loss.
- A run in `waiting_signal` status has durable `waiting_kind` and `waiting_ref`
  values that survive a restart and remain visible to the Task 19 recovery scan.
- Signal consumption is atomic: no partial state is observable between marking
  consumed and clearing `waiting_kind`, even if the database connection is
  interrupted.
- The eager-consume path prevents a run from parking when a matching signal
  already exists, confirmed by at least one end-to-end integration test.
- `POST /api/v1/runs/{id}/signals` with a duplicate `idempotency_key` returns
  HTTP 200 with the existing signal record, never HTTP 409 or a duplicate row.
- `GET /api/v1/runs/{id}/signals` returns only rows from `compozy.db`, never
  from an in-memory fallback, confirmed by a test that drops in-memory state
  and re-queries.
- All cargo verification commands pass with zero warnings and zero errors.

---

## Notes

- This task unlocks explicit workflow continuation semantics and is the
  prerequisite for trigger `workflow_signal` targets defined in DESIGN.md §19
  and API-SPEC.md §6 (`action.kind = "workflow_signal"`).
- The `wait_signal` step kind must also be handled by the Task 19 recovery scan:
  runs in `waiting_signal` status must survive restart without downgrade (unlike
  `running` runs which are downgraded to `paused`). Coordinate with Task 19 on
  the status enum values and recovery policy boundary.
