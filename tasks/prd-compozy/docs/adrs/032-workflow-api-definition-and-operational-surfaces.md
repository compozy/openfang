# ADR-032: Workflow API Definition And Operational Surfaces

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public `/api/v1/workflows` surface should include:

- definition management
- validation and compilation helpers
- operational run and runtime sub-resources

Definition and validation resource set:

- `GET /api/v1/workflows`
- `POST /api/v1/workflows`
- `POST /api/v1/workflows/validate`
- `POST /api/v1/workflows/compile`
- `GET /api/v1/workflows/{id}`
- `PUT /api/v1/workflows/{id}`
- `DELETE /api/v1/workflows/{id}`
- `GET /api/v1/workflows/{id}/compiled`

Operational and inspection sub-resources:

- `GET /api/v1/workflows/{id}/runtime`
- `POST /api/v1/workflows/{id}/runs`
- `GET /api/v1/workflows/{id}/runs`

## Rationale

- Workflows are part of the primary control plane, not only static config.
- Machine-driven administration benefits from validate and compile endpoints
  before definitions are applied.
- Workflow definitions and workflow executions need related but distinct
  surfaces.

## Consequences

- Workflow control becomes definition-first without hiding execution.
- Validation and compilation become normal public capabilities of the control
  plane.
- A matching CLI surface should mirror this model under `compozy workflows ...`.
- The exact payloads for definitions, runtime, compilation, and run creation
  should follow `API-SPEC.md`.
