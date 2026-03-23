## markdown

## status: pending

<task_context>
<domain>domain/looper/runtime</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task28</dependencies>
</task_context>

# Task 34.0: Looper Durable Schema And Runtime

## Overview

Implement the durable looper model in `compozy.db` and the looper runtime
executor. The looper is a specialized executor for iterative subtask work
(ADR-011): it selects subtasks from a `task`, dispatches them according to
an explicit execution policy (ADR-046), observes results, and continues or
replans. It is not a second orchestration engine — it runs on top of the same
durable foundation as workflow runs and reuses `agent_dispatch` records for
execution lineage. The `looper_run` table anchors each looper execution to a
`task_id`, not to a generic queue entry.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add a `looper_run` table to `compozy.db` with the full approved column set
  from DATABASE-SCHEMA.md: `looper_run_id`, `task_id` (FK to `task`),
  `source_run_id`, `status`, `execution_policy_json`, `current_subtask_id`,
  `progress_json`, `error_json`, `started_at`, `updated_at`, `completed_at`.
  Status values are at minimum: `pending`, `running`, `paused`, `completed`,
  `failed`, `cancelled`.
- Add a `looper_subtask` table with the full approved column set:
  `looper_subtask_id`, `looper_run_id` (FK to `looper_run`), `subtask_id`
  (FK to `subtask`), `status`, `dispatch_id`, `result_json`, `error_json`,
  `updated_at`. This table is the per-subtask execution view for a given
  looper run and must not duplicate canonical `subtask` state.
- Implement a `LooperRunRepository` covering: `create`, `find_by_id`, `list`
  (with filters: `task_id`, `source_run_id`, `status`, `execution_mode` per
  API-SPEC.md section 13), `update_status`, `update_progress`,
  `set_current_subtask`, `pause`, `resume`, `cancel`. Repository errors must
  use `thiserror` enums.
- Implement a `LooperSubtaskRepository` covering: `create_for_run`,
  `find_by_looper_run`, `update_status`, `set_dispatch`. These records track
  the execution view only; canonical subtask state lives in the `subtask` table
  from task 28.
- Implement a `LooperRuntime` (or equivalent executor type) that drives
  subtask execution according to the stored `execution_policy_json`. The
  runtime must implement both `sequential` and `parallel` modes. In
  `sequential` mode, one subtask runs at a time. In `parallel` mode, at most
  `max_parallelism` subtasks run concurrently. Subtask `depends_on` constraints
  and `parallelizable = false` always narrow the effective concurrency, even
  under a `parallel` policy. Subtasks must never widen the concurrency envelope
  beyond what the looper policy permits (ADR-046).
- The looper runtime must not infer sequencing or concurrency implicitly.
  The `execution_policy_json` — with `mode`, `max_parallelism`, and
  `selection` fields — must be present and explicit on every `looper_run`
  record. A missing or malformed policy is a hard error at run-creation time,
  not a silent default.
- Looper execution must be durable across restart. After the daemon restarts,
  any `looper_run` in `running` or `paused` state must be recoverable: the
  runtime must be able to resume from the last committed `looper_subtask`
  state without re-executing already-completed subtasks.
</requirements>

## Subtasks

- [ ] 34.1 Write `compozy.db` migrations for `looper_run` and `looper_subtask`.
      Include all approved columns, FK constraints (`looper_run.task_id` → `task`,
      `looper_subtask.looper_run_id` → `looper_run`, `looper_subtask.subtask_id`
      → `subtask`), and indexes on `looper_run.task_id`, `looper_run.status`,
      `looper_subtask.looper_run_id`, and `looper_subtask.status`. Migration files
      go in `migrations/compozy/` and must continue the existing numbering sequence.
- [ ] 34.2 Implement `LooperRunRepository` and `LooperSubtaskRepository` with
      the operations listed in requirements. Follow the shared-connection pattern
      from `crates/openfang-memory/src/structured.rs`. Validate
      `execution_policy_json` shape on `create` — reject unknown `mode` values and
      `max_parallelism < 1` with a domain error, not a panic.
- [ ] 34.3 Implement `LooperRuntime` executor. The runtime must: (a) load the
      looper run and its policy from the repository; (b) load the target subtask
      list from `SubtaskRepository` (task 28), applying the `selection` strategy
      (`priority` ordering is the default); (c) drive execution in `sequential` or
      `parallel` mode while respecting `depends_on` and `parallelizable` subtask
      fields; (d) write `looper_subtask` execution records and update
      `looper_run.progress_json` and `looper_run.current_subtask_id` as work
      advances; (e) transition the looper run to `completed` or `failed` on
      terminal outcomes.
- [ ] 34.4 Implement restart recovery: on daemon boot, the kernel (or a
      dedicated recovery step) must scan `looper_run` for runs in `running` status
      and re-attach the `LooperRuntime` to them, resuming from the last committed
      `looper_subtask` state. Completed subtasks must not be re-dispatched.
- [ ] 34.5 Implement `pause` and `resume` transitions on `LooperRuntime`. Pausing
      must stop accepting new subtask dispatches while allowing in-flight dispatches
      to complete. Resuming must restart subtask selection from where the run left
      off.
- [ ] 34.6 Write unit and integration tests as detailed in the Tests section.
- [ ] 34.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass with zero warnings before marking done.

## Implementation Details

The looper is not the workflow engine and not a general queue (ADR-011,
DESIGN.md section 12). It is a specialized executor whose only purpose is to
drive iterative work over the subtasks of a given `task`. It reuses the durable
foundation: `looper_run` is a peer of `workflow_run`, and looper subtask
dispatches produce `agent_dispatch` records for lineage tracing.

The `execution_policy_json` stored on `looper_run` must serialize the full
policy shape from ADR-046 and API-SPEC.md section 13:

```json
{
  "mode": "parallel",
  "max_parallelism": 4,
  "selection": "priority"
}
```

The `selection` field controls subtask ordering. `priority` uses `subtask.position`
and any priority metadata. Future selection strategies (`depends_first`,
`random`) may be added but `priority` must be the supported default in this task.

The `progress_json` field on `looper_run` must track at minimum:

```json
{
  "total": 12,
  "completed": 3,
  "failed": 1
}
```

This is the source of truth for the progress shape returned by API-SPEC.md
section 13.

The concurrency enforcement rule from ADR-046 must be encoded as a hard
invariant in the runtime, not as a soft convention. The runtime must track
the count of in-flight subtask dispatches and must not start a new dispatch
when the count equals `max_parallelism`. Subtask `parallelizable = false`
means the looper must finish all currently in-flight dispatches before starting
that subtask, even in `parallel` mode.

The `depends_on` resolution must not call `SubtaskRepository::find_by_id` in a
loop per subtask. Load the full subtask list for the task once, then resolve
the dependency graph in memory. This avoids N+1 query patterns on looper start.

For restart recovery, the runtime checks `looper_subtask.status` on reload:
any entry in `completed` or `failed` is already settled and must not be
re-dispatched. Any entry in `running` was interrupted and must be retried (or
marked `failed` based on the policy). Entries in `pending` are not yet started
and are eligible for dispatch.

The `LooperRuntime` must use `tokio::sync::Semaphore` to enforce
`max_parallelism` under the `parallel` policy. Never hold the semaphore permit
across an `.await` on the database write; acquire for dispatch, release on
completion callback.

### Relevant Files

- `crates/openfang-memory/src/structured.rs` — shared-connection repository pattern
- `crates/openfang-memory/src/substrate.rs` — connection initialization
- `crates/openfang-kernel/src/kernel.rs` — where recovery boot hook should be registered
- `crates/openfang-runtime/src/agent_loop.rs` — dispatch execution pattern to reuse
- `crates/openfang-types/src/agent.rs` — newtype patterns
- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — `looper_run` and `looper_subtask` columns
- `tasks/prd-compozy/docs/API-SPEC.md` section 13 — looper run shapes and endpoints
- `tasks/prd-compozy/docs/adrs/011-looper-as-specialized-executor.md`
- `tasks/prd-compozy/docs/adrs/046-explicit-looper-execution-policy.md`
- `tasks/prd-compozy/docs/DESIGN.md` sections 12, 23 — looper model and runtime surfaces
- `migrations/compozy/` — migration sequence to extend

### Dependent Files

- looper control-plane API handlers (task 39) — consumes `LooperRunRepository`
- E2E integration test (task 43) — exercises full looper flow

## Deliverables

- `migrations/compozy/XXXX_looper_run_subtask.sql` (or equivalent numbered file)
- `LooperRunRepository` and `LooperSubtaskRepository`
- `LooperRuntime` executor with sequential and parallel modes
- Restart recovery hook in the kernel boot sequence
- Pause and resume transitions
- Full test suite as described below

## Tests

### Unit Tests (Required)

- [ ] Creating a `looper_run` with `mode = "sequential"` persists the full
      `execution_policy_json` and returns it unchanged on `find_by_id`.
- [ ] Creating a `looper_run` with a missing `mode` field in `execution_policy_json`
      returns a domain error at creation time, not a panic.
- [ ] Creating a `looper_run` with `max_parallelism = 0` returns a domain validation
      error.
- [ ] `LooperRuntime` in `sequential` mode dispatches subtasks one at a time;
      after each dispatch completes, the next subtask is selected. Confirm no more
      than one `looper_subtask` record has status `running` at any point.
- [ ] `LooperRuntime` in `parallel` mode with `max_parallelism = 2` dispatches
      at most 2 subtasks concurrently. A third subtask does not start until one of
      the first two completes.
- [ ] A subtask with `parallelizable = false` causes the parallel runtime to wait
      for all in-flight dispatches to complete before starting that subtask, even
      if the semaphore has capacity.
- [ ] `depends_on` enforcement: a subtask whose dependency has not yet reached
      `completed` status is skipped by the subtask selector and becomes eligible
      only after the dependency settles.

### Integration Tests (Required)

- [ ] A `looper_run` created against a `task` with five subtasks advances through
      all five in `sequential` mode, setting `looper_run.status = completed` and
      `progress.completed = 5` at the end.
- [ ] Simulating a daemon restart mid-looper-run: the looper run record is in
      `running` state, three subtasks are `completed`, two are `pending`. After
      recovery, the runtime resumes from the two pending subtasks and does not
      re-execute the completed ones.
- [ ] `pause` transitions the looper run to `paused` without interrupting
      already-dispatched subtasks; `resume` restarts selection from the remaining
      pending subtasks.
- [ ] `cancel` transitions the looper run to `cancelled` and stops all further
      subtask selection. In-flight dispatches may complete, but no new ones are
      started.
- [ ] Parallel execution with `max_parallelism = 3` and 10 subtasks (none with
      `depends_on`): all 10 complete, the run finishes with `completed`, and the
      `progress_json` reflects `total = 10, completed = 10`.

### Regression and Anti-Pattern Guards

- [ ] Do not treat the old OpenFang task queue as the looper backend. The looper
      must use `looper_run` and `looper_subtask` records in `compozy.db`, not any
      legacy queue table.
- [ ] Do not infer concurrency mode when `execution_policy_json` is absent or
      malformed. Fail hard at creation time.
- [ ] Do not let subtasks widen the looper policy: a subtask that is individually
      `parallelizable = true` must not cause more than `max_parallelism` concurrent
      dispatches under any circumstances.
- [ ] Do not hold a `Mutex` lock across an async dispatch call; follow the
      project's `Never hold locks across .await points` rule from `CLAUDE.md`.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `looper_run` and `looper_subtask` tables exist in `compozy.db` with every
  column from DATABASE-SCHEMA.md and correct FK constraints.
- `LooperRunRepository` supports full lifecycle: create, find, list, status
  transitions (pause, resume, cancel, complete, fail).
- `LooperRuntime` correctly enforces `sequential` and `parallel` modes,
  respects `depends_on` and `parallelizable` subtask constraints, and never
  exceeds `max_parallelism`.
- Restart recovery correctly identifies interrupted looper runs and resumes
  without re-executing completed subtasks.
- The execution policy is always explicit — no looper run can be created or
  resume without a valid stored policy.
- `cargo fmt --all`, `cargo clippy`, and `cargo test --workspace` all pass at
  zero warnings and zero failures.

---

## Prior Implementation Reference

The old TypeScript codebase has an actor-based looper engine:

- `~/Dev/compozy/compozy-code/packages/tauri/src-node/looper/` — Actor-based looper with:
  - `actors/job-manager-actor.ts` — Job scheduling and management
  - `actors/task-stream-actor.ts` — Task streaming and execution
  - `actors/execution-control-actor.ts` — Execution control and policies
  - `core/runtime-service.ts` — Runtime service orchestration
  - `sqlite/` — Local SQLite persistence for looper state

The old looper is a Node.js actor-based engine. The new looper is a durable Rust executor on top
of the same workflow foundation. The old code shows execution policies, parallelism control, and
how subtask dependencies were evaluated at runtime.

## Notes

- This task is the core execution layer for subtasks.
