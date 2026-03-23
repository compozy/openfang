# ADR-045: Task And Subtask Domain Model

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy uses `task` as the main durable work object of the product and
`subtask` as the executable child unit inside that task.

This replaces the old Compozy hierarchy where:

- `issue` was the root planning object
- nested `tasks` represented executable work items

In the new model:

- the old `issue` concept becomes `task`
- the old nested `task` concept becomes `subtask`

`task` owns:

- objective and planning context
- durable identity across replanning
- linked artifacts, docs, files, repositories, and labels when relevant

`subtask` owns:

- concrete executable work
- assignee or executor targeting
- dependency information
- execution result

The old OpenFang shared task queue is not the canonical product model. At most,
it survives as a legacy adapter or runtime mechanism where still useful.

The public names remain `task` and `subtask`. Naming collisions with legacy
OpenFang internals should be handled by internal isolation, not by renaming the
Compozy domain model.

## Rationale

- The product already needs a richer planning root than a simple queue item.
- The looper and durable workflow runtime need executable child units that can
  be retried, blocked, reordered, and replayed without replacing the root task
  identity.
- Preserving the old `issue` and nested `task` naming would keep unnecessary
  legacy semantics in the new product.

## Consequences

- `task` becomes the primary durable product object.
- `subtask` becomes the unit the looper executes.
- The schema outline should use `task` and `subtask` as first-class domain
  tables.
- Existing OpenFang task-queue concepts should not constrain the new domain
  model.
- Public naming should stay aligned with `task/subtask` even if legacy
  OpenFang internals keep older queue terminology.
