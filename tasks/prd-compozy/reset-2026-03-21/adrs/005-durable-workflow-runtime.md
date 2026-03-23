# ADR-005: Durable Workflow Runtime In The OpenFang Fork

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The OpenFang workflow runtime is hardened to support durable workflow runs. Compozy does not introduce a second primary orchestration engine.

The first durable cut centers on these persisted objects:

- `workflow_run`
- `workflow_checkpoint`
- `agent_dispatch`
- `hitl_request`
- `workflow_signal`
- `looper_run`

`workflow_step_run` and richer run-history tables remain valid later additions, but they are not required to begin the durable runtime refactor.

## Rationale

- The current in-memory run model is not enough for long-running, restart-safe product workflows.
- Keeping one workflow center preserves a coherent product model for user-defined and first-party workflows.

## Consequences

- Workflow persistence becomes a foundational refactor in the fork.
- Triggers and schedulers wake runs up, but durable run state becomes the real source of continuity.
- Runtime-state hardening takes priority over making the workflow schema more expressive on day one.
