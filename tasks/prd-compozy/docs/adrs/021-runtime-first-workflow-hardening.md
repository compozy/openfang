# ADR-021: Runtime-First Workflow Hardening

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The first major workflow refactor is runtime-first.

The initial focus is on durable run state and resumability:

- `workflow_run`
- `workflow_checkpoint`
- `agent_dispatch`
- `hitl_request`
- `workflow_signal`
- `looper_run`

Public workflow-schema implementation and authoring sugar should follow after the runtime can safely support them. The target public contract can still be designed up front.

## Rationale

- The largest real gap in the current OpenFang workflow implementation is runtime durability, not authoring syntax.
- Rich new schema fields are low value if the run model still loses state on restart.
- A runtime-first order preserves the approved feature set while reducing early schema churn and migration mistakes.

## Consequences

- Early implementation work should prioritize state models, transitions, recovery, and dispatch/HITL handling.
- Public workflow-schema changes should be justified by real runtime support, not invented ahead of it.
- The fork can still reach the planned workflow v2 model, but through a safer implementation order.
