## markdown

## status: pending

<task_context>
<domain>engine/infra/migrations</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task1,task2</dependencies>
</task_context>

# Task 3.0: Reusable Migration Runner For Both Databases

## Overview

Extract or adapt the existing SQLite migration pattern into a reusable runner
for both `runtime.db` and `compozy.db`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Support independent migration streams per database.
- Keep migrations deterministic, ordered, and idempotent.
</requirements>

## Subtasks

- [ ] 3.1 Audit `openfang-memory` migration helpers and isolate the reusable pieces.
- [ ] 3.2 Implement a dual-target migration runner with per-database streams.
- [ ] 3.3 Add migration tests for ordering, idempotency, and failure propagation.

## Implementation Details

The goal is not a generic migration framework for every future use case. The
goal is a reliable runner that matches the accepted dual-database ownership
model.

### Relevant Files

- `crates/openfang-memory/src/migration.rs`
- `crates/openfang-memory/src/lib.rs`
- `crates/openfang-kernel/src/kernel.rs`
- `tasks/prd-compozy/reset-2026-03-21/INITIAL-RUNTIME-MIGRATIONS.md`

### Dependent Files

- future migration modules for `runtime.db`
- future migration modules for `compozy.db`

## Deliverables

- Reusable migration runner for both databases
- Per-database migration stream structure
- Tests for ordering and idempotency

## Tests

### Unit Tests (Required)

- [ ] Migration runner applies versions in order.
- [ ] Migration runner is idempotent for already-applied migrations.
- [ ] Migration runner reports and surfaces failing migrations correctly.

### Integration Tests (Required)

- [ ] Boot can apply `runtime.db` and `compozy.db` migrations independently.
- [ ] Partial migration failure stops startup clearly.
- [ ] Fresh database boot produces the expected migration metadata in both DBs.

### Regression and Anti-Pattern Guards

- [ ] Do not duplicate migration logic once per database with copy-paste drift.
- [ ] Do not hide migration failure and continue boot in a broken state.
- [ ] Do not rely on manual operator steps to initialize schema.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- The fork has a stable migration runner for both databases.
- Later schema tasks can add migrations without inventing new infra.
- Migration behavior is deterministic in tests and startup.

---

## Notes

- The current `openfang-memory` migration code is the reference, not the final shape.
