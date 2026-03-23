# ADR-018: HITL Can Occur Inside Active Agent Steps

**Status:** Accepted
**Date:** 2026-03-21

## Decision

HITL in Compozy is not limited to workflow-level pauses. A workflow step of kind `agent` may enter one or more HITL interactions while the same step remains active.

The runtime therefore supports:

- workflow-level waiting via explicit wait steps
- in-step HITL via durable interaction records linked to the active `agent_dispatch`

## Rationale

- Compozy needs clarification-style interactions during execution, not only approval points between phases.
- Modeling all HITL as a workflow-level `wait_signal` would lose important product behavior.

## Consequences

- `agent_dispatch` must be interruptible and resumable.
- `hitl_request` must link to the workflow run, the logical step identity, and the active dispatch.
- If a first-class `workflow_step_run` object is introduced later, `hitl_request` may link to it as well.
- The runtime may need multi-turn interaction threads for clarification loops within one step.
