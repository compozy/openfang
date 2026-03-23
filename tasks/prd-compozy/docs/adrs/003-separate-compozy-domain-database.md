# ADR-003: Separate Compozy Domain Database

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy uses two SQLite databases under the same `~/.compozy/data/` root:

- `runtime.db` for platform-core runtime state
- `compozy.db` for Compozy domain state and durable workflow state

## Rationale

- Keeps product-domain ownership clear.
- Reduces schema coupling with upstream OpenFang.
- Gives Compozy freedom to evolve durable run state without entangling it with OpenFang engine persistence.

## Consequences

- Cross-system relationships are resolved in application code, not SQL joins.
- The product ships with two local SQLite files, each with a distinct ownership boundary.
- The public product should not expose OpenFang-branded database names as part of the normal user-facing surface.
- Definition resources should remain file-backed under `~/.compozy/` instead of
  turning either database into a second source of truth for config-first
  objects.
