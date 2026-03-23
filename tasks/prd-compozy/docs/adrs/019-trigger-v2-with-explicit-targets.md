# ADR-019: Trigger v2 With Explicit Targets

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The OpenFang trigger engine remains the base, but the target side is extended with explicit dispatch targets instead of assuming every trigger wakes an agent.

Recommended target classes:

- `agent_message`
- `workflow_start`
- `workflow_signal`

The existing pattern-matching model remains the conceptual base.

## Rationale

- The current trigger model is strongly agent-centric.
- Compozy needs triggers that can start and signal durable workflow runs without creating a second event system.
- The current OpenFang trigger routes and files are too tied to `agent_id` to serve as the clean product surface for the fork.
- Extending targets is still the right conceptual move, but it should happen under a Compozy-owned public surface when the old contract gets in the way.

## Consequences

- The event system remains unified.
- Current agent-wakeup behavior remains valid as one target type among several.
- Trigger runtime must learn to dispatch to workflow-level actions as well as agents.
- Legacy OpenFang trigger definitions may remain importable, but they are not the primary target authoring model for the fork.
