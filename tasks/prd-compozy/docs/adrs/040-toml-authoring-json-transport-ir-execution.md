# ADR-040: TOML Authoring, JSON Transport, IR Execution

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The canonical authoring format for file-backed Compozy definitions is TOML.

The canonical API transport format is JSON representing the same logical model.

The canonical execution format is an internal IR produced by:

1. parse
2. validate
3. normalize
4. compile

The runtime should execute the IR, not raw TOML or raw API payloads.

## Rationale

- TOML fits the config-first, file-backed model better than making JSON or a
  database the primary authoring format.
- CLI and API still need a structured transport format that is not tied to the
  exact file syntax.
- Executing only the IR keeps runtime behavior stable even as authoring syntax
  evolves.

## Consequences

- Files under `~/.compozy/` remain the source of truth for definitions.
- API and CLI can manipulate the same logical definitions without forcing raw
  TOML as the only wire format.
- Compiled projections may be cached, but they remain derived artifacts.
- This rule applies across config-first definition families, including agents,
  workflows, triggers, and schedules.
