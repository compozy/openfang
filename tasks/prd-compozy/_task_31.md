## markdown

## status: pending

<task_context>
<domain>domain/looper/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task27,task26,task28</dependencies>
</task_context>

# Task 31.0: Looper Control-Plane And SSE Surfaces

## Overview

Expose looper control-plane API surfaces and SSE/watch endpoints for looper
runs. This task covers the public API layer on top of the durable looper
runtime implemented in task 28.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Expose looper run control-plane surfaces through the API.
- Implement SSE/watch endpoints for looper run event streaming.
</requirements>

## Subtasks

- [ ] 31.1 Implement looper control-plane API surfaces: `POST /api/looper-runs`, `GET /api/looper-runs`, `GET /api/looper-runs/{id}`, `GET /api/looper-runs/{id}/subtasks`, `POST /api/looper-runs/{id}/pause`, `POST /api/looper-runs/{id}/resume`, `POST /api/looper-runs/{id}/cancel`.
- [ ] 31.2 Implement SSE/watch endpoint for looper runs: `GET /api/looper-runs/{id}/events`.
- [ ] 31.3 Add tests for looper API surfaces and SSE streaming behavior.

## Implementation Details

The looper control-plane surfaces follow API-SPEC.md section 13 (Looper Runs).
The creation endpoint (`POST /api/looper-runs`) accepts a `task_id`,
optional `subtask_ids`, and an `execution_policy` object. List and detail
endpoints return the looper run shape with progress, status, and policy
metadata.

The SSE endpoint (`GET /api/looper-runs/{id}/events`) streams real-time
updates for looper run state changes, subtask starts, completions, and errors.
It should support `Last-Event-ID` for best-effort resume within a bounded
replay window, following the watch policy in API-SPEC.md section 14.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- looper runtime modules from task 28
- SSE infrastructure

## Deliverables

- Looper control-plane API surfaces (CRUD, pause, resume, cancel)
- SSE/watch endpoint for looper run events
- Tests for API surfaces and SSE streaming

## Tests

### Unit Tests (Required)

- [ ] Looper control-plane payloads align with the accepted public schema in API-SPEC.md.
- [ ] SSE event serialization produces correct event types and data formats.
- [ ] Pause/resume/cancel actions update looper run state correctly through the API.

### Integration Tests (Required)

- [ ] End-to-end looper API usage works against durable runtime state.
- [ ] SSE endpoint streams subtask progress events in real time.
- [ ] `Last-Event-ID` resume works within the bounded replay window.

### Regression and Anti-Pattern Guards

- [ ] Do not bolt on SSE/watch as ad hoc duplicates of existing endpoints.
- [ ] Do not treat this task as permission to re-architect the looper runtime from task 28.
- [ ] Do not create internal-only endpoints; keep all surfaces public.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Looper runs are fully controllable through the public API.
- SSE streaming provides real-time visibility into looper execution.

---

## Notes

- This task was slimmed from the original task 24. Artifact/doc versioning is now in task 30. Pack operationalization and final hardening are now in task 32.
- CLI commands for looper management are deferred to future work (do not touch openfang-cli).
