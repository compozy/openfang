## markdown

## status: pending

<task_context>
<domain>engine/workflows/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task13,task17</dependencies>
</task_context>

# Task 21.0: Workflow Definition CRUD Control-Plane Surfaces

## Overview

Implement the workflow definition CRUD API surfaces. Users can create, read,
update, delete, validate, compile, and fork workflow definitions through the
API. Workflow definitions are file-backed (following the config-first storage
model), so writes go through the validate-normalize-write-reload path. The
compile endpoint returns the workflow IR from task 13.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Full CRUD for workflow definitions via API.
- Validate and compile endpoints return useful diagnostics.
- Fork creates a new definition from an existing one with proper provenance tracking.
</requirements>

## Subtasks

- [ ] 21.1 Implement GET/POST/PUT/DELETE endpoints for workflow definitions at `/api/workflows`.
- [ ] 21.2 Implement `/api/workflows/{id}/validate`, `/api/workflows/{id}/compile`, and `/api/workflows/{id}/fork` endpoints.
- [ ] 21.3 Add tests for all CRUD operations, validation errors, compile output, and fork behavior.

## Implementation Details

Workflow definitions are file-backed. Writes go through a
validate-normalize-write-reload path to ensure consistency. The compile
endpoint delegates to the workflow IR compiler from task 13.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `crates/openfang-kernel/src/workflow.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- `crates/openfang-types/src/workflow.rs` (from task 13)

## Deliverables

- Workflow definition CRUD endpoints
- Validate/compile/fork endpoints
- Tests for all operations

## Tests

### Unit Tests (Required)

- [ ] CRUD payloads serialize/deserialize correctly.
- [ ] Validate returns actionable errors.
- [ ] Compile returns valid IR.

### Integration Tests (Required)

- [ ] E2E create-read-update-delete flow.
- [ ] Compile output matches expected IR.
- [ ] Fork preserves provenance.

### Regression and Anti-Pattern Guards

- [ ] No endpoint returns empty or null for valid definitions.
- [ ] File-backed writes are atomic and consistent.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Workflow definitions are fully manageable through the API.
- Validate and compile endpoints provide useful feedback.
- Fork creates traceable copies with provenance.

---

## Notes

- Use `tasks/prd-compozy/reset-2026-03-21/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
