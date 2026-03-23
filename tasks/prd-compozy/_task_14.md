## markdown

## status: pending

<task_context>
<domain>engine/workflows/runtime</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task6,task8,task9</dependencies>
</task_context>

# Task 14.0: Durable Workflow Run Repository And Transition Writer

## Overview

Replace the purely in-memory workflow run lifecycle with a durable repository
and transition writer built on `workflow_run` and `workflow_checkpoint`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Introduce a durable repository for `workflow_run`.
- Centralize major state transitions through a transition writer.
</requirements>

## Subtasks

- [ ] 14.1 Implement repositories for `workflow_run` and `workflow_checkpoint`.
- [ ] 14.2 Introduce a transition writer that updates run state and appends checkpoints coherently.
- [ ] 14.3 Route workflow run creation and state transitions through the durable path.

## Implementation Details

This task is the center of the Phase 1 runtime refactor.

### Relevant Files

- `crates/openfang-kernel/src/workflow.rs`
- `crates/openfang-kernel/src/kernel.rs`
- new repositories for `compozy.db`
- `tasks/prd-compozy/reset-2026-03-21/INITIAL-RUNTIME-MIGRATIONS.md`

### Dependent Files

- `crates/openfang-api/src/routes.rs`
- workflow tests

## Deliverables

- Durable workflow run repository
- Transition writer
- Workflow run creation path persisted before execution

## Tests

### Unit Tests (Required)

- [ ] Run creation persists `workflow_run` and `run_created`/`run_started` checkpoints.
- [ ] Transition writer updates run state and checkpoints atomically where expected.
- [ ] Terminal states persist correctly with error metadata when applicable.

### Integration Tests (Required)

- [ ] Starting a workflow creates a durable run record immediately.
- [ ] Run list/detail after execution reflects durable state, not only memory.
- [ ] Restart retains run identity and major state transitions.

### Regression and Anti-Pattern Guards

- [ ] No important workflow run exists only in memory after this task.
- [ ] Do not scatter run-state writes across unrelated call sites.
- [ ] Do not bypass the transition writer for normal lifecycle transitions.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Workflow runs are durable from creation onward.
- Major transitions are checkpointed.
- The runtime no longer depends on a transient-only run model.

---

## Notes

- This task is the most important runtime pivot in the whole PRD.
