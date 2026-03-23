# ADR-030: Agent API Definition And Operational Surfaces

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public `/api/v1/agents` surface should include both:

- a definition-first primary resource
- explicit operational sub-resources

Primary definition resource:

- `GET /api/v1/agents`
- `POST /api/v1/agents`
- `POST /api/v1/agents/validate`
- `POST /api/v1/agents/compile`
- `GET /api/v1/agents/{id}`
- `PUT /api/v1/agents/{id}`
- `DELETE /api/v1/agents/{id}`
- `GET /api/v1/agents/{id}/compiled`

Operational sub-resources:

- `GET /api/v1/agents/{id}/runtime`
- `POST /api/v1/agents/{id}/runtime/start`
- `POST /api/v1/agents/{id}/runtime/stop`
- `POST /api/v1/agents/{id}/runtime/restart`
- `PUT /api/v1/agents/{id}/runtime/mode`
- `GET /api/v1/agents/{id}/sessions`
- `POST /api/v1/agents/{id}/sessions`
- `GET /api/v1/agents/{id}/sessions/{session_id}`
- `POST /api/v1/agents/{id}/sessions/{session_id}/activate`
- `POST /api/v1/agents/{id}/sessions/{session_id}/reset`
- `POST /api/v1/agents/{id}/sessions/{session_id}/compact`
- `POST /api/v1/agents/{id}/messages`
- `POST /api/v1/agents/{id}/messages/stream`

## Rationale

- The product should model agents first as persistent definitions.
- OpenFang already has strong direct-agent operational capabilities that remain
  part of the product value.
- Hiding all operational agent behavior behind ad hoc or legacy routes would
  weaken the fork's platform identity.
- Collapsing definition and runtime into one flat resource would make the API
  less clear.
- Agents are part of the primary control plane, so both humans and internal
  agents need a complete public surface for managing live agent behavior.
- Machine-driven administration benefits from validate and compile endpoints
  before definitions are applied.

## Consequences

- `/api/v1/agents` stays definition-first without pretending agents are only
  static definitions.
- Runtime status, sessions, and direct messaging become first-class parts of
  the public product API.
- The product keeps a clean distinction between definition management and live
  agent operations.
- A matching CLI surface should mirror these capabilities under
  `compozy agents ...`.
- Validation and compilation become part of the normal public control plane.
- The exact payloads for list, detail, validation, compilation, runtime,
  sessions, and messaging should follow `API-SPEC.md`.
