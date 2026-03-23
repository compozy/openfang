# ADR-033: Trigger API Definition And Operational Surfaces

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public `/api/v1/triggers` surface should include:

- definition management
- validation and compilation helpers
- operational and inspection sub-resources

Definition and validation resource set:

- `GET /api/v1/triggers`
- `POST /api/v1/triggers`
- `POST /api/v1/triggers/validate`
- `POST /api/v1/triggers/compile`
- `GET /api/v1/triggers/{id}`
- `PUT /api/v1/triggers/{id}`
- `DELETE /api/v1/triggers/{id}`
- `GET /api/v1/triggers/{id}/compiled`

Operational and inspection sub-resources:

- `GET /api/v1/triggers/{id}/runtime`
- `POST /api/v1/triggers/{id}/enable`
- `POST /api/v1/triggers/{id}/disable`
- `POST /api/v1/triggers/{id}/test`

Related shared ingress:

- `POST /api/v1/events`

## Rationale

- Triggers are part of the primary control plane for event-driven automation.
- Machine-driven administration benefits from validate, compile, and test
  endpoints before a trigger is enabled or before real events are injected.
- A public event ingress is required if external systems and internal agents are
  supposed to drive event-based automation through the same contract.

## Consequences

- Trigger control becomes more complete than a simple create/delete surface.
- Testing and event-injection become first-class public capabilities.
- A matching CLI surface should mirror this model under `compozy triggers ...`
  and `compozy events ...`.
- The exact payloads for trigger definitions, runtime, tests, and event ingress
  should follow `API-SPEC.md`.
