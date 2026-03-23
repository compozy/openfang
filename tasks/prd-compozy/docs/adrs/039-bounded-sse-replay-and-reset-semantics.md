# ADR-039: Bounded SSE Replay And Reset Semantics

**Status:** Accepted
**Date:** 2026-03-21

## Decision

SSE watch endpoints should support bounded replay with `Last-Event-ID`, not
unbounded historical backfill.

Rules:

- replay is best-effort within a bounded retention window
- if the requested event is unavailable, the server emits `stream.reset`,
  then `stream.snapshot`, then continues with live events
- long-term history belongs to normal resource endpoints such as run detail,
  checkpoints, dispatch detail, and HITL detail

## Rationale

- The control plane needs resumable live streams for agents and automation.
- Treating SSE as a full history API would overcomplicate retention and
  persistence requirements.
- Reset-and-snapshot semantics are simpler for clients than silent gaps.

## Consequences

- SSE remains suitable for local-first live operations.
- Durable history stays in normal read models instead of drifting into stream
  retention logic.
- Exact watch endpoints and event names should follow `API-SPEC.md`.
