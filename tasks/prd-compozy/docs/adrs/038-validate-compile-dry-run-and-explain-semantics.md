# ADR-038: Validate, Compile, Dry-Run, And Explain Semantics

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The control plane should distinguish four related but different behaviors:

- `validate`
- `compile`
- `dry-run`
- `explanation`

Meaning:

- `validate` checks whether a definition is acceptable
- `compile` returns the normalized and compiled internal form of a definition
- `dry-run` simulates a side-effecting operation without executing it
- `explanation` is a structured section inside `compile`, `dry-run`, `test`,
  or similar responses, not a universal standalone endpoint

Baseline dry-run surfaces:

- `POST /api/v1/agents/{id}/messages/dry-run`
- `POST /api/v1/workflows/{id}/runs/dry-run`
- `POST /api/v1/events/dry-run`
- `POST /api/v1/schedules/{id}/run-now/dry-run`

Trigger simulation remains centered on:

- `POST /api/v1/triggers/{id}/test`

## Rationale

- Machine-driven administration needs more than validation of static
  definitions.
- Side-effecting operations often need a simulation mode before execution.
- A dedicated `explain` endpoint for every resource would bloat the public
  surface and duplicate data already present in compile or dry-run results.

## Consequences

- The public API gets a consistent simulation model without turning every
  resource into a separate expert-system endpoint family.
- `compile` stays definition-oriented.
- `dry-run` stays operation-oriented.
- Detailed payloads should follow `API-SPEC.md`.
