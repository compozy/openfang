# ADR-047: Keep Task And Subtask As Public Domain Names

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy keeps `task` and `subtask` as the public domain names for its work
model.

The old OpenFang task queue may continue to exist as an internal or legacy
mechanism, but it does not control the public naming of the Compozy domain.

Any collision between the Compozy work model and the old OpenFang queue should
be handled internally through:

- separate modules and types
- separate tables and storage ownership
- explicit legacy adapters

The product should not rename its public domain objects to protect internal
legacy naming.

## Rationale

- `task` and `subtask` are clearer and more natural product terms than the
  alternative names considered.
- Renaming the Compozy domain to avoid a collision with legacy OpenFang
  internals would make the public API, CLI, and documentation worse.
- The actual implementation risk comes from shared semantics and storage, not
  from the existence of the same word in two different internal layers.

## Consequences

- The public control plane should continue to speak in terms of `task` and
  `subtask`.
- Internal implementation should isolate any surviving OpenFang queue concepts
  under clearly separated modules, tables, and adapter boundaries.
- Future work on task APIs, looper execution, and storage should assume
  `task/subtask` is stable public terminology.
