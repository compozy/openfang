# ADR-048: Task And Subtask Control Plane Surfaces

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Compozy exposes `task` and `subtask` as first-class public resources in the
primary control plane.

The minimum public surfaces include:

- `/api/v1/tasks`
- `/api/v1/tasks/{id}`
- `/api/v1/tasks/{id}/subtasks`
- `/api/v1/subtasks`
- `/api/v1/subtasks/{id}`
- `/api/v1/tasks/{id}/replan`
- `/api/v1/tasks/{id}/artifacts`
- `/api/v1/tasks/{id}/docs`
- `/api/v1/tasks/{id}/files`

`replan` is a first-class task action that changes the subtask plan of an
existing task without replacing task identity.

Looper execution should start by creating a `looper-run` against an explicit
`task_id`:

- `POST /api/v1/looper-runs`

This is the canonical public way to start looper execution for a task.

## Rationale

- `task` is the durable work anchor of the product, not only a container for
  looper state.
- `subtask` is the executable child unit that needs its own retrieval and
  mutation surface.
- replanning is central to an agentic system and should be modeled explicitly
  instead of hidden in scattered low-level edits.
- looper execution is a runtime resource creation, not merely a flag change on
  the task object.

## Consequences

- `tasks` and `subtasks` join the primary public control plane.
- linked artifacts, docs, and files are navigated from the task surface.
- `looper-run` payloads should include `task_id`.
- CLI grammar should mirror the same model with `compozy tasks ...`,
  `compozy subtasks ...`, and `compozy looper-runs create ...`.
