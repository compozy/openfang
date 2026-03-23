# ADR-012: Arky Provider Depth For Claude Code And Codex

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Arky crates are part of the OpenFang workspace and provide the deep, typed provider layer for Claude Code and Codex. OpenFang continues to provide provider breadth for the wider model landscape.

## Rationale

- Claude Code and Codex are strategic providers for Compozy.
- OpenFang gives broad provider coverage, but the Arky crates hold deeper implementations where Compozy most needs product quality.
- Keeping Arky crates inside the workspace avoids path-dependency fragility and makes them first-class members of the build and CI pipeline.

## Consequences

- The provider layer is intentionally hybrid for a while.
- A full provider rewrite is not required for the first meaningful version.
- The public agent schema should acknowledge the typed provider model from the Arky crates rather than flattening all provider behavior into generic fields.
- Claude Code, Codex, and Claude-compatible wrappers may require namespaced
  provider-specific config blocks in `agent_definition`.
