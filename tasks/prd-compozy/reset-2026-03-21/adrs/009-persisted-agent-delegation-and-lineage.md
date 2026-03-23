# ADR-009: Persisted Agent Delegation And Lineage

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Agent delegation remains a native platform capability, but workflow-relevant delegations are recorded durably.

Core semantics:

- `agent.call`
- `agent.send`
- `agent.spawn`

These operations create persisted dispatch records when they participate in important runs.

## Rationale

- OpenFang's agent network and delegation model is one of the platform's strengths.
- Durable workflows cannot rely on invisible runtime side effects if they need restart safety and observability.

## Consequences

- The system needs `agent_dispatch` and lineage-aware run tracking.
- Delegation stays flexible, but workflows gain recovery and traceability.
