# ADR-041: Bounded Layered Definition Validation

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Config-first definitions should use bounded layered validation.

The pipeline is:

1. schema validation
2. reference validation
3. semantic validation
4. normalization

This applies to the main definition families:

- agents
- workflows
- triggers
- schedules

## Rationale

- Parse-only validation is too weak and lets bad definitions fail late in the
  runtime.
- Fully interpretive validation would create too much complexity too early and
  would effectively become a second runtime.
- A layered model catches the important errors early without trying to predict
  every runtime outcome.

## Boundaries

Validation should not:

- boot providers
- call the network
- execute templates
- perform full dry-runs of workflows
- attempt whole-system symbolic execution

Validation should:

- reject malformed shapes
- reject bad references
- reject invalid cross-field combinations
- produce a normalized logical form suitable for compilation

## Consequences

- `validate` remains stronger than raw parsing but weaker than execution.
- `compile` assumes validated and normalized input.
- The exact payload envelopes remain in `API-SPEC.md`.
- Detailed resource-specific rules can evolve without changing the core model.
