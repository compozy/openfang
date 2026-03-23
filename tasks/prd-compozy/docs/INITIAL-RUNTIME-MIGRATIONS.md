# Initial Runtime Migrations And Phase 0-1 Execution Plan

**Status:** Current initial migration baseline
**Date:** 2026-03-21

This document turns the high-level implementation order into the first concrete
delivery slice for:

- `runtime.db`
- `compozy.db`
- Phase 0
- Phase 1

> **Phase label note:** Phase 0 and Phase 1 in this document now correspond to
> the 43-task Phase 0 (tasks 1-9) defined in the design decisions, which includes
> both `runtime.db` and `compozy.db` initial schemas. See
> `docs/plans/2026-03-23-prd-decisions-design.md` section 15 for the full phase
> map.

It is intentionally more specific than [IMPLEMENTATION-PLAN.md](IMPLEMENTATION-PLAN.md),
but still stops short of freezing every exact SQL type or ORM detail.

## 1. Scope

This plan covers:

- dual-database migration bootstrap
- the first migration units for both databases
- the runtime changes required to make Phase 0 and Phase 1 real

It does **not** yet cover:

- `agent_dispatch`
- `hitl_request`
- `task`
- `subtask`
- `looper_run`
- artifact/doc versioning

Those belong to later phases already defined in
[IMPLEMENTATION-PLAN.md](IMPLEMENTATION-PLAN.md).

## 2. Migration Structure

Each database should have its own independent migration stream.

Recommended structure:

```text
migrations/
  runtime/
    20260321_001_schema_migrations.sql
    20260321_002_agent_runtime_core.sql
    20260321_003_agent_sessions_and_messages.sql
    20260321_004_schedule_runtime_core.sql
    20260321_005_trigger_runtime.sql
  compozy/
    20260321_001_schema_migrations.sql
    20260321_002_workflow_run_core.sql
    20260321_003_workflow_checkpoint.sql
    20260321_004_workflow_signal.sql
```

Design rules:

- migration numbering is per database, not global across both databases
- each migration should be small and monotonic
- each migration should be independently idempotent at the runner level
- schema ownership must stay aligned with [STORAGE-MODEL.md](STORAGE-MODEL.md)

## 3. Phase 0 Database Bootstrap

### `runtime.db`

#### `20260321_001_schema_migrations.sql`

Purpose:

- establish migration tracking for `runtime.db`

Minimum table intent:

- `schema_migration`

Representative columns:

- `version`
- `name`
- `applied_at`

#### `20260321_002_agent_runtime_core.sql`

Purpose:

- establish the minimum durable projection for loaded agent runtime state

Tables:

- `agent_runtime`

Minimum fields:

- `agent_id`
- `loaded`
- `state`
- `mode`
- `healthy`
- `active_session_id`
- `active_dispatches`
- `last_active_at`
- `updated_at`

#### `20260321_003_agent_sessions_and_messages.sql`

Purpose:

- establish the durable runtime surfaces already reflected in the public agent
  contract

Tables:

- `agent_session`
- `agent_message`

#### `20260321_004_schedule_runtime_core.sql`

Purpose:

- establish durable runtime state for typed schedules

Tables:

- `schedule_runtime`
- `schedule_execution`

#### `20260321_005_trigger_runtime.sql`

Purpose:

- establish durable runtime state for trigger fire tracking

Tables:

- `trigger_runtime`

Minimum fields:

- `trigger_id`
- `enabled`
- `fire_count`
- `last_fired_at`
- `loaded_at`
- `updated_at`

### `compozy.db`

#### `20260321_001_schema_migrations.sql`

Purpose:

- establish migration tracking for `compozy.db`

Minimum table intent:

- `schema_migration`

Representative columns:

- `version`
- `name`
- `applied_at`

## 4. Phase 0 Runtime Changes

### Boot Sequence

The startup sequence should become:

1. load config
2. resolve paths for `runtime.db` and `compozy.db`
3. open both databases
4. apply `runtime.db` migrations
5. apply `compozy.db` migrations
6. initialize repository/store layer for both ownership domains
7. continue normal kernel/runtime boot

### Required Components

Phase 0 should introduce:

- a database manager that knows about both databases
- a migration runner that can target each database independently
- repository boundaries that keep `runtime.db` and `compozy.db` separate in
  code

### Phase 0 Exit Criteria

- fork boots cleanly with both databases absent
- fork recreates both databases and applies migrations automatically
- fork detects already-applied migrations correctly
- runtime health can report migration status or failure clearly

## 5. Phase 1 `compozy.db` Migrations

### `20260321_002_workflow_run_core.sql`

Purpose:

- create the durable root record for workflow execution

Tables:

- `workflow_run`

Minimum fields:

- `run_id`
- `workflow_id`
- `workflow_version`
- `status`
- `input_json`
- `vars_json`
- `current_step_id`
- `waiting_kind`
- `waiting_ref`
- `active_dispatch_id`
- `active_hitl_request_id`
- `labels_json`
- `metadata_json`
- `error_json`
- `started_at`
- `updated_at`
- `completed_at`

### `20260321_003_workflow_checkpoint.sql`

Purpose:

- create the recovery and audit trail for workflow transitions

Tables:

- `workflow_checkpoint`

Minimum fields:

- `checkpoint_id`
- `run_id`
- `step_id`
- `kind`
- `data_json`
- `created_at`

Recommended minimum checkpoint kinds for Phase 1:

- `run_created`
- `run_started`
- `step_selected`
- `waiting_signal`
- `signal_received`
- `run_paused`
- `run_resumed`
- `run_completed`
- `run_failed`
- `shutdown_requested` — written during graceful shutdown before status transitions to `paused`

### `20260321_004_workflow_signal.sql`

Purpose:

- persist workflow-level signals independently of in-memory delivery

Tables:

- `workflow_signal`

Minimum fields:

- `signal_id`
- `run_id`
- `name`
- `payload_json`
- `source`
- `consumed`
- `created_at`
- `consumed_at`

## 6. Phase 1 Index Intent

The exact SQL is still open, but the first migration set should support these
queries efficiently:

### `workflow_run`

- by `workflow_id`
- by `status`
- by `updated_at`

### `workflow_checkpoint`

- by `run_id`, ordered by `created_at`

### `workflow_signal`

- by `run_id`
- by `run_id + consumed`
- by `run_id + name`

## 7. Phase 1 Runtime Changes

### Run Creation Path

Before meaningful workflow execution begins, the system should:

1. validate and compile the workflow definition
2. create `workflow_run`
3. write `run_created` checkpoint
4. move run to `running`
5. write `run_started` checkpoint
6. only then continue execution

This ensures a workflow never starts as a purely in-memory object.

### Transition Writer

Phase 1 should add one internal transition writer responsible for:

- mutating `workflow_run`
- appending `workflow_checkpoint`
- doing both in one database transaction when practical

This avoids scattering run-state semantics across unrelated call sites.

### Waiting For Signal

When a workflow enters a wait-for-signal state, Phase 1 should:

1. update `workflow_run.status = waiting_signal`
2. persist `waiting_kind` and `waiting_ref`
3. append a `waiting_signal` checkpoint

Signal delivery should:

1. insert `workflow_signal`
2. resolve the destination run
3. mark the signal consumed when accepted for progression
4. append `signal_received` checkpoint

### Read Surfaces

Phase 1 should make these read paths durable:

- list runs
- get run detail
- get run checkpoints
- submit signal to run

The first durable public win is observability and resumability, not rich edit
features.

## 8. Recovery Semantics For Phase 1

Phase 1 should keep recovery semantics intentionally conservative.

Recommended startup recovery policy:

- `pending` stays `pending`
- `waiting_signal` stays `waiting_signal`
- `completed` stays `completed`
- `failed` stays `failed`
- `cancelled` stays `cancelled`
- `running` is downgraded to `paused` and receives a recovery checkpoint

Recommended checkpoint kind:

- `run_recovered_needs_resume`

Rationale:

- Phase 1 has durable root state, but not yet the full dispatch/HITL model
- auto-resuming arbitrary in-flight execution is riskier than making restart
  state explicit
- later phases can tighten recovery once more execution detail is durable

## 9. Handler And Service Order

The lowest-risk order for code changes is:

1. dual-database bootstrap and migration runner
2. repository/store layer for `workflow_run`, `workflow_checkpoint`,
   `workflow_signal`
3. durable run creation path
4. durable run list/detail/checkpoint endpoints
5. waiting-signal persistence and signal ingestion
6. startup recovery scan for Phase 1 statuses

This keeps write-path correctness ahead of broader API coverage.

## 10. What Should Not Happen In Phase 0-1

Do **not** add these yet:

- `workflow_step_run`
- `agent_dispatch`
- `hitl_request`
- `task`
- `subtask`
- `looper_run`
- generalized background replay engine
- automatic resume of arbitrary mid-step compute

These belong to later phases and should not be pulled forward casually.

## 11. Exit Criteria

This initial slice is done when:

- both databases bootstrap and migrate cleanly
- workflow runs become durable records from creation onward
- checkpoints exist for major transitions
- workflow signals are persisted and queryable
- restart no longer loses run identity or waiting state
- the system can expose durable run list/detail/checkpoint surfaces through the
  control plane
