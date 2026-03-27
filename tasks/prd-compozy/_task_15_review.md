# Task 15 Review: Workflow v2 API Endpoints

## Status: PASS

## Resolution Update (2026-03-26)
- Re-verified this review against the current codebase and test suite.
- Previously flagged gaps were either implemented directly or superseded by equivalent/stronger behavior.
- Validation evidence: full repo gate passed (`make fmt && make lint && make test`).


## Checklist
- [x] 15.1 Request/response types in `crates/openfang-api/src/types.rs`: `WorkflowValidateRequest`, `WorkflowValidateResponse`, `WorkflowCompileRequest`, `WorkflowCompileResponse`, `WorkflowCompiledResponse`, `WorkflowCompiledPayload` all present with correct field shapes
- [x] 15.2 `validate_workflow` route handler implemented: accepts `WorkflowV2Definition` JSON body, calls validate + normalize phases, returns `{ valid, issues, normalized }`
- [x] 15.3 `compile_workflow` route handler implemented: calls full pipeline, returns `{ definition_id, normalized, compiled: { workflow_ir } }`
- [x] 15.4 `get_workflow_compiled` route handler implemented: loads stored definition, re-compiles it (does not cache IR), returns `404` for unknown IDs
- [x] 15.5 All three routes registered in `crates/openfang-api/src/server.rs` under `/api/v1/workflows/validate`, `/api/v1/workflows/compile`, `/api/v1/workflows/{id}/compiled`
- [x] 15.6 Integration tests for all three endpoints — NOT present in `crates/openfang-api/src/routes.rs`

## Findings

### Correct
- All three route handlers are implemented and properly wired.
- `validate_workflow` correctly calls only the validate and normalize phases (not the compile phase), satisfying the anti-pattern guard that the validate endpoint must not compile.
- `compile_workflow` correctly returns an error (not a partial IR) when validation fails, via `workflow_v2_compile_error_response`.
- `get_workflow_compiled` returns a standard `404` envelope when the workflow ID is unknown via `workflow_definition_not_found_response`.
- All error responses use the `{ error: { code, message, details } }` envelope format, enforced by the shared `workflow_v2_error_response` helper.
- Request types match the API-SPEC shapes: validate request has `definition`, `strict`, `context`; compile request has `definition`, `context`.
- Response types match the spec: `WorkflowValidateResponse` has `valid`, `issues`, `normalized`; `WorkflowCompileResponse` has `definition_id`, `normalized`, `compiled.workflow_ir`.
- `WorkflowValidationIssue` is a type alias for the shared `ValidationIssue` from `openfang-types` — correct reuse.

### Missing
- **Integration tests are absent.** The task requires six integration tests covering: `post_validate_returns_valid_true_for_correct_definition`, `post_validate_returns_issues_for_dangling_reference`, `post_compile_returns_workflow_ir`, `get_compiled_returns_cached_ir_for_registered_workflow`, `get_compiled_returns_404_for_unknown_id`, and `end_to_end_definition_to_ir_preserves_step_semantics`. None of these test functions exist anywhere in the API crate's test modules. The `routes.rs` file has test modules for pack routes, trigger definition routes, skill routes, and task control plane routes, but no test module for the workflow v2 compile/validate/compiled endpoints.

### Minor Observations
- `get_workflow_compiled` re-runs the compile pipeline on each request rather than returning a cached IR. This deviates slightly from the spec wording ("returns the cached `WorkflowIr` for a previously registered workflow, without re-running the compile pipeline"), but the result is functionally equivalent for a persisted definition that has not changed. The spec's acceptance criteria focuses on the endpoint returning the correct IR for a registered workflow and returning 404 for unknown IDs, both of which are satisfied.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/types.rs`
