## markdown

## status: pending

<task_context>
<domain>engine/hitl/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task9,task17</dependencies>
</task_context>

# Task 20.0: hitl_request Schema And Persistence Layer

## Overview

Add the durable schema and persistence layer for `hitl_request`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add the accepted `hitl_request` table and repository.
- Support sequencing and links to run, step, and optional dispatch.
</requirements>

## Subtasks

- [ ] 20.1 Add `hitl_request` migrations.
- [ ] 20.2 Implement request repository operations.
- [ ] 20.3 Add storage tests for status changes and sequencing.

## Implementation Details

This task covers durable HITL state only. Runtime semantics come after.

### Relevant Files

- `tasks/prd-compozy/reset-2026-03-21/DATABASE-SCHEMA.md`
- new `compozy.db` migration files
- new HITL repository modules

### Dependent Files

- HITL runtime integration
- HITL API handlers

## Deliverables

- `hitl_request` schema
- repository layer
- tests for lifecycle storage

## Tests

### Unit Tests (Required)

- [ ] HITL requests persist run/step/dispatch linkage correctly.
- [ ] Sequence numbering persists correctly for repeated prompts in one step.
- [ ] Status and answer timestamps update durably.

### Integration Tests (Required)

- [ ] HITL schema migrates cleanly into `compozy.db`.
- [ ] HITL requests remain queryable after restart.
- [ ] Repository behavior matches the accepted public shape.

### Regression and Anti-Pattern Guards

- [ ] Do not model HITL with the old approval subsystem.
- [ ] Do not bury HITL state only in dispatch payloads.
- [ ] Do not lose the linkage needed for in-step resume.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- HITL state exists durably as its own runtime concept.
- Later runtime integration can pause and resume against stable storage.

---

## Prior Implementation Reference

The old TypeScript codebase implements HITL as a "clarification" tool:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/clarification/` — HITL/clarification tool implementation showing pause/resume patterns

In the old model, clarification was a tool-level concern. In the new model, `hitl_request` is a
first-class durable runtime concept with its own table, status lifecycle, and linkage to run/step/dispatch.
The old code is useful for understanding the user-facing interaction patterns and question/answer flow.

## Notes

- This is the storage half of HITL, not the full runtime behavior.
