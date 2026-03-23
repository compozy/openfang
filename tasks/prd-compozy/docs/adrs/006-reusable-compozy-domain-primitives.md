# ADR-006: Reusable Compozy Domain Primitives

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy exposes reusable domain primitives instead of hiding product behavior inside one closed SDLC pipeline.

Initial primitives:

- `artifact.*`
- `doc.*`
- `hitl.*`
- `task.*`
- `subtask.*`
- `agent.*`
- `capability.*`

## Rationale

- New agents such as `PromptMaker` must reuse durable platform concepts instead of forcing new bespoke subsystems.
- The product now treats `task` as the root durable work object, so the old
  internal `issue.*` shape should not remain the primary domain primitive.
- The SDLC package should be one composition on top of shared primitives, not a special case that blocks future growth.

## Consequences

- Primitive design becomes a product API surface, not just internal plumbing.
- User workflows and first-party workflows can share the same building blocks.
