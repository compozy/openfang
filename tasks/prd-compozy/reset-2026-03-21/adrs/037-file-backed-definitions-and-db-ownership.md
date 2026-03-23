# ADR-037: File-Backed Definitions And Database Ownership

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Config-first definitions remain file-backed under `~/.compozy/`.

That includes:

- agents
- workflows
- triggers
- schedules
- skills
- packs
- templates

The databases have distinct ownership:

- `runtime.db` owns platform-core runtime state
- `compozy.db` owns product-domain state and durable workflow execution state

## Rationale

- Config-first authoring loses value if API and CLI silently move the source of
  truth into a database.
- Files are easier to inspect, diff, back up, and manipulate with automation.
- Separate database ownership makes the boundary between platform runtime and
  product semantics easier to keep clean.

## Consequences

- API and CLI mutations for definitions should write canonical file-backed
  representations.
- Databases may contain indexes, caches, compiled projections, and runtime
  metadata, but not competing authoritative copies of definition resources.
- Cross-database references are resolved in application code, not through
  shared SQL joins.
- The detailed storage split should be documented centrally in
  `STORAGE-MODEL.md`.
