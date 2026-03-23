## markdown

## status: pending

<task_context>
<domain>engine/infra/persistence</domain>
<type>implementation</type>
<scope>configuration</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 1.0: Split Persistence Config For Dual Databases

## Overview

Split the current single SQLite persistence configuration into explicit
`runtime.db` and `compozy.db` configuration paths and ownership boundaries.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Replace the current single-DB configuration assumption without breaking the existing boot path abruptly.
- Define stable config fields for `runtime.db` and `compozy.db` that later tasks can rely on.
</requirements>

## Subtasks

- [ ] 1.1 Inspect current config loading and identify all call sites that assume `memory.sqlite_path`.
- [ ] 1.2 Introduce explicit config fields and defaults for `runtime.db` and `compozy.db`.
- [ ] 1.3 Add config validation and tests for the new persistence split.

## Implementation Details

This task only defines the config shape and ownership contract. It should not
yet implement the full bootstrap or migration flow.

### Relevant Files

- `crates/openfang-types/src/config.rs`
- `crates/openfang-kernel/src/kernel.rs`
- `tasks/prd-compozy/reset-2026-03-21/STORAGE-MODEL.md`
- `tasks/prd-compozy/reset-2026-03-21/IMPLEMENTATION-PLAN.md`

### Dependent Files

- `crates/openfang-cli/src/main.rs`
- `crates/openfang-api/src/server.rs`

## Deliverables

- Dual-database config fields with clear defaults
- Validation rules for the new persistence config
- Tests covering config parsing and default resolution

## Tests

### Unit Tests (Required)

- [ ] Config parsing accepts explicit `runtime.db` and `compozy.db` paths.
- [ ] Config defaults resolve both database paths when the user omits them.
- [ ] Invalid persistence config reports actionable validation errors.

### Integration Tests (Required)

- [ ] Existing boot config fixtures continue to load with migration-safe defaults.
- [ ] CLI or daemon startup can resolve the new config shape without panic.
- [ ] Legacy single-path setups are mapped or rejected consistently, per design.

### Regression and Anti-Pattern Guards

- [ ] No hidden fallback keeps `memory.sqlite_path` as the de facto source of truth.
- [ ] Config changes do not silently reintroduce a single-DB assumption elsewhere.
- [ ] No test-only config branches are added.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- The fork has an explicit dual-database config model.
- Later bootstrap and migration tasks can rely on stable config fields.
- Existing startup flows remain compatible or fail clearly.

---

## Notes

- Use `tasks/prd-compozy/reset-2026-03-21/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
