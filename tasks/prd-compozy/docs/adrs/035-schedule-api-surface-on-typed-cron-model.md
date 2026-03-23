# ADR-035: Schedule API Surface On Typed Cron Model

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public `/api/v1/schedules` surface should stay close to the typed OpenFang
cron model.

The public surface includes:

- `GET /api/v1/schedules`
- `POST /api/v1/schedules`
- `POST /api/v1/schedules/validate`
- `GET /api/v1/schedules/{id}`
- `PUT /api/v1/schedules/{id}`
- `DELETE /api/v1/schedules/{id}`
- `GET /api/v1/schedules/{id}/runtime`
- `POST /api/v1/schedules/{id}/enable`
- `POST /api/v1/schedules/{id}/disable`
- `POST /api/v1/schedules/{id}/run-now`

The shape should preserve:

- typed `schedule`
- typed `action`
- typed `delivery`

The action family stays close to the OpenFang cron action model, with the
addition of `workflow_signal` as part of the durable workflow model.

## Rationale

- The typed cron model is already one of the stronger reusable parts of
  OpenFang.
- The product still needs a first-class public schedule surface because CLI and
  API are the primary control plane.
- Aligning action payloads with the rest of the Compozy control plane avoids
  making schedules the one isolated resource with incompatible payloads.

## Consequences

- Compozy does not promote the older blob-style schedule CRUD.
- Schedules remain more OpenFang-like than workflows and triggers.
- Schedules expose validation but do not currently require a separate public
  compilation surface.
- The exact payloads for schedules should follow `API-SPEC.md`.
