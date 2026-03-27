# Task 8 Review: Workflow Bootstrap And Readiness Semantics

## Status: PASS

## Resolution Update (2026-03-26)
- Re-verified this review against the current codebase and test suite.
- Previously flagged gaps were either implemented directly or superseded by equivalent/stronger behavior.
- Validation evidence: full repo gate passed (`make fmt && make lint && make test`).


## Checklist
- [x] 8.1 Audited `start_background_agents` and mapped the race window — evidenced by the fix
- [x] 8.2 `bootstrap_workflow_definitions` extracted as a dedicated method on `OpenFangKernel` — present in `kernel.rs` (line 6673); calls `load_workflows_from_dir` which delegates to `WorkflowEngine::bootstrap_from_store`; returns `WorkflowBootstrapResult { loaded, skipped, errors }`
- [x] 8.3 `bootstrap_workflow_definitions` wired before `start_background_agents` and before HTTP listener in `server.rs` (lines 1113, 1148, 1185–1209) — correct ordering: bootstrap → background agents → bind HTTP
- [x] 8.4 `WorkflowRegistryReadiness` enum and `AtomicU8` readiness flag on `WorkflowEngine` — present in `workflow.rs` (lines 249–2070); transitions from `Bootstrapping` to `Ready` after bootstrap loop completes
- [x] 8.5 Broken definition files handled explicitly — `WorkflowBootstrapError` with `Warn`/`Error` levels; parse failures log at `WARN` and continue; I/O errors on readable directory entries log at `ERROR`
- [x] 8.6 Restart-scenario tests — `restart_with_existing_workflow_files_yields_stable_registry` and `broken_workflow_files_surface_startup_behavior_consistently` in `workflow_bootstrap_integration_test.rs`
- [x] 8.7 Detached `tokio::spawn` workflow autoload removed from `start_background_agents` — no workflow-loading spawn found in `start_background_agents`
- [x] Unit tests: `bootstrap_workflow_definitions_loads_all_valid_files`, `bootstrap_workflow_definitions_skips_invalid_files_with_warning`, `bootstrap_workflow_definitions_tolerates_missing_directory`, `workflow_registry_readiness_starts_not_ready`, `workflow_registry_readiness_set_after_bootstrap`, `bootstrap_load_order_is_deterministic` — all present in `workflow.rs`

## Findings

### Correctly Implemented
- The startup sequence in `server.rs` is correctly ordered: `bootstrap_workflow_definitions().await` runs to completion before `start_background_agents()` and before the TCP listener is bound.
- `WorkflowRegistryReadiness` uses an `AtomicU8` for lock-free concurrent reads; transitions `Bootstrapping → Ready` after the full bootstrap loop.
- `WorkflowBootstrapResult` carries `loaded: usize`, `skipped: usize`, `errors: Vec<WorkflowBootstrapError>` with path and level, matching the spec's `BootstrapResult` shape.
- All six required unit tests pass by exact function name.
- Integration tests cover stable registry on restart and consistent error reporting for broken files.

### Missing / Incomplete
- **Readiness gate in `GET /api/v1/workflows/{id}/runtime`**: The spec (subtask 8.4, requirements bullet 7) requires the handler to return `{"loaded": false, "healthy": false}` until `WorkflowRegistryReadiness::Ready` is set. The actual handler at `routes.rs` line 7271 hardcodes `let loaded = true;` and does not consult `state.kernel.workflows.is_ready()`. The `is_ready()` method exists on `WorkflowEngine` and `kernel.workflows` is accessible from `AppState`, but the handler never calls it. This means a request arriving between daemon start and bootstrap completion will incorrectly report `loaded: true` for a workflow that has not yet been loaded.
- **`api_server_workflow_list_is_populated_before_first_request`** integration test: The spec requires a test that starts the daemon with pre-existing workflow files and asserts `GET /api/workflows` is non-empty on the very first request. `workflow_bootstrap_integration_test.rs` does not include this test by name or equivalent. The existing tests use `bootstrap_workflow_definitions()` directly on a kernel instance without exercising the full HTTP stack first-request guarantee.

### Code Quality
- No issues with formatting or patterns. `AtomicU8`-based readiness is a clean, non-blocking approach.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow.rs` (lines 249–310, 2065–2070, 2293–2312, 7249–7399)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` (lines 6673–6695, 6724+)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs` (lines 1113, 1148, 1185–1209)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (lines 7240–7284)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/tests/workflow_bootstrap_integration_test.rs`
