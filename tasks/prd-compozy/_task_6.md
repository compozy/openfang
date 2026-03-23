## markdown

## status: pending

<task_context>
<domain>engine/infra/runtime-db</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task2,task3</dependencies>
</task_context>

# Task 6.0: Initial runtime.db Schema And Stores

## Overview

Create the first `runtime.db` migration stream and wire the initial runtime
stores for agents and schedules.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add the first runtime tables without regressing current agent/session/schedule behavior.
- Keep table ownership aligned with `STORAGE-MODEL.md`.
</requirements>

## Subtasks

- [ ] 6.1 Add the initial `runtime.db` migrations for runtime surfaces.
- [ ] 6.2 Wire repositories or store adapters for those tables.
- [ ] 6.3 Align boot and health paths with the new runtime store ownership.

## Implementation Details

This task should cover only the initial runtime tables, not product-domain or
workflow-durability tables.

### Relevant Files

- `crates/openfang-memory/src/migration.rs`
- `crates/openfang-kernel/src/kernel.rs`
- `tasks/prd-compozy/reset-2026-03-21/DATABASE-SCHEMA.md`
- `tasks/prd-compozy/reset-2026-03-21/INITIAL-RUNTIME-MIGRATIONS.md`

### Dependent Files

- `crates/openfang-memory/src/session.rs`
- `crates/openfang-memory/src/structured.rs`
- `crates/openfang-api/src/server.rs`

## Deliverables

- Initial `runtime.db` migration set
- Runtime-store adapters for agents and schedules
- Tests validating runtime DB ownership

## Tests

### Unit Tests (Required)

- [ ] `runtime.db` tables are created and versioned correctly.
- [ ] Agent/session/schedule stores can read and write against the new schema.
- [ ] Ownership boundaries reject product-domain leakage into `runtime.db`.

### Integration Tests (Required)

- [ ] Boot creates and migrates `runtime.db` end-to-end.
- [ ] Agent runtime behavior still works after the storage split.
- [ ] Schedule runtime metadata persists through restart.

### Regression and Anti-Pattern Guards

- [ ] No task/workflow-domain tables are introduced into `runtime.db`.
- [ ] Existing runtime features do not silently fall back to unrelated stores.
- [ ] No temporary duplication becomes a second long-lived source of truth.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `runtime.db` exists with the intended initial schema.
- Core runtime surfaces can use it without ambiguity.
- The fork is ready to add `compozy.db` runtime durability next.

---

## Notes

- Keep this task scoped to platform runtime ownership only.
