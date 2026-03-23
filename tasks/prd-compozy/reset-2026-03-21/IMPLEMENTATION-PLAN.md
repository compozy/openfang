# Compozy Durable Runtime Implementation Plan

**Status:** Current implementation baseline
**Date:** 2026-03-21

This document translates the reset architecture into an implementation order.

It does not redefine the target product contract. It defines the lowest-risk
path to reach that contract.

The detailed first slice for Phase 0 and Phase 1 lives in
[INITIAL-RUNTIME-MIGRATIONS.md](INITIAL-RUNTIME-MIGRATIONS.md).

## 1. Goals

The implementation plan should:

- preserve the accepted public model in [API-SPEC.md](API-SPEC.md)
- follow the ownership split in [STORAGE-MODEL.md](STORAGE-MODEL.md)
- realize the schema outline in [DATABASE-SCHEMA.md](DATABASE-SCHEMA.md)
- harden the current OpenFang fork without rewriting everything at once

## 2. Delivery Principles

### Runtime First

The first meaningful delivery is durable workflow state, not authoring sugar.

That means the earliest phases should prioritize:

- `workflow_run`
- `workflow_checkpoint`
- `workflow_signal`
- `agent_dispatch`
- `hitl_request`

before richer workflow UX is treated as done.

### Domain Before Convenience

`task`, `subtask`, and `looper_run` are product concepts and should land as
their own durable records instead of being simulated through the old OpenFang
task queue.

### File-Backed Definitions Stay Canonical

Definitions remain file-backed throughout delivery.

The new database tables are for:

- durable execution
- product domain state
- projections
- operational metadata

They are not a second source of truth for definitions.

### Avoid Early Table Explosion

The first cut should not introduce every possible helper table.

In particular, the initial runtime should avoid requiring:

- `workflow_step_run`
- general event sourcing infrastructure
- dedicated symbol tables
- excessive join tables for refs that can start as bounded JSON payloads

These can be added later if the simpler model proves insufficient.

## 3. Dependency Graph

The durable model has a natural dependency order:

1. `workflow_run`
2. `workflow_checkpoint`
3. `workflow_signal`
4. `agent_dispatch`
5. `hitl_request`
6. `task`
7. `subtask`
8. `looper_run`
9. `looper_subtask`

Interpretation:

- `workflow_run` is the root execution record
- checkpoints, signals, dispatches, and HITL depend on runs
- `task` can optionally point back to `workflow_run`
- `subtask` depends on `task`
- `looper_run` depends on `task` and may optionally point to `workflow_run`
- `looper_subtask` depends on both `looper_run` and `subtask`

## 4. Phase Plan

### Phase 0: Database And Migration Bootstrap

Objective:

- make `runtime.db` and `compozy.db` first-class migration targets

Minimum work:

- migration table or equivalent versioning metadata in both databases
- startup path that opens both databases predictably
- migration runner that can apply ordered migrations independently per database

Exit criteria:

- fork can boot with both databases present
- migrations are deterministic and idempotent

### Phase 1: Durable Workflow Core

Objective:

- survive restart without losing workflow identity or basic progression

First-cut tables in `compozy.db`:

- `workflow_run`
- `workflow_checkpoint`
- `workflow_signal`

Minimum runtime work:

- persist run creation before execution begins
- persist run status transitions
- persist current step identity and waiting state
- persist signals that resume waiting runs
- record coarse checkpoints at important transitions

Recommended checkpoint policy for the first cut:

- event-like rows for major transitions
- no separate snapshot table yet
- optional coarse snapshot payload embedded in selected checkpoint rows if
  needed later

Exit criteria:

- a workflow run can be listed and inspected durably
- a waiting run can survive restart and still be resumable
- a completed or failed run remains inspectable after restart

### Phase 2: Durable Delegation And HITL

Objective:

- make agent execution and in-step human pauses recoverable

First-cut tables in `compozy.db`:

- `agent_dispatch`
- `hitl_request`

Minimum runtime work:

- create dispatch records before sending meaningful work to an agent
- update dispatch lifecycle durably
- create HITL requests as records tied to run, step, and dispatch
- resume the same dispatch after HITL response

Exit criteria:

- a dispatch can be recovered after restart
- HITL mid-step can pause and resume without losing the run
- run detail can show active dispatch and active HITL request

### Phase 3: Task And Subtask Domain

Objective:

- land the durable product work model outside the workflow runtime tables

First-cut tables in `compozy.db`:

- `task`
- `subtask`

Minimum domain work:

- create tasks directly through the control plane
- create subtasks directly through the control plane
- allow tasks to anchor linked artifacts, docs, files, repositories, and labels
- allow subtasks to carry local execution input, dependencies, and result
- implement explicit `replan` against the task/subtask model

Implementation note:

- refs such as `artifact_refs_json`, `doc_refs_json`, and `file_refs_json` can
  remain bounded JSON in the first cut
- this avoids blocking the phase on join-table design

Exit criteria:

- `tasks` and `subtasks` are queryable and mutable independently of workflows
- replanning changes subtask structure without replacing task identity

### Phase 4: Looper On Top Of Tasks

Objective:

- make the looper a durable executor over subtasks, not a hidden queue loop

First-cut tables in `compozy.db`:

- `looper_run`
- `looper_subtask`

Minimum runtime work:

- create looper runs through `POST /api/v1/looper-runs`
- require explicit `task_id`
- persist execution policy with the looper run
- track subtask-level execution view per looper run
- support pause, resume, and cancel at looper-run level

Execution rules for the first cut:

- looper reads subtasks from the canonical `subtask` table
- looper policy defines the concurrency envelope
- subtasks may narrow execution through `depends_on` and `parallelizable`

Exit criteria:

- sequential and bounded-parallel looper runs are durable
- looper state and subtask progress survive restart

### Phase 5: Product-Domain Enrichment

Objective:

- round out domain objects without blocking the durable core

Candidate tables or enrichments:

- `artifact`
- `artifact_version`
- `doc`
- `doc_version`
- stronger file/ref normalization if needed
- richer indices and query surfaces

This phase is intentionally after the durable core because the runtime and
control-plane value does not depend on fully normalized artifact/doc storage on
day one.

Exit criteria:

- artifact/doc identity and versioning are durable enough for first-party SDLC
  flows

## 5. Minimal First Shipping Slice

If the fork needs a sharply bounded first slice, the minimum useful set is:

- `workflow_run`
- `workflow_checkpoint`
- `workflow_signal`
- `agent_dispatch`
- `hitl_request`
- `task`
- `subtask`
- `looper_run`
- `looper_subtask`

This slice is enough to deliver:

- durable workflow runs
- durable delegation
- in-step HITL
- task/subtask domain surfaces
- looper execution over subtasks

Artifact and document version tables can trail slightly behind this slice if
task refs remain bounded and the initial flows do not require deep artifact
history yet.

## 6. Initial Index Strategy

The exact SQL design remains open, but the first migration set should at least
plan for these lookup patterns:

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

### `agent_dispatch`

- by `run_id`
- by `status`
- by `parent_dispatch_id`

### `hitl_request`

- by `run_id`
- by `dispatch_id`
- by `status`

### `task`

- by `status`
- by `priority`
- by `source_run_id`
- by `updated_at`

### `subtask`

- by `task_id + position`
- by `task_id + status`
- by `assignee_kind + assignee_ref`

### `looper_run`

- by `task_id`
- by `source_run_id`
- by `status`

### `looper_subtask`

- by `looper_run_id + status`
- by `subtask_id`

## 7. What Stays Deferred

The first durable implementation should explicitly defer:

- `workflow_step_run` as a mandatory first-class table
- full event sourcing
- deep replay of every intermediate symbol mutation
- fully normalized refs for every linked domain object
- aggressive upstream sync work

These may become necessary later, but they should not block the first durable
cut.

## 8. Relationship To Open Questions

This plan resolves the architectural open question about the lowest-risk
migration path from the current in-memory runtime to a durable one.

It does **not** yet resolve:

- exact SQL types
- exact index implementation
- retention policies
- artifact/doc versioning details
- upstreaming strategy

Those remain implementation and maintenance questions, not architecture
blockers.
