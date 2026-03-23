## markdown

## status: pending

<task_context>
<domain>engine/workflows/recovery</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task6,task14,task15</dependencies>
</task_context>

# Task 17.0: Restart Recovery And Durable Run Control Surfaces

## Overview

Implement conservative restart recovery for durable runs and align the run
control-plane surfaces with the new stored state.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Downgrade unrecoverable in-flight `running` state to `paused` on restart.
- Align list/detail/checkpoints/signals endpoints with durable state.
</requirements>

## Subtasks

- [ ] 17.1 Implement startup recovery scan for Phase 1 statuses.
- [ ] 17.2 Add recovery checkpoints such as `run_recovered_needs_resume`.
- [ ] 17.3 Route run control-plane read surfaces to durable repositories.

## Implementation Details

This is the first recovery-hardening layer, not the final auto-resume model.

### Relevant Files

- `crates/openfang-kernel/src/kernel.rs`
- `crates/openfang-kernel/src/workflow.rs`
- `crates/openfang-api/src/routes.rs`
- `tasks/prd-compozy/reset-2026-03-21/INITIAL-RUNTIME-MIGRATIONS.md`

### Dependent Files

- API integration tests
- daemon boot tests

## Deliverables

- Conservative startup recovery
- Durable run control-plane surfaces
- Tests for restart and visibility behavior

## Tests

### Unit Tests (Required)

- [ ] `running` runs downgrade to `paused` on restart as designed.
- [ ] Waiting runs remain waiting across restart.
- [ ] Recovery checkpoints are emitted correctly.

### Integration Tests (Required)

- [ ] Restart after in-flight run preserves durable state and exposes pause-for-resume behavior.
- [ ] Run list/detail/checkpoints endpoints reflect recovered durable state.
- [ ] Signal and checkpoint history remain intact after restart.

### Regression and Anti-Pattern Guards

- [ ] Do not auto-resume arbitrary in-flight execution in Phase 1.
- [ ] Do not keep API reads pointed at in-memory state after durability lands.
- [ ] Do not hide recovery decisions from checkpoint history.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Restart no longer loses workflow run identity or waiting state.
- Run control-plane surfaces are backed by durable data.
- Recovery behavior is explicit and testable.

---

## Notes

- This task finishes the first durable workflow-core slice.
