# ADR-044: Versioned Packs, Explicit Upgrades, And Safe Forks

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy packs are versioned distribution units for managed product
definitions.

Design rules:

- packs use explicit versions
- installations pin exact pack versions
- upgrades are explicit operations
- upgrades should support dry-run and explanation
- pack-managed definitions are immutable in place

Built-ins are treated as bundled first-party packs rather than as a separate
mutation model.

Users and internal agents customize managed pack objects through explicit fork
operations instead of editing pack-managed files directly.

The safe default is explicit same-ID fork by shadowing:

- the fork becomes a user-owned definition
- it is stored in the normal top-level definition directories
- it shadows the managed pack object for this installation
- it records `forked_from` provenance
- later pack upgrades do not overwrite it

Normal create operations must not silently shadow managed pack objects.

## Rationale

- Explicit version pins make pack behavior predictable across upgrades and
  across agent-managed automation.
- Treating built-ins as bundled packs avoids maintaining a second special-case
  lifecycle for first-party content.
- Immutable managed objects keep upgrades safe and comprehensible.
- Explicit same-ID forks preserve local references and pack ergonomics while
  still protecting user changes from upstream updates.

## Consequences

- The control plane should expose pack install, inspect, upgrade, and uninstall
  operations.
- Definition resources that may originate from packs should expose explicit
  fork operations.
- Pack upgrade logic only mutates managed pack content.
- User-owned forks remain detached until a future explicit refresh or rebase
  workflow is invoked.
