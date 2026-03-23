# ADR-043: Provider Layering And Constrained Request Extra

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Provider configuration is split into three layers:

- installation or workspace configuration
- reusable named provider profiles
- per-agent provider configuration

The classification rule is:

- agent-level config changes the behavior of a specific agent
- installation or workspace config changes how a provider is installed,
  authenticated, connected, or executed on this machine
- profiles are reusable behavior presets between those two layers

Per-agent configuration may include:

- `provider.driver`
- `provider.model`
- `provider.profile`
- small portable request defaults such as `max_tokens` and
  `reasoning_effort`
- typed provider behavior settings such as allowed tools, conversation or
  session behavior, fallback model choice, and agent-local budget caps

Installation or workspace configuration owns infrastructure concerns such as:

- credentials and credential wiring
- binary paths
- raw environment maps
- transport setup
- app-server bootstrap and lifecycle plumbing
- cache and runtime directories
- client identity and low-level timeout tuning

`provider.request_extra` may exist, but only as a constrained request-level
escape hatch.

It must not be used for:

- credentials
- raw environment variables
- provider bootstrap
- transport plumbing
- process lifecycle tuning
- other installation or workspace infrastructure concerns

## Rationale

- Compozy needs enough agent-level power for CLI, API, and internal agents to
  manage a living system without forcing every change through global config.
- Letting every provider and infrastructure knob live in `agent_definition`
  would make agent documents noisy, repetitive, and non-portable.
- Profiles provide the reusable middle layer that avoids duplication while
  preserving agent-level override power where it actually matters.
- A constrained `provider.request_extra` is safer than either forbidding all
  escape hatches or allowing an unbounded JSON bag to undermine validation.

## Consequences

- Agent definitions stay behavior-oriented rather than machine-oriented.
- Installation and workspace documents own infrastructure and secrets.
- Validation can enforce the boundary between request-level override and
  infrastructure config.
- Internal agents can still vary provider behavior per agent without turning
  each agent document into a full provider installation manifest.
