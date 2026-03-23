## markdown

## status: pending

<task_context>
<domain>engine/workflows/definitions</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task2</dependencies>
</task_context>

# Task 7.0: Workflow Definition Source-Of-Truth Consistency

## Overview

Fix the current inconsistency between in-memory workflow state and file-backed
workflow definitions so restart cannot resurrect stale workflow definitions.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Create, update, and delete must keep filesystem state and runtime state coherent.
- Restart must not reload stale workflow definitions from disk.
</requirements>

## Subtasks

- [ ] 7.1 Audit create/update/delete workflow paths for file/runtime drift.
- [ ] 7.2 Make mutation paths consistently persist canonical definitions.
- [ ] 7.3 Add tests for restart behavior after workflow mutation.

## Implementation Details

This task is about definition correctness, not durable `workflow_run` yet.
It needs to land early because Phase 1 run durability should not sit on top of
definition inconsistency.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-kernel/src/kernel.rs`
- `tasks/prd-compozy/reset-2026-03-21/STORAGE-MODEL.md`
- `tasks/prd-compozy/reset-2026-03-21/DESIGN.md`

### Dependent Files

- workflow definition loaders
- workflow tests under `openfang-api` or `openfang-kernel`

## Deliverables

- Consistent workflow definition mutation behavior
- Restart-safe definition reload semantics
- Regression tests for stale-definition recovery

## Tests

### Unit Tests (Required)

- [ ] Workflow update persists the new canonical definition.
- [ ] Workflow delete removes or invalidates the file-backed definition correctly.
- [ ] Runtime registry and file store stay aligned after mutation.

### Integration Tests (Required)

- [ ] Create -> restart -> reload returns the same definition.
- [ ] Update -> restart -> reload reflects the updated definition.
- [ ] Delete -> restart does not resurrect removed workflows.

### Regression and Anti-Pattern Guards

- [ ] No code path mutates only memory while leaving disk stale.
- [ ] No code path mutates only disk while leaving runtime stale.
- [ ] Do not add a database definition source of truth for workflows.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Workflow definition lifecycle is coherent across memory, disk, and restart.
- Later run-durability work can assume stable definition loading.

---

## Notes

- This task is a prerequisite for trustworthy workflow bootstrap and run persistence.
