## markdown

## status: pending

<task_context>
<domain>engine/workflows/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task2,task3</dependencies>
</task_context>

# Task 9.0: Initial compozy.db Workflow Core Schema

## Overview

Create the initial `compozy.db` schema for durable workflow execution:
`workflow_run`, `workflow_checkpoint`, and `workflow_signal`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add the first Phase 1 `compozy.db` migrations.
- Cover the minimum fields and indexes already frozen in the baseline docs.
</requirements>

## Subtasks

- [ ] 9.1 Create migration files for `workflow_run`, `workflow_checkpoint`, and `workflow_signal`.
- [ ] 9.2 Add schema tests for creation and idempotent migration.
- [ ] 9.3 Validate index intent against expected read/write paths.

## Implementation Details

This task creates the schema foundation only. Runtime write-path changes belong
to the next task.

### Relevant Files

- new `compozy.db` migration modules
- `tasks/prd-compozy/reset-2026-03-21/DATABASE-SCHEMA.md`
- `tasks/prd-compozy/reset-2026-03-21/INITIAL-RUNTIME-MIGRATIONS.md`

### Dependent Files

- future workflow repositories
- run API handlers

## Deliverables

- Initial `compozy.db` Phase 1 migration set
- Schema coverage tests
- Documented index intent in code comments or migration descriptions

## Tests

### Unit Tests (Required)

- [ ] Migrations create the three Phase 1 workflow tables.
- [ ] Re-running migrations is idempotent.
- [ ] Schema includes the expected columns and key indexes.

### Integration Tests (Required)

- [ ] Fresh `compozy.db` boot applies the workflow-core migration set cleanly.
- [ ] Existing boot path can coexist with an already-migrated `compozy.db`.
- [ ] Migration failure is surfaced clearly during startup.

### Regression and Anti-Pattern Guards

- [ ] No runtime fields are silently omitted from the schema baseline.
- [ ] Do not add Phase 2 or Phase 3 tables into this first workflow-core slice.
- [ ] Do not hardcode schema assumptions only in runtime code without tests.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `compozy.db` has the accepted Phase 1 workflow-core schema.
- Runtime work can start using durable workflow tables immediately after this.

---

## Notes

- Keep this task limited to Phase 1 workflow tables.
