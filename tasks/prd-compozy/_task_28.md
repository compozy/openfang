## markdown

## status: pending

<task_context>
<domain>domain/looper/runtime</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task23,task26</dependencies>
</task_context>

# Task 28.0: Looper Durable Schema And Runtime

## Overview

Implement the durable looper model on top of `task` and `subtask`, with
explicit execution policy and `task_id` as the canonical anchor.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `looper_run` and `looper_subtask`.
- Implement execution over canonical subtasks with explicit sequential/parallel policy.
</requirements>

## Subtasks

- [ ] 28.1 Add looper schema and repositories.
- [ ] 28.2 Implement looper runtime selection and execution over subtasks.
- [ ] 28.3 Add tests for sequential and bounded-parallel execution policies.

## Implementation Details

The public and runtime model must stay aligned: looper is a specialized durable
executor over subtasks.

### Relevant Files

- new looper migration and repository modules
- looper runtime modules
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`
- `tasks/prd-compozy/reset-2026-03-21/DATABASE-SCHEMA.md`

### Dependent Files

- looper control-plane handlers
- final hardening and recovery tests

## Deliverables

- Looper schema and repositories
- Runtime execution over canonical subtasks
- Tests for policy-driven execution

## Tests

### Unit Tests (Required)

- [ ] Looper runs persist `task_id`, execution policy, and progress correctly.
- [ ] Subtask execution respects `depends_on` and `parallelizable`.
- [ ] Policy logic enforces sequential and bounded-parallel modes correctly.

### Integration Tests (Required)

- [ ] `POST /api/looper-runs` creates a durable looper run anchored to a task.
- [ ] Sequential looper execution behaves predictably across restart.
- [ ] Parallel looper execution respects `max_parallelism` and durable state.

### Regression and Anti-Pattern Guards

- [ ] Do not treat the old OpenFang task queue as the looper backend.
- [ ] Do not infer concurrency implicitly when the policy is missing.
- [ ] Do not let subtasks widen the looper policy beyond the configured envelope.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Looper execution is durable and policy-driven.
- It runs on top of `task` and `subtask`, not a legacy queue abstraction.

---

## Prior Implementation Reference

The old TypeScript codebase has an actor-based looper engine:

- `~/Dev/compozy/compozy-code/packages/tauri/src-node/looper/` — Actor-based looper with:
  - `actors/job-manager-actor.ts` — Job scheduling and management
  - `actors/task-stream-actor.ts` — Task streaming and execution
  - `actors/execution-control-actor.ts` — Execution control and policies
  - `core/runtime-service.ts` — Runtime service orchestration
  - `sqlite/` — Local SQLite persistence for looper state

The old looper is a Node.js actor-based engine. The new looper is a durable Rust executor on top
of the same workflow foundation. The old code shows execution policies, parallelism control, and
how subtask dependencies were evaluated at runtime.

## Notes

- This task is the core execution layer for subtasks.
