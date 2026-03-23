# ADR-027: Provider-Specific Agent Configuration

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public Compozy `agent_definition` must expose provider configuration in a
structured way that mirrors the model from the internal Arky crates:

- `provider.driver`
- `provider.model`
- `provider.profile`
- `provider.defaults`
- `provider.config`
- optional `provider.request_extra`

`provider.defaults` is reserved for small cross-provider request defaults.
`provider.config` is a typed block whose shape depends on the chosen provider.
`provider.request_extra` is a constrained request-level escape hatch, not a
general infrastructure bag.

Provider configuration is split across three layers:

- installation or workspace configuration
- reusable named profiles
- per-agent provider configuration

The product should not reduce provider configuration to only
`driver/model/profile`, and should not use one raw untyped map as the main
provider contract.

## Rationale

- The internal Arky crates provide strongly typed provider runtime configuration
  per provider for Claude Code, Codex, and Claude-compatible wrappers.
- They also keep a smaller generic per-request settings envelope for portable
  overrides.
- That split is a better match for Compozy than either extreme:
  - one fully generic provider block
  - or every provider knob flattened into the top level of the agent schema
- Compozy uses the internal Arky crates for Claude Code and Codex, so the public
  agent schema needs enough structure to express meaningful provider-specific
  behavior cleanly.

## Consequences

- `agent_definition` remains product-first, but compiles to `AgentManifest`
  plus provider-specific runtime metadata.
- Provider-specific settings remain namespaced and typed instead of leaking as
  generic top-level fields.
- Cross-provider defaults stay intentionally small and conservative.
- Low-level installation and runtime plumbing should live in installation or
  workspace config, not in every agent document.
- Profiles become the reusable middle layer for model choice and safe provider
  behavior presets.
- `provider.request_extra` may exist, but only for bounded request-level
  overrides and not for credentials, environment variables, bootstrap, or
  transport concerns.
