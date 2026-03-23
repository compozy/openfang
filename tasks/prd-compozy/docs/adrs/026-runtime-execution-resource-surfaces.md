# ADR-026: Runtime Execution Resource Surfaces

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public product exposes these execution resources under `/api/v1`:

- `runs`
- `dispatches`
- `hitl-requests`
- `looper-runs`

The durable runtime also uses `workflow_signal` internally as a first-class execution object, even if it is not always promoted as a top-level user-facing resource.

Minimum semantic roles:

- `run` = durable execution of a workflow
- `dispatch` = delegated execution inside a run
- `hitl-request` = explicit human interaction tied to a run/dispatch
- `looper-run` = durable execution of the specialized looper

`looper-run` is created against an explicit `task_id` and should not be treated
as a free-floating runtime object with no domain anchor.

## Rationale

- These are the core durable product concepts missing from current OpenFang public surfaces.
- Without them, restart safety, observability, lineage, and HITL would remain hidden implementation details instead of product primitives.
- A real long-running product needs execution resources that are queryable and operable on their own.

## Consequences

- `/api/v1/runs`, `/api/v1/dispatches`, `/api/v1/hitl-requests`, and `/api/v1/looper-runs` become first-class public resources.
- HITL is modeled separately from the older approval subsystem.
- Workflow execution state becomes part of the product surface instead of a hidden in-memory detail.
- These resources should expose canonical list, detail, and action payloads in
  `API-SPEC.md`.
- The looper surface should align with the task/subtask domain model rather
  than generic queue semantics.
