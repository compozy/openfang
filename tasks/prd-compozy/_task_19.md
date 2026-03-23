## markdown

## status: pending

<task_context>
<domain>engine/dispatch/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task9,task17,task18</dependencies>
</task_context>

# Task 19.0: agent_dispatch Schema And Persistence Layer

## Overview

Add the durable schema and persistence layer for `agent_dispatch`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add the accepted `agent_dispatch` table and repository.
- Support lifecycle metadata needed for later runtime integration.
</requirements>

## Subtasks

- [ ] 19.1 Add `agent_dispatch` migrations.
- [ ] 19.2 Implement dispatch repository operations.
- [ ] 19.3 Add tests for status transitions and parent/child linkage.

## Implementation Details

Schema and repository first. Runtime integration happens in later tasks.

### Relevant Files

- `tasks/prd-compozy/reset-2026-03-21/DATABASE-SCHEMA.md`
- new `compozy.db` migration files
- new dispatch repository modules

### Dependent Files

- dispatch runtime integration
- dispatch control-plane handlers

## Deliverables

- `agent_dispatch` schema
- repository layer
- tests for lifecycle storage

## Tests

### Unit Tests (Required)

- [ ] Dispatch records persist all required fields.
- [ ] Parent-child dispatch linkage persists correctly.
- [ ] Status transitions are represented durably.

### Integration Tests (Required)

- [ ] `compozy.db` migrations add dispatch tables cleanly.
- [ ] Dispatch repository works against fresh and migrated databases.
- [ ] Stored dispatch records remain queryable after restart.

### Regression and Anti-Pattern Guards

- [ ] Do not encode dispatch state only as workflow checkpoint payloads.
- [ ] Do not skip parent-child lineage fields for convenience.
- [ ] Do not let dispatch lifecycle depend on runtime-only memory structures.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Dispatch persistence exists as a first-class runtime layer.
- Later runtime integration can build on durable dispatch storage.

---

## Notes

- This is the first half of dispatch durability.
