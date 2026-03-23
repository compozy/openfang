# ADR-034: Canonical Control-Plane Payload Conventions

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public Compozy control plane should use one canonical payload convention
across `/api/v1` and `compozy ...`.

That contract is written in `API-SPEC.md`.

The baseline conventions are:

- list endpoints return `items` plus `next_cursor`
- validation endpoints accept `{ definition, strict, context }` and return
  `{ valid, issues, normalized }`
- compilation endpoints accept `{ definition, context }` and return
  `{ definition_id, normalized, compiled }`
- dry-run endpoints mirror the real side-effecting request and return
  `{ would_execute, resolved, effects, explanation }`
- definition create and update endpoints return the full resulting resource
- operational actions return an accepted-style envelope instead of ad hoc
  free-form payloads
- structured error responses use a stable `{ error: { ... } }` envelope
- streaming endpoints use SSE with stable event names

## Rationale

- CLI and API are the primary control plane, so both humans and machines need
  stable patterns across resources.
- Internal agents should not have to relearn payload shapes for every resource
  family.
- A single convention reduces accidental inconsistencies between agents,
  workflows, triggers, and runtime resources.

## Consequences

- `API-SPEC.md` becomes the canonical contract reference for payload and command
  grammar details.
- Resource-specific ADRs remain focused on what surfaces exist and what they
  mean, while the shared payload structure is centralized.
- Future public resource additions should follow these conventions unless an ADR
  explicitly documents a justified exception.
