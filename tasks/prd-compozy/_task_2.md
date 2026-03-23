## markdown

## status: pending

<task_context>
<domain>engine/infra/bootstrap</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task1</dependencies>
</task_context>

# Task 2.0: Dual-Database Bootstrap In Kernel Startup

## Overview

Make kernel startup open, own, and expose both `runtime.db` and `compozy.db`
before dependent subsystems are constructed.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Kernel boot must no longer assume one shared SQLite store for everything.
- Database handles and ownership boundaries must be visible early enough for later runtime tasks.
</requirements>

## Subtasks

- [ ] 2.1 Refactor boot sequencing to open both databases before subsystem construction.
- [ ] 2.2 Introduce kernel-owned handles or services for `runtime.db` and `compozy.db`.
- [ ] 2.3 Surface bootstrap failures clearly to daemon, CLI, and tests.

## Implementation Details

This task should change startup order, not the durable workflow model itself.
It is the earliest wiring needed for later migrations and repositories.

### Relevant Files

- `crates/openfang-kernel/src/kernel.rs`
- `crates/openfang-api/src/server.rs`
- `crates/openfang-cli/src/main.rs`
- `tasks/prd-compozy/reset-2026-03-21/INITIAL-RUNTIME-MIGRATIONS.md`

### Dependent Files

- `crates/openfang-runtime/*`
- `crates/openfang-memory/*`

## Deliverables

- Kernel boot path that owns both databases
- Clear bootstrap error handling
- Tests for dual-database startup behavior

## Tests

### Unit Tests (Required)

- [ ] Boot logic fails clearly when either database path cannot be opened.
- [ ] Boot logic initializes both handles in the expected order.
- [ ] Internal state exposes both databases where later tasks need them.

### Integration Tests (Required)

- [ ] Daemon startup succeeds with both databases absent and creates them.
- [ ] CLI startup succeeds with the new dual-database bootstrap.
- [ ] Test harnesses can still construct the kernel without bespoke hacks.

### Regression and Anti-Pattern Guards

- [ ] No subsystem still reaches into the old single-DB bootstrap path implicitly.
- [ ] Boot does not hide partial-failure states behind degraded success.
- [ ] No global singleton is introduced to avoid wiring the second DB correctly.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Both databases are first-class bootstrap resources.
- Startup order is stable and ready for migrations.
- Later persistence tasks can hook into boot without more structural rewrites.

---

## Notes

- This task is the earliest structural prerequisite for durable workflow work.
