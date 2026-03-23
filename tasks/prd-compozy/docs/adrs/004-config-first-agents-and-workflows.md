# ADR-004: Config-First Agents And Workflows

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Agents and workflows are created through configuration first, with TOML, CLI,
and API as the primary surfaces over the same conceptual model. UI is a later
client of that same model.

Rust is reserved for:

- new primitives
- new providers
- new integrations
- runtime internals

## Rationale

- The product should be customizable without requiring Rust development for each new agent or workflow.
- Config-first definitions make the platform more flexible and more aligned with OpenFang's composable nature.

## Consequences

- The system needs stable schemas for agent and workflow definitions.
- CLI, API, and later UI must edit the same logical objects as file-based
  configuration.
