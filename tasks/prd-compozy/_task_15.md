## markdown

## status: pending

<task_context>
<domain>engine/workflow/api</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task14</dependencies>
</task_context>

# Task 15.0: Workflow v2 API Endpoints

## Overview

Wire the Workflow v2 compile pipeline (Task 14) to the public API surface.
This task implements the three workflow API endpoints specified in API-SPEC.md
section 4:

- `POST /api/v1/workflows/validate` — runs the validation phase and returns
  issues without compiling.
- `POST /api/v1/workflows/compile` — runs the full validate-normalize-compile
  pipeline and returns the compiled `WorkflowIr`.
- `GET /api/v1/workflows/{id}/compiled` — returns the cached `WorkflowIr` for
  an already-registered workflow.

These endpoints follow the payload conventions from ADR-034 (canonical
control-plane payload conventions) and the validate/compile semantics from
ADR-038 (validate, compile, dry-run, and explain semantics). The endpoints
are registered in `crates/openfang-api/src/server.rs` under the `/api/v1/`
prefix per ADR-032 (workflow API definition and operational surfaces).

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- `POST /api/v1/workflows/validate` must accept a `WorkflowV2Definition` JSON body and return `{ valid, issues, normalized }` per ADR-034 and ADR-038. The `issues` array must contain `ValidationIssue` objects with `severity`, `code`, `path`, and `message`.
- `POST /api/v1/workflows/compile` must accept a `WorkflowV2Definition` JSON body, run the full pipeline from Task 14, and return `{ definition_id, normalized, compiled: { workflow_ir } }` per API-SPEC.md section 4.
- `GET /api/v1/workflows/{id}/compiled` must return the cached `WorkflowIr` for a previously registered workflow, without re-running the compile pipeline, and return `404` if the workflow ID is unknown.
- All error responses must use the `{ error: { code, message, details } }` envelope from API-SPEC.md per ADR-034.
- All routes must be registered in `crates/openfang-api/src/server.rs` under the `/api/v1/workflows` prefix.
</requirements>

## Subtasks

- [ ] 15.1 Add request and response types to `crates/openfang-api/src/types.rs`: `WorkflowValidateRequest`, `WorkflowValidateResponse`, `WorkflowCompileRequest`, `WorkflowCompileResponse`, `WorkflowCompiledResponse`. All structs must match the API-SPEC.md payload shapes exactly.
- [ ] 15.2 Implement `validate_workflow` route handler in `crates/openfang-api/src/routes.rs`: accepts a `WorkflowV2Definition` JSON body, calls the Task 14 validate phase, and returns the validation response.
- [ ] 15.3 Implement `compile_workflow` route handler: accepts a `WorkflowV2Definition` JSON body, calls the full Task 14 pipeline, and returns the compile response with `WorkflowIr`.
- [ ] 15.4 Implement `get_workflow_compiled` route handler for `GET /api/v1/workflows/{id}/compiled`: loads the stored definition, returns the cached compiled IR, or returns `404` for unknown IDs.
- [ ] 15.5 Register all new routes in `crates/openfang-api/src/server.rs` under the `/api/v1/workflows` prefix.
- [ ] 15.6 Write integration tests for all three endpoints.

## Implementation Details

### Route Registration

All three routes must be registered in `crates/openfang-api/src/server.rs`
inside the `build_router` function. The routes live under `/api/v1/workflows`:

```
POST /api/v1/workflows/validate       -> validate_workflow
POST /api/v1/workflows/compile        -> compile_workflow
GET  /api/v1/workflows/{id}/compiled  -> get_workflow_compiled
```

### Request/Response Shapes

**Validate endpoint:**
- Request: `{ definition: WorkflowV2Definition, strict: Option<bool>, context: Option<ValidationContext> }`
- Response (valid): `{ valid: true, issues: [], normalized: { ... } }`
- Response (invalid): `{ valid: false, issues: [{ severity, code, path, message }, ...] }`

**Compile endpoint:**
- Request: `{ definition: WorkflowV2Definition, context: Option<CompileContext> }`
- Response: `{ definition_id: "...", normalized: { ... }, compiled: { workflow_ir: { ... } } }`

**Get compiled endpoint:**
- Response: `{ definition_id: "...", compiled: { workflow_ir: { ... } } }`
- Error: `{ error: { code: "not_found", message: "Workflow not found", details: null } }`

### Relevant Files

- `crates/openfang-api/src/routes.rs` — add new handlers
- `crates/openfang-api/src/server.rs` — register new routes in build_router
- `crates/openfang-api/src/types.rs` — add new request/response types
- `tasks/prd-compozy/docs/API-SPEC.md` — section 4 (Workflows: endpoints, compiled response, run creation)
- `tasks/prd-compozy/docs/adrs/032-workflow-api-definition-and-operational-surfaces.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`

## Deliverables

- `POST /api/v1/workflows/validate`, `POST /api/v1/workflows/compile`, and `GET /api/v1/workflows/{id}/compiled` API endpoints per API-SPEC.md.
- Request and response types in `crates/openfang-api/src/types.rs`.
- Route registrations in `crates/openfang-api/src/server.rs`.
- Integration tests for all three endpoints.

## Tests

### Integration Tests (Required)

- [ ] `post_validate_returns_valid_true_for_correct_definition`: `POST /api/v1/workflows/validate` with a valid SDLC-like definition must return `{"valid": true, "issues": [], "normalized": {...}}`.
- [ ] `post_validate_returns_issues_for_dangling_reference`: `POST /api/v1/workflows/validate` with an `outputs` dangling reference must return `{"valid": false, "issues": [{"severity": "error", "code": "dangling_reference", ...}]}`.
- [ ] `post_compile_returns_workflow_ir`: `POST /api/v1/workflows/compile` with a valid definition must return `{"definition_id": "...", "normalized": {...}, "compiled": {"workflow_ir": {...}}}`.
- [ ] `get_compiled_returns_cached_ir_for_registered_workflow`: after registering a workflow via `POST /api/v1/workflows`, `GET /api/v1/workflows/{id}/compiled` must return the pre-compiled IR without re-running the pipeline.
- [ ] `get_compiled_returns_404_for_unknown_id`: `GET /api/v1/workflows/{unknown_id}/compiled` must return a `404` response with the standard error envelope.
- [ ] `end_to_end_definition_to_ir_preserves_step_semantics`: a definition with all eight step kinds must produce an IR where each step's kind, uses, save_as, and flow mode are faithfully preserved.

### Regression and Anti-Pattern Guards

- [ ] All error responses use the `{ error: { code, message, details } }` envelope — no unstructured plain-text error bodies.
- [ ] The validate endpoint does not compile — it only validates and normalizes.
- [ ] The compile endpoint returns an error (not a partial IR) when validation fails.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `POST /api/v1/workflows/validate`, `POST /api/v1/workflows/compile`, and `GET /api/v1/workflows/{id}/compiled` are implemented and return the payload shapes specified in API-SPEC.md section 4.
- All error responses follow the standard envelope format.
- All routes are registered in `server.rs` and return non-`404` responses for valid inputs.
- `cargo test --workspace` passes with zero failures and `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.

---

## Notes

- Live integration testing (starting the daemon and curling real endpoints) is mandatory per `CLAUDE.md` before marking this task complete.
- The internal `AppState` in `crates/openfang-api/src/routes.rs` must not hold a direct reference to the compile pipeline functions — route handlers must call the pipeline functions from the Task 14 module by importing them; `AppState` provides only the kernel and shared state.
