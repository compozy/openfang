# ADR-024: Workflow v2 Public Schema

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public workflow contract is designed as the target product model, not as a temporary subset of the current OpenFang workflow JSON.

Top-level workflow fields:

- `id`
- `name`
- `version`
- `description`
- `enabled`
- `tags`
- `input`
- `output`
- `defaults`
- `steps`
- `outputs`

Step structure:

- `id`
- `name`
- `kind`
- `uses`
- `with`
- `save_as`
- `flow`
- `runtime`

Supported step kinds:

- `agent`
- `primitive`
- `workflow`
- `wait_signal`
- `start_looper`
- `emit_event`
- `collect`
- `noop`

Supported flow modes:

- `sequential`
- `fan_out`
- `conditional`
- `loop`

## Rationale

- The product already knows the correct semantic surface it wants.
- The public contract should reflect that target model instead of inheriting the current OpenFang route payload shape.
- OpenFang still remains the conceptual base through steps, control-flow ideas, and execution internals.

## Consequences

- The workflow surface is more structured than the current OpenFang public API.
- `input` and `output` use the shared lightweight definition contract schema.
- `outputs` remains the projection layer that builds the final result shape
  described by `output`.
- `uses`, `with`, `flow`, and `runtime` become explicit public concepts.
- Runtime implementation may arrive in phases, but the public schema should already reflect the correct product shape.
