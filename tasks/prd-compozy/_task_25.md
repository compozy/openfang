## markdown

## status: pending

<task_context>
<domain>engine/hitl/runtime</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task20,task24</dependencies>
</task_context>

# Task 25.0: HITL Mid-Step Pause And Resume Integration

## Overview

Implement HITL pause/resume in the middle of an active agent step, tied to the
same durable dispatch.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Pause the active dispatch on HITL.
- Resume the same dispatch after response.
</requirements>

## Subtasks

- [ ] 25.1 Implement runtime pause behavior when a HITL request is created.
- [ ] 25.2 Implement resume behavior after HITL response.
- [ ] 25.3 Add tests for repeated HITL interactions in one active step.

## Implementation Details

This task should preserve the accepted distinction between in-step HITL and
workflow-level `wait_signal`.

### Relevant Files

- dispatch runtime modules
- HITL repository/runtime modules
- workflow runtime integration points
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- HITL control-plane handlers
- restart/recovery tests

## Deliverables

- In-step HITL runtime behavior
- Resume path for the same dispatch
- Tests for multiple question/answer cycles

## Tests

### Unit Tests (Required)

- [ ] Creating a HITL request pauses the active dispatch correctly.
- [ ] Answering a HITL request resumes the same dispatch correctly.
- [ ] Sequence numbering and repeat interactions work within one step.

### Integration Tests (Required)

- [ ] End-to-end agent step with clarification question resumes correctly.
- [ ] Restart during HITL preserves pending request and resumable dispatch state.
- [ ] HITL does not incorrectly advance the workflow to another step.

### Regression and Anti-Pattern Guards

- [ ] Do not implement clarification as a separate synthetic workflow step.
- [ ] Do not lose provider/session context across HITL pause/resume.
- [ ] Do not conflate `wait_signal` with HITL semantics.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- HITL works mid-step with the same dispatch identity.
- Workflow and dispatch state remain coherent through pause and resume.

---

## Prior Implementation Reference

The old TypeScript codebase implements the clarification/HITL flow end-to-end:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/clarification/` — Clarification tool showing pause/resume interaction patterns
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/hitl/` — Frontend HITL interaction system

The old model handled HITL as a tool-level concern within the provider. The new model makes it a
durable runtime concept — the old code is useful for understanding the user-facing pause/resume UX
and how question/answer cycles interleave with active execution.

## Notes

- This task validates one of the core product requirements directly.
