# ADR-017: Workflow v2 As Minimal Evolution Of OpenFang

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The Compozy fork introduces a `workflow v2` by evolving the current OpenFang workflow model instead of replacing it with a separate orchestration language.

The fork keeps the existing OpenFang workflow mental model as the base and adds:

- durable run state
- an internal workflow IR
- explicit step kinds
- separation between step action and step mode

Minimum step kinds:

- `agent`
- `primitive`
- `workflow`
- `wait_signal`
- `start_looper`
- `emit_event`
- `collect`
- `noop`

Minimum modes:

- `sequential`
- `fan_out`
- `conditional`
- `loop`

## Rationale

- The current OpenFang workflow model already has the right high-level shape.
- The main gaps are runtime durability and action semantics, not the existence of the workflow concept itself.
- Replacing the model completely would create avoidable drift and another mental model for users.
- The current OpenFang workflow files, routes, and editors are not sufficient as the primary public contract for the new product surface.

## Consequences

- Existing OpenFang-style workflow definitions remain the conceptual starting point, not the required public contract.
- The fork needs a compilation step from user-facing definition format to an internal IR.
- Workflow runtime refactoring remains a foundational effort, but without inventing a second workflow center.
- Runtime durability should be delivered before richer public schema sugar depends on it.
- Legacy OpenFang workflow definitions may be supported as import paths, not as the main workflow v2 surface.
- `collect` is treated as an explicit step action in the public contract, while flow modes continue to describe control behavior such as sequencing, fan-out, conditions, and loops.
