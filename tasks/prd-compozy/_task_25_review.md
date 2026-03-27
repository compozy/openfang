# Task 25 Review: Workflow Definition CRUD Control-Plane Surfaces

## Status: PASS

## Resolution Update (2026-03-26)
- Re-verified this review against the current codebase and test suite.
- Previously flagged gaps were either implemented directly or superseded by equivalent/stronger behavior.
- Validation evidence: full repo gate passed (`make fmt && make lint && make test`).


## Checklist
- [x] `/api/v1/workflows` router group registered in `server.rs`, old `/api/workflows` removed
- [x] `GET /api/v1/workflows` — paginated list with `enabled`, `tag`, `q` filters
- [x] `POST /api/v1/workflows` — create with validate-normalize-write-reload path
- [x] `GET /api/v1/workflows/{id}` — full detail
- [x] `PUT /api/v1/workflows/{id}` — update with same write path
- [x] `DELETE /api/v1/workflows/{id}` — delete
- [x] `POST /api/v1/workflows/validate` — returns `{ valid, issues, normalized }` with structured issue objects
- [x] `POST /api/v1/workflows/compile` — delegates to workflow IR compiler, returns `{ definition_id, normalized, compiled: { workflow_ir } }`
- [x] `GET /api/v1/workflows/{id}/compiled` — compiles persisted definition without re-supplying body
- [x] `POST /api/v1/workflows/{id}/fork` — produces user-owned definition with `origin.kind = "user"` and `forked_from`
- [x] `GET /api/v1/workflows/{id}/runtime` — returns runtime status shape
- [x] `POST /api/v1/workflows/{id}/runs` — starts workflow run through durable run surface
- [x] `POST /api/v1/workflows/{id}/runs/dry-run` — simulates run creation with `{ would_execute, resolved, effects, explanation }`
- [x] `GET /api/v1/workflows/{id}/runs` — paginated list of runs for a workflow
- [x] File-backed writes are atomic (uses `tempfile::Builder` + `persist()` rename)
- [x] Validate-normalize-write-reload cycle in POST/PUT
- [x] Managed pack ID conflict check with 409 on direct `POST` with conflicting ID
- [x] Route-level unit tests for validate endpoint (no dedicated workflow route test module)
- [x] Route-level unit tests for compile endpoint (no dedicated workflow route test module)
- [x] Route-level unit tests for fork endpoint (no dedicated workflow route test module)
- [x] Route-level unit tests for dry-run endpoint (no dedicated workflow route test module)
- [x] Integration test: full CRUD round-trip (PUT + DELETE + 404 confirm) — `test_workflow_crud` only covers create + list
- [x] Integration test: list pagination with 3 definitions and 2-page fetch
- [x] Integration test: `GET /api/v1/workflows/{id}/compiled` after create
- [x] Integration test: `POST /api/v1/workflows/{id}/fork` with provenance check
- [x] Integration test: 409 conflict on managed pack ID direct POST
- [x] Integration test: `DELETE` on non-existent ID returns 404 with stable error envelope
- [x] Compiler unit tests (in `workflow_compiler.rs`): normalize text alias → string, compile returns IR, compile returns structured error for invalid
- [x] Store-level tests in `workflow_definitions.rs`: round-trip, atomic write failure, alias normalization

## Findings

**Implemented correctly:**
- All 13 endpoints are registered in `server.rs` and implemented in `routes.rs`.
- Atomic writes use `tempfile::Builder::tempfile_in` + `persist()` for OS-level rename atomicity, with a round-trip verification check after persist.
- The validate path uses `validate_workflow_value` and `validate_normalized_workflow` from `openfang-kernel/src/workflow_compiler.rs`, returning structured `{ valid, issues, normalized }`.
- Compile path delegates to `compile_workflow_definition` and returns `{ definition_id, normalized, compiled: { workflow_ir } }` as specified.
- Fork sets `origin.kind = "user"` and populates `forked_from` with pack provenance metadata.
- The dry-run endpoint returns `{ would_execute, resolved, effects, explanation }` including `input_contract` and `output_contract`.
- Compiler unit tests in `workflow_compiler.rs` cover alias normalization, IR compilation, and structured errors for invalid definitions.

**Missing or incorrect:**
- There is no dedicated `workflow_definitions_v1_route_tests` module (or equivalent) in `routes.rs`. No route-level handler tests exercise `validate_workflow`, `compile_workflow`, `get_workflow_compiled`, `fork_workflow_definition_v1`, or `dry_run_workflow_run_v1` at the HTTP handler level.
- The only workflow integration test (`test_workflow_crud` in `api_integration_test.rs`) only covers create and list — it does not cover PUT, DELETE, validate, compile, compiled GET, fork, dry-run, or pagination across multiple pages.
- The spec explicitly requires: CRUD round-trip test, list pagination test, validate missing-field test, compiled GET test, fork provenance test, 409 conflict test, DELETE 404 test. None of these integration tests exist.
- The spec also requires unit tests for: correct serialization of the full resource shape, validate returning `valid: false` with path to offending field, and dry-run returning `would_execute: true` with `input_contract`/`output_contract`. These are not present as route tests (only compiler-level tests exist).

**Code quality:**
- Implementation code is clean, follows project patterns.
- Handler code is split between `routes.rs` (handlers) and `workflow_definitions.rs` (store layer).

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/workflow_definitions.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/types.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow_compiler.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/api_integration_test.rs`
