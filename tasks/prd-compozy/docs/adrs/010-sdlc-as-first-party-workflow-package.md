# ADR-010: SDLC As First-Party Workflow Package

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The SDLC flow is a first-party workflow package built on the same platform as user-defined workflows. It is not a separate engine and not the only intended way to use the product.

## Rationale

- The product should remain programmable by users.
- The SDLC package should demonstrate the platform rather than close it off.

## Consequences

- SDLC workflows must be built from the same primitives and runtime features available to the rest of the system.
- Product polish can focus on SDLC without making it architecturally privileged.
- SDLC should follow the same pack versioning, upgrade, and safe-fork rules as
  other first-party managed packs.
