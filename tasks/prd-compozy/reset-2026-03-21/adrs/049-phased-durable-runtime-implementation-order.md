# ADR-049: Phased Durable Runtime Implementation Order

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The durable runtime lands in phases instead of one large rewrite.

The implementation order is:

1. database and migration bootstrap
2. durable workflow core
3. durable delegation and HITL
4. task and subtask domain
5. looper on top of tasks
6. product-domain enrichment such as artifact/doc versioning

The minimum useful durable slice is:

- `workflow_run`
- `workflow_checkpoint`
- `workflow_signal`
- `agent_dispatch`
- `hitl_request`
- `task`
- `subtask`
- `looper_run`
- `looper_subtask`

## Rationale

- The fork needs durable execution before it needs more authoring complexity.
- `task` and `subtask` should be real domain objects before the looper becomes
  a product-level executor over them.
- A phased order keeps the fork close enough to current OpenFang internals to
  move steadily without turning the migration into a second platform rewrite.

## Consequences

- Migration work should start with dual-database bootstrap and `compozy.db`
  runtime tables, not with UI or schema sugar.
- `workflow_step_run` stays deferred until the simpler runtime model proves
  insufficient.
- Artifact and doc versioning can trail the first durable slice if task-linked
  refs remain bounded in the early implementation.
- The detailed phase plan should live in `IMPLEMENTATION-PLAN.md`.
