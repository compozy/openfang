## markdown

## status: pending

<task_context>
<domain>domain/tasks/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task23</dependencies>
</task_context>

# Task 26.0: Task And Subtask Control-Plane Plus Replanning

## Overview

Expose `tasks`, `subtasks`, and explicit `replan` behavior through the public
control plane.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement the public task/subtask API endpoints.
- Implement `replan` as an explicit operation with durable effects.
</requirements>

## Subtasks

- [ ] 26.1 Implement task and subtask API list/detail/mutation surfaces.
- [ ] 26.2 Implement `replan` against the durable task/subtask model.
- [ ] 26.3 Add tests for linked artifacts/docs/files views and replanning behavior.

## Implementation Details

This task makes the domain model available to humans and internal agents via
the control plane.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- looper API/runtime surfaces
- artifact/doc versioning

## Deliverables

- Public task/subtask control-plane surfaces
- `replan` implementation
- Tests for control-plane and replanning behavior

## Tests

### Unit Tests (Required)

- [ ] Task and subtask payloads match the accepted public schema.
- [ ] `replan` creates, updates, and cancels subtasks correctly.
- [ ] Linked artifact/doc/file projections are consistent with task refs.

### Integration Tests (Required)

- [ ] End-to-end task and subtask CRUD works through API.
- [ ] Replanning preserves task identity while changing the subtask plan.
- [ ] Internal agentic administration can operate tasks through the same public surfaces.

### Regression and Anti-Pattern Guards

- [ ] Do not model `replan` as ad hoc patching scattered across multiple endpoints without one semantic operation.
- [ ] Do not hide subtasks only under looper-run surfaces.
- [ ] Do not make task control depend on workflow-run internals.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `tasks` and `subtasks` are first-class public resources.
- Replanning is explicit, durable, and testable.

---

## Prior Implementation Reference

The old TypeScript codebase has the prior task/subtask API surfaces and replanning logic:

- `~/Dev/compozy/compozy-code/packages/backend/src/modules/tasks/route.ts` — Old task CRUD routes
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/tasks/usecases.ts` — Task use cases including replan behavior
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/subtasks/` — Subtask CRUD and lifecycle
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/tasks/` — Frontend task management system

The old replan was ad hoc; the new model makes it an explicit durable operation with clear semantics.

## Notes

- This task operationalizes the task/subtask domain model.
- CLI commands for task/subtask management are deferred to future work (do not touch openfang-cli).
