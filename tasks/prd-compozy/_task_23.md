## markdown

## status: pending

<task_context>
<domain>domain/tasks/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task17</dependencies>
</task_context>

# Task 23.0: Task And Subtask Domain Schema And Repositories

## Overview

Create the `task` and `subtask` domain schema and repositories with the richer
shape accepted in the baseline.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `task` and `subtask` tables with the approved field set.
- Add repository access aligned with domain ownership in `compozy.db`.
</requirements>

## Subtasks

- [ ] 23.1 Add migrations for `task` and `subtask`.
- [ ] 23.2 Implement repositories for task and subtask lifecycle operations.
- [ ] 23.3 Add tests for refs, dependencies, and durable identity.

## Implementation Details

The `task` schema should include refs for artifacts, docs, files,
repositories, and labels. The `subtask` schema should carry local execution
state, not the full task context.

### Relevant Files

- new `compozy.db` migration files
- new task/subtask repository modules
- `tasks/prd-compozy/reset-2026-03-21/DATABASE-SCHEMA.md`

### Dependent Files

- task/subtask control-plane handlers
- looper runtime integration

## Deliverables

- `task` schema and repository
- `subtask` schema and repository
- Tests for durable domain behavior

## Tests

### Unit Tests (Required)

- [ ] Task records persist the approved domain fields and refs.
- [ ] Subtask records persist dependencies, assignee info, and local execution fields.
- [ ] Repository operations preserve task identity across subtask changes.

### Integration Tests (Required)

- [ ] Fresh and migrated databases accept the task/subtask schema cleanly.
- [ ] Task and subtask records survive restart and remain queryable.
- [ ] Source-run linkage behaves correctly where present.

### Regression and Anti-Pattern Guards

- [ ] Do not route canonical task/subtask state through the old OpenFang task queue.
- [ ] Do not reduce task context to only workflow runtime variables.
- [ ] Do not hide subtask dependency structure inside opaque blobs with no tests.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `task` and `subtask` exist durably in `compozy.db`.
- Later control-plane and looper tasks can use them directly.

---

## Prior Implementation Reference

The old TypeScript codebase has the prior task/subtask schema (called "issues" in the old model):

- `~/Dev/compozy/compozy-code/packages/backend/src/db/schema/tasks.ts` — Old `tasks` + `subtasks` table definitions with statuses, complexities, position ordering, and indexes
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/tasks/` — Task CRUD use cases and routes
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/subtasks/` — Subtask model and repository

Key differences from the old model:
- Old subtask has 3-value status; new model adds `depends_on`, `parallelizable`, `assignee`
- Old "issue" concept → renamed to "task" (see ADR-047)
- Old model uses PostgreSQL (Drizzle ORM); new model uses SQLite (`compozy.db`)

## Notes

- This task introduces the product-domain work model for real.
