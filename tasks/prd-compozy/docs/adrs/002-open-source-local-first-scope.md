# ADR-002: Open Source Local-First Scope

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy is scoped as an open source, single-user, local-first product. The new version does not target parity with the current Compozy product. Billing, subscriptions, auth, and organizations are out of scope.

## Rationale

- The reset aims for flexibility and programmability, not SaaS feature parity.
- Removing SaaS concerns keeps the architecture focused on autonomy, workflows, persistence, and provider quality.

## Consequences

- Product decisions should optimize for local operation and direct user control.
- Regressions against the older web/SaaS surface are acceptable when they simplify the core.
