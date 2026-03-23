# ADR-014: AgentManifest As The Base Agent Schema

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The OpenFang `AgentManifest` remains the base schema for agent definitions in the Compozy fork.

Compozy may extend or wrap it, but should not introduce a parallel agent-definition system by default.

## Rationale

- `AgentManifest` already covers a large part of the desired config-first surface: model selection, schedules, capabilities, tools, skills, routing, autonomy, workspace, and runtime-related settings.
- Replacing it too early would duplicate a mature part of the platform without solving the main architectural gaps.

## Consequences

- Agent UX in TOML, CLI, API, and later UI should compile to or remain
  compatible with `AgentManifest`.
- New product-level fields should be added carefully, ideally as compatible extensions or product-level sugar.
- `AgentManifest` as the base does not require Compozy to expose the raw
  OpenFang-shaped surface directly.
- Provider configuration can be product-shaped when that helps express the real
  model backed by the internal Arky crates, as long as it still compiles into the underlying agent and
  provider runtime structures.
- Product-facing metadata such as `group`, `tags`, `input`, and `output` may
  live above the raw manifest shape when they improve product UX and
  organization.
