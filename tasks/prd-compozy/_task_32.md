## markdown

## status: pending

<task_context>
<domain>domain/tasks/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task28</dependencies>
</task_context>

# Task 32.0: Task And Subtask Control-Plane Plus Replanning

## Overview

Expose `tasks`, `subtasks`, and the explicit `replan` operation through the public control plane
under `/api/v1/tasks` and `/api/v1/subtasks`. Tasks are durable domain objects (not temporary
queue items) that anchor the working context of product execution. Subtasks are their executable
child units. Together they form the primary work model of the product per ADR-045 and ADR-047.

This task implements the minimum public surfaces mandated by ADR-048:
`/api/v1/tasks`, `/api/v1/tasks/{id}`, `/api/v1/tasks/{id}/subtasks`,
`/api/v1/tasks/{id}/replan`, `/api/v1/tasks/{id}/artifacts`, `/api/v1/tasks/{id}/docs`,
`/api/v1/tasks/{id}/files`, `/api/v1/subtasks`, and `/api/v1/subtasks/{id}`.

The `replan` operation is a first-class action that atomically changes the subtask plan of an
existing task (via `cancel_subtasks`, `create_subtasks`, and `update_subtasks` operations) without
replacing task identity or requiring the caller to patch individual subtask rows. This is the
canonical model described in API-SPEC.md section 12 and replaces the ad hoc patching approach
used in the old TypeScript codebase.

These surfaces must be designed for both direct human administration and internal agentic
administration through the same public contracts per ADR-031 and DESIGN.md section 2.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement the full task CRUD surface at `/api/v1/tasks`: `GET /api/v1/tasks`,
  `POST /api/v1/tasks`, `GET /api/v1/tasks/{id}`, `PUT /api/v1/tasks/{id}`, and
  `DELETE /api/v1/tasks/{id}`. The task resource shape must include all fields from API-SPEC.md
  section 12: `id`, `slug`, `title`, `description`, `status`, `priority`, `complexity`,
  `position`, `source`, `owner`, `created_by`, `repository_refs`, `label_refs`, `artifact_refs`,
  `doc_refs`, `file_refs`, `metadata`, `created_at`, `updated_at`, `completed_at`.
- Implement subtask surfaces: `GET /api/v1/tasks/{id}/subtasks`, `POST /api/v1/tasks/{id}/subtasks`,
  `GET /api/v1/subtasks`, `GET /api/v1/subtasks/{id}`, `PUT /api/v1/subtasks/{id}`, and
  `DELETE /api/v1/subtasks/{id}`. The subtask resource shape must include `id`, `task_id`, `title`,
  `description`, `kind`, `status`, `complexity`, `position`, `assignee`, `depends_on`,
  `parallelizable`, `input`, `result`, `metadata`, `created_at`, `updated_at`, `completed_at`.
- Implement `POST /api/v1/tasks/{id}/replan` as a first-class atomic operation per API-SPEC.md
  section 12. The request body carries `{ reason, operations, metadata }` where `operations` is
  an ordered list of `cancel_subtasks`, `create_subtasks`, and `update_subtasks` operations applied
  in sequence. The response is `{ accepted, resource_id, status, effects: { created_subtasks,
  updated_subtasks, cancelled_subtasks } }`.
- Implement the linked context sub-resources: `GET /api/v1/tasks/{id}/artifacts`,
  `GET /api/v1/tasks/{id}/docs`, and `GET /api/v1/tasks/{id}/files`. Each returns
  `{ items, next_cursor }` per ADR-034. These views project the artifact, doc, and file refs
  attached to the task without duplicating the full artifact or doc resource shapes.
- All list endpoints return `{ items, next_cursor }` with `limit` (default 50, max 200), `cursor`,
  `sort`, `order`, and the filter parameters specified in API-SPEC.md section 12: `status`,
  `priority`, `created_by`, `source_kind`, `label`, `repository`, and `q` for tasks; `task_id`,
  `status`, `assignee_kind`, `assignee_ref`, `kind`, `ready`, and `blocked` for subtasks.
- Tasks and subtasks are stored in `compozy.db` (per DESIGN.md section 4 and ADR-003), not in
  `runtime.db` or in file-backed definitions. The `task` and `subtask` public domain names must
  not be renamed to protect legacy OpenFang internal naming per ADR-047.
- Replan is the only canonical way to change the subtask plan of a task in bulk. Ad hoc patching
  of individual subtask rows to achieve replanning is explicitly forbidden (see Anti-Pattern Guards).
</requirements>

## Subtasks

- [ ] 32.1 Register the `/api/v1/tasks` and `/api/v1/subtasks` router groups in
      `crates/openfang-api/src/server.rs`. Implement `GET /api/v1/tasks` (paginated list with
      `status`, `priority`, `created_by`, `source_kind`, `label`, `repository`, `q` filters),
      `POST /api/v1/tasks` (create, persisted to `compozy.db`), `GET /api/v1/tasks/{id}` (full detail
      with all ref fields), `PUT /api/v1/tasks/{id}` (update), and `DELETE /api/v1/tasks/{id}`.
      Create and update must return the full task resource shape.
- [ ] 32.2 Implement the subtask surfaces: `GET /api/v1/tasks/{id}/subtasks` (paginated, filtered),
      `POST /api/v1/tasks/{id}/subtasks` (create subtask under task), `GET /api/v1/subtasks`
      (global paginated list with `task_id`, `status`, `assignee_kind`, `assignee_ref`, `kind`,
      `ready`, `blocked` filters), `GET /api/v1/subtasks/{id}` (full detail), `PUT /api/v1/subtasks/{id}`
      (update), and `DELETE /api/v1/subtasks/{id}`. Subtask create and update must return the full
      subtask resource shape.
- [ ] 32.3 Implement `POST /api/v1/tasks/{id}/replan`. The handler must apply all operations in
      the `operations` array atomically inside a single `compozy.db` transaction:
      `cancel_subtasks` marks the listed subtask IDs as cancelled (must reject if any ID is not a
      subtask of the target task), `create_subtasks` inserts new subtask rows with the supplied fields,
      and `update_subtasks` applies the supplied field patches to the identified subtask rows. The
      response carries `{ accepted, resource_id, status, effects }` with counts for each operation
      kind. Task identity (`id`, `slug`, `created_at`) must not change as a result of replan.
- [ ] 32.4 Implement the linked context sub-resources: `GET /api/v1/tasks/{id}/artifacts`,
      `GET /api/v1/tasks/{id}/docs`, and `GET /api/v1/tasks/{id}/files`. Each returns
      `{ items, next_cursor }`. Items project the corresponding ref fields already attached to the
      task (`artifact_refs`, `doc_refs`, `file_refs`) without fetching the full artifact or doc bodies.
- [ ] 32.5 Define the `compozy.db` table schemas for `tasks` and `subtasks` if not already
      created by task 23. The tables must support all queryable fields used by the list filters
      (`status`, `priority`, `created_by`, `source_kind`, `label`, `repository`, `assignee_kind`,
      `assignee_ref`, `kind`, `ready`, `blocked`). The ref fields (`artifact_refs`, `doc_refs`,
      `file_refs`, `repository_refs`, `label_refs`) may be stored as JSON columns on the task row or
      as normalized join tables; either is acceptable provided the list filter queries remain indexed.
- [ ] 32.6 Verify that the same public task and subtask endpoints are usable by internal agents
      (not only human operators). Write an integration test that simulates an agent-sourced replan
      request with `metadata.source = "agent"` and verifies the effects response.
- [ ] 32.7 Add route-level and handler-level tests. See the Tests section below.

## Implementation Details

Tasks and subtasks are Compozy-owned domain objects stored in `compozy.db`. They are not
file-backed definitions and do not go through the validate-normalize-write-reload path used by
agents, workflows, triggers, and schedules. Persistence is straightforward SQL through the
existing SQLite layer.

The task resource shape is defined in API-SPEC.md section 12. The source field uses a union:
`{ kind: "workflow", workflow_id, run_id }` or `{ kind: "manual" }` or `{ kind: "api" }`.
The owner and created_by fields use `{ kind: "agent"|"agent_group"|"user", ref: "..." }`.

The subtask `assignee` field uses `{ kind: "agent"|"agent_group"|"user", ref: "..." }`.
The `depends_on` field is an array of subtask IDs that must complete before this subtask is ready.
The `parallelizable` boolean signals to the looper whether multiple subtasks may run concurrently.

The `replan` request shape per API-SPEC.md section 12:

```
{
  "reason": "string",
  "operations": [
    { "op": "cancel_subtasks", "subtask_ids": ["subtask_003"] },
    { "op": "create_subtasks", "items": [ { ...subtask fields... } ] },
    { "op": "update_subtasks", "items": [ { "id": "subtask_004", "depends_on": [...] } ] }
  ],
  "metadata": { "source": "agent" }
}
```

The `replan` response shape per API-SPEC.md section 12:

```
{
  "accepted": true,
  "resource_id": "task_001",
  "status": "accepted",
  "effects": {
    "created_subtasks": 1,
    "updated_subtasks": 1,
    "cancelled_subtasks": 1
  }
}
```

Linked context sub-resource shapes (API-SPEC.md section 12):

- `/artifacts`: `{ items: [{ artifact_id, type, current_version_id }], next_cursor }`
- `/docs`: `{ items: [{ doc_id, type, current_version_id }], next_cursor }`
- `/files`: `{ items: [{ path, kind, description }], next_cursor }`

All list endpoints follow the `{ items, next_cursor }` convention from ADR-034. Operational
actions (replan) use the accepted envelope. Error responses use the stable
`{ error: { code, message, details } }` envelope.

The old TypeScript codebase task/subtask surfaces are referenced in the Prior Implementation
Reference section below. The new Rust model makes replan an explicit atomic operation rather than
a collection of scattered endpoint calls. The old approach must not be replicated.

### Relevant Files

- `crates/openfang-api/src/routes.rs` — existing handler implementations; add task and subtask handlers here
- `crates/openfang-api/src/server.rs` — router registration; add `/api/v1/tasks` and `/api/v1/subtasks` blocks
- `tasks/prd-compozy/docs/API-SPEC.md` — canonical payload contracts (section 12 for tasks/subtasks, section 2 for common conventions)
- `tasks/prd-compozy/docs/DESIGN.md` — section 8 (domain primitives, task/subtask domain model)
- `tasks/prd-compozy/docs/adrs/045-task-subtask-domain-model.md`
- `tasks/prd-compozy/docs/adrs/047-keep-task-and-subtask-as-public-domain-names.md`
- `tasks/prd-compozy/docs/adrs/048-task-and-subtask-control-plane-surfaces.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`
- `tasks/prd-compozy/docs/adrs/031-cli-and-api-as-primary-control-plane.md`

### Dependent Files

- `compozy.db` schema from task 28 (tasks and subtasks tables)
- Artifact and doc versioning surfaces (for ref projection in linked context sub-resources)

## Deliverables

- `GET`, `POST`, `GET {id}`, `PUT {id}`, `DELETE {id}` for `/api/v1/tasks`
- `GET /api/v1/tasks/{id}/subtasks` and `POST /api/v1/tasks/{id}/subtasks`
- `POST /api/v1/tasks/{id}/replan` with atomic operation semantics
- `GET /api/v1/tasks/{id}/artifacts`, `GET /api/v1/tasks/{id}/docs`, `GET /api/v1/tasks/{id}/files`
- `GET /api/v1/subtasks`, `GET /api/v1/subtasks/{id}`, `PUT /api/v1/subtasks/{id}`, `DELETE /api/v1/subtasks/{id}`
- Tests for all control-plane and replanning behavior

## Tests

### Unit Tests (Required)

- [ ] Task resource shape serializes/deserializes correctly for all top-level fields including
      nested `source`, `owner`, `created_by`, `repository_refs`, `label_refs`, `artifact_refs`,
      `doc_refs`, and `file_refs`.
- [ ] Subtask resource shape serializes/deserializes correctly for all fields including `assignee`,
      `depends_on`, `parallelizable`, `input`, and `result`.
- [ ] A `replan` request with `cancel_subtasks` referencing a subtask ID that belongs to a
      different task returns a structured 422 error; the operation must not partially apply.
- [ ] A `replan` request with `create_subtasks` items whose `depends_on` references a non-existent
      subtask ID returns a structured 422 error before any database writes occur.
- [ ] A `replan` request with all three operation kinds (`cancel_subtasks`, `create_subtasks`,
      `update_subtasks`) applies all three atomically: the effects response counts must match the
      actual database state after the operation.
- [ ] Subtask list filter `ready=true` returns only subtasks whose `depends_on` subtasks are all
      in a completed status; `blocked=true` returns subtasks with at least one incomplete dependency.

### Integration Tests (Required)

- [ ] Full task CRUD round-trip: create a task (`POST`), read it back (`GET {id}`) with all ref
      fields, update its `priority` (`PUT {id}`), verify the update is reflected, delete it
      (`DELETE {id}`), confirm subsequent `GET {id}` returns 404.
- [ ] Create a task with two subtasks, then call `POST /api/v1/tasks/{id}/replan` with one
      `cancel_subtasks` and one `create_subtasks` operation; verify the response `effects` counts are
      correct and the database reflects the new subtask plan without changing the task `id` or `slug`.
- [ ] `GET /api/v1/tasks/{id}/subtasks` returns only subtasks belonging to that task; adding a
      subtask to a different task does not appear in the first task's list.
- [ ] `GET /api/v1/tasks/{id}/artifacts` returns `{ items, next_cursor }` projecting only the
      `artifact_refs` attached to that task.
- [ ] `GET /api/v1/tasks` with filter `status=in_progress` returns only tasks whose status field
      matches; `q=onboarding` performs text search across `title` and `description`.
- [ ] `GET /api/v1/subtasks` with filter `assignee_ref=prd-writer` returns only subtasks assigned
      to that agent.
- [ ] Internal agent-sourced replan: a replan request with `metadata.source = "agent"` succeeds
      and is stored; the same public endpoint is used by both human operators and internal agents.

### Regression and Anti-Pattern Guards

- [ ] `replan` is the only canonical way to change the subtask plan in bulk; do not add hidden
      side-effecting paths on `PUT /api/v1/tasks/{id}` that silently replace or delete subtasks.
- [ ] Subtasks are not hidden under looper-run surfaces only; `GET /api/v1/tasks/{id}/subtasks`
      and `GET /api/v1/subtasks` must work independently of whether a looper-run exists.
- [ ] Task control does not depend on workflow-run internals; `GET /api/v1/tasks/{id}` must return
      the task even when no associated `run_id` exists (source kind `"manual"`).
- [ ] The public domain names `task` and `subtask` must not be renamed to `issue` or
      `work_item` or any other legacy term in handler code, route paths, or response payloads.
- [ ] A `replan` that partially fails (e.g., valid `cancel_subtasks` but invalid `create_subtasks`)
      must not apply any part of the operation; the entire transaction must roll back.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All fourteen endpoints are registered in the axum router and return correct status codes and
  payload shapes for both happy-path and error cases.
- `POST /api/v1/tasks/{id}/replan` applies `cancel_subtasks`, `create_subtasks`, and
  `update_subtasks` atomically in a single `compozy.db` transaction with correct effects counts.
- Task identity (`id`, `slug`, `created_at`) is preserved across all replan operations.
- Linked context sub-resources (`/artifacts`, `/docs`, `/files`) return `{ items, next_cursor }`
  projecting refs from the task without fetching full artifact or doc bodies.
- Subtask list filters (`ready`, `blocked`, `assignee_ref`, etc.) return correct filtered results
  backed by indexed queries in `compozy.db`.
- Both human operators and internal agents (with `metadata.source = "agent"`) can use the same
  public endpoints without special-casing.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Prior Implementation Reference

The old TypeScript codebase has the prior task/subtask API surfaces and replanning logic:

- `~/Dev/compozy/compozy-code/packages/backend/src/modules/tasks/route.ts` — Old task CRUD routes
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/tasks/usecases.ts` — Task use cases including replan behavior
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/subtasks/` — Subtask CRUD and lifecycle
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/tasks/` — Frontend task management system

The old replan was ad hoc (scattered PUT calls on subtask rows); the new model makes it an
explicit atomic operation with clear semantics and a structured effects response.

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- This task operationalizes the task/subtask domain model. CLI commands for task/subtask
  management are deferred to future work (do not touch openfang-cli).
- Looper execution starts by creating a `looper-run` via `POST /api/v1/looper-runs` with a
  `task_id`. That is a separate resource and out of scope for this task.
