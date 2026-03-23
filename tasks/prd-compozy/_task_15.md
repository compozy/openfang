## markdown

## status: pending

<task_context>
<domain>engine/workflows/signals</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task9,task14</dependencies>
</task_context>

# Task 15.0: Workflow Signal Persistence And Waiting-State Integration

## Overview

Persist workflow-level signals and wire them into the durable waiting-state
model for workflow runs.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Persist signals independently of in-memory delivery.
- Integrate signal delivery with durable `waiting_signal` transitions.
</requirements>

## Subtasks

- [ ] 15.1 Implement `workflow_signal` repository operations.
- [ ] 15.2 Wire waiting-state transitions to persisted signals.
- [ ] 15.3 Add list/detail/consume tests for durable signal behavior.

## Implementation Details

This task should cover signal persistence and waiting-state semantics only, not
the full recovery policy yet.

### Relevant Files

- `crates/openfang-kernel/src/workflow.rs`
- `crates/openfang-kernel/src/kernel.rs`
- `crates/openfang-api/src/routes.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- run control-plane handlers
- scheduler/trigger integrations that later emit signals

## Deliverables

- Durable signal repository
- Waiting-state integration
- Tests for signal submission and consumption

## Tests

### Unit Tests (Required)

- [ ] Signal insertion persists the expected payload and source.
- [ ] Waiting runs accept and consume compatible signals correctly.
- [ ] Consumed flags and timestamps update as expected.

### Integration Tests (Required)

- [ ] `POST /api/runs/{id}/signals` persists and affects run state correctly.
- [ ] Waiting workflow resumes correctly after durable signal delivery.
- [ ] Restart preserves waiting state and outstanding signals.

### Regression and Anti-Pattern Guards

- [ ] Do not process workflow signals only in memory.
- [ ] Do not bypass run waiting-state persistence when signal delivery occurs.
- [ ] Do not overfit signal handling to one trigger or schedule path.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Signals are durable.
- Waiting-state semantics are durable.
- Future trigger and schedule work can target signals cleanly.

---

## Notes

- This task unlocks explicit workflow continuation semantics.
