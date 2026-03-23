# ADR-020: Compozy-Owned Workflow And Trigger Surfaces

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The fork introduces Compozy-owned public surfaces for:

- workflow definitions
- trigger definitions
- workflow runs
- dispatches
- HITL requests
- looper runs

Those surfaces live under a single public Compozy namespace:

- config and files under `~/.compozy`
- public HTTP API under `/api/v1`
- public CLI under `compozy`

OpenFang remains the platform core underneath, but its existing workflow and trigger files, routes, and editors are not the primary public contract of the new product.

Legacy OpenFang workflow and trigger shapes may remain available as import paths or transitional adapters when useful.

## Rationale

- The fork does not need to preserve public backward compatibility for an inherited OpenFang user base.
- The current OpenFang workflow and trigger surfaces are too constrained by in-memory assumptions, reduced API shapes, and agent-centric contracts.
- Trying to preserve those old surfaces as the main product contract would distort the new product more than it would help.

## Consequences

- Compozy can design cleaner workflow and trigger authoring surfaces without creating a second runtime center.
- Old OpenFang workflow and trigger formats move to a legacy or import role.
- Product UX and API design can optimize for the Compozy model instead of mirroring legacy OpenFang route contracts.
- The product should not expose dual public identities such as `.openfang` plus `.compozy` or competing route namespaces.
