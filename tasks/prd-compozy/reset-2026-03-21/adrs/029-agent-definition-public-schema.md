# ADR-029: Agent Definition Public Schema

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public Compozy `agent_definition` should use this high-level shape:

- top-level fields:
  - `id`
  - `name`
  - `version`
  - `description`
  - `enabled`
  - `group`
  - `tags`
- main blocks:
  - `provider`
  - `prompt`
  - `capabilities`
  - `runtime`
  - `input`
  - `output`

`triggers` and `schedules` are not embedded in the agent schema.

`input` and `output` use the shared lightweight definition contract schema.

The public agent schema compiles into:

- `AgentManifest`
- `ProviderBinding`
- `AgentProductMetadata`

## Rationale

- The product needs one clean public contract for agents.
- `AgentManifest` remains a useful internal base, but it is not the whole
  product contract.
- Provider configuration from the internal Arky crates needs a typed provider block.
- Product metadata such as `group`, `tags`, `input`, and `output` should not be
  forced into the raw runtime agent shape.
- Keeping triggers and schedules separate preserves a cleaner system model.

## Consequences

- Agent definitions stay product-first while still compiling into the OpenFang
  and the internal Arky provider runtime layers.
- UI, API, and TOML can share the same logical contract.
- The product avoids inventing a second agent execution model while still
  exposing a more usable schema than the raw underlying manifest.
