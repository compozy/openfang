# ADR-007: Autonomous By Default, Explicit HITL Only

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Agents and workflows are autonomous by default. Interactive permission prompts are not part of the normal runtime model. HITL is the only intentional human pause, and it appears only when explicitly modeled.

## Rationale

- The product is meant to be an autonomous system, not an approval-driven shell.
- Permission prompts in the hot path would make the platform rigid and misaligned with OpenFang.
- HITL should exist for product semantics, not as a defensive runtime crutch.

## Consequences

- Tool execution remains open by default.
- HITL design becomes a first-class primitive of the domain, not an ad hoc runtime behavior.
