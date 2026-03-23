## markdown

## status: pending

<task_context>
<domain>engine/schedules/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task6,task17</dependencies>
</task_context>

# Task 22.0: Schedule Control-Plane Surfaces

## Overview

Implement the schedule definition CRUD and operational API surfaces. Schedules
are persisted in `runtime.db` (schema from task 6). The API provides CRUD,
validate, fork, enable/disable, and run-now endpoints. Schedules connect cron
expressions to workflow or agent targets.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Full schedule CRUD via API.
- Enable/disable controls schedule activation.
- Run-now triggers immediate execution.
- Validation catches invalid cron expressions and target references.
</requirements>

## Subtasks

- [ ] 22.1 Implement GET/POST/PUT/DELETE endpoints for schedule definitions at `/api/schedules`.
- [ ] 22.2 Implement `/api/schedules/{id}/enable`, `/api/schedules/{id}/disable`, `/api/schedules/{id}/run-now` endpoints.
- [ ] 22.3 Add tests for CRUD, enable/disable state transitions, run-now behavior, and validation errors.

## Implementation Details

Schedules are persisted in `runtime.db`. The typed cron model from ADR 035
ensures cron expressions are validated at write time. Enable/disable toggles
the active state without deleting the schedule. Run-now creates an immediate
execution bypass of the cron schedule.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`
- `tasks/prd-compozy/reset-2026-03-21/adrs/035-schedule-api-surface-on-typed-cron-model.md`

### Dependent Files

- `crates/openfang-kernel/src/kernel.rs`
- `crates/openfang-types/src/config.rs`

## Deliverables

- Schedule CRUD endpoints
- Enable/disable/run-now operational endpoints
- Tests for all operations

## Tests

### Unit Tests (Required)

- [ ] Cron expression validation.
- [ ] Target reference resolution.
- [ ] Enable/disable state transitions.

### Integration Tests (Required)

- [ ] E2E schedule lifecycle (create, enable, run-now, disable, delete).
- [ ] Regression: disabled schedules never fire.
- [ ] Run-now respects target validation.

### Regression and Anti-Pattern Guards

- [ ] Disabled schedules never fire.
- [ ] Run-now respects target validation.
- [ ] Invalid cron expressions are rejected at write time, not at fire time.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Schedules are fully manageable through the API.
- Enable/disable and run-now provide operational control.
- Validation catches errors early and returns actionable diagnostics.

---

## Notes

- Use `tasks/prd-compozy/reset-2026-03-21/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
