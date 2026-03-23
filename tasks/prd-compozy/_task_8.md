## markdown

## status: pending

<task_context>
<domain>engine/workflows/bootstrap</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task2,task7</dependencies>
</task_context>

# Task 8.0: Workflow Bootstrap And Readiness Semantics

## Overview

Replace the current loose background workflow autoload behavior with explicit
bootstrap and readiness semantics appropriate for the new durable product.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Make workflow bootstrap timing explicit.
- Define what daemon/API readiness means relative to workflow loading.
</requirements>

## Subtasks

- [ ] 8.1 Audit the current background autoload path and readiness assumptions.
- [ ] 8.2 Make workflow loading order explicit during startup.
- [ ] 8.3 Add readiness tests for workflow availability after boot.

## Implementation Details

This task is about startup semantics and service readiness, not workflow run
durability itself.

### Relevant Files

- `crates/openfang-kernel/src/kernel.rs`
- `crates/openfang-api/src/server.rs`
- `tasks/prd-compozy/reset-2026-03-21/INITIAL-RUNTIME-MIGRATIONS.md`

### Dependent Files

- workflow loader helpers
- daemon bootstrap tests

## Deliverables

- Explicit workflow bootstrap behavior
- Readiness semantics that reflect actual workflow availability
- Tests for boot ordering and startup guarantees

## Tests

### Unit Tests (Required)

- [ ] Workflow bootstrap order is explicit and deterministic.
- [ ] Readiness logic matches the new bootstrap semantics.
- [ ] Workflow loading errors fail or degrade startup in a documented way.

### Integration Tests (Required)

- [ ] API/daemon startup exposes readiness only after expected workflow bootstrap.
- [ ] Restart with existing workflow files yields a stable registry.
- [ ] Broken workflow files surface startup behavior consistently.

### Regression and Anti-Pattern Guards

- [ ] No hidden background loader remains the only source of workflow truth.
- [ ] Do not solve readiness with sleeps or timing hacks.
- [ ] Do not accept API success before definitions are actually available.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Workflow bootstrap timing is explicit.
- Readiness reflects real availability.
- Later durable-run work builds on stable startup semantics.

---

## Notes

- This task protects later Phase 1 work from boot-order ambiguity.
