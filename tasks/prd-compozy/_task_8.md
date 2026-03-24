## markdown

## status: completed

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
bootstrap and readiness semantics appropriate for a durable product runtime.

The current implementation in `crates/openfang-kernel/src/kernel.rs` loads
workflow definitions inside `start_background_agents` by spawning a detached
`tokio::spawn` task that calls `load_workflows_from_dir`. This means:

1. Workflow definitions are not guaranteed to be loaded when the API server
   becomes available. A caller hitting `GET /api/workflows` immediately after
   daemon start may receive an empty list even if definitions exist on disk.
2. The spawn is fire-and-forget — if `load_workflows_from_dir` fails silently,
   the daemon considers itself healthy with an empty workflow registry.
3. There is no readiness gate on the workflow registry. Nothing prevents a
   run-creation request from arriving before definitions are loaded.
4. The `start_background_agents` method already does many things (hand
   restoration, MCP connection, extension health monitor, cron scheduler) and
   workflow loading is buried inside it as a parallel concern.

Per ADR-021 (runtime-first hardening), the startup sequence must be explicit
and deterministic before Phase 1 run-durability work begins. Per ADR-005
(durable workflow runtime), the runtime must be in a known state before
accepting workflow run requests. This task enforces both by making workflow
bootstrap a synchronous, ordered, error-surfacing step in the daemon startup
sequence.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Workflow definition loading must complete synchronously before the API server begins accepting requests. The current detached `tokio::spawn` inside `start_background_agents` is not acceptable for a durable runtime.
- The daemon startup sequence must have an explicit ordered phase for workflow bootstrap: parse config, load definitions, validate definitions, register definitions, then start the HTTP server.
- Readiness must reflect actual workflow availability. A readiness check that returns healthy while definitions are still loading is not acceptable per ADR-021.
- Bootstrap errors must be classified: a malformed definition file must surface as a warning with the specific file and error, not silently skip to empty. A missing workflows directory must not be an error (it may not exist yet). A filesystem read failure on an existing directory must surface as an error that degrades startup in a documented, observable way.
- The workflow loading order must be deterministic. Definitions loaded from disk must be registered in a stable order (e.g., lexicographic by filename) so that any logging, metrics, or test assertions about load order are reproducible.
- No sleep-based readiness workaround is acceptable. Readiness must be a state the daemon transitions into explicitly, not something the caller polls until it stabilizes.
- The public `GET /api/v1/workflows/{id}/runtime` endpoint (per API-SPEC.md section 4) must reflect `loaded: true` only after the bootstrap phase completes for that definition.
</requirements>

## Subtasks

- [x] 8.1 Audit the current `start_background_agents` method in `crates/openfang-kernel/src/kernel.rs` and map the exact position of the workflow autoload spawn relative to the API server start in `crates/openfang-api/src/server.rs`. Document the race window.
- [x] 8.2 Extract workflow definition loading from `start_background_agents` into a dedicated synchronous `bootstrap_workflow_definitions` method on `OpenFangKernel` that: (a) reads and validates all `.json` or `.toml` files from the configured workflows directory, (b) registers valid definitions, (c) logs each load result with path and outcome, and (d) returns a `BootstrapResult` summary (count loaded, count skipped, any errors).
- [x] 8.3 Wire `bootstrap_workflow_definitions` into the server startup sequence in `crates/openfang-api/src/server.rs` so it runs to completion before the Axum router begins serving requests. Ensure the startup sequence follows the pattern: `bootstrap_workflow_definitions` → `start_background_agents` (for non-workflow background tasks) → bind HTTP listener.
- [x] 8.4 Introduce a `WorkflowRegistryReadiness` state on `WorkflowEngine` (or an equivalent lightweight flag on `OpenFangKernel`) that transitions from `Bootstrapping` to `Ready` after `bootstrap_workflow_definitions` completes. The `GET /api/v1/workflows/{id}/runtime` handler must return `loaded: false` until this transition occurs.
- [x] 8.5 Handle broken definition files explicitly: a TOML/JSON parse failure must log the file path and error at `WARN` level and continue loading remaining files. A file I/O error on an existing readable directory entry must log at `ERROR` level. Neither case should silently produce an empty registry without surfacing the cause.
- [x] 8.6 Add restart-scenario tests that verify the workflow registry is fully populated before any API handler can serve a run-creation request.
- [x] 8.7 Remove the detached `tokio::spawn` workflow autoload block from `start_background_agents` once the synchronous bootstrap path is in place.

## Implementation Details

The current problematic pattern in `crates/openfang-kernel/src/kernel.rs` (around line 4026):

```
// Auto-load workflow definitions from configured directory
{
    let wf_dir = ...;
    if wf_dir.exists() {
        let kernel = Arc::clone(self);
        tokio::spawn(async move {
            let count = kernel.load_workflows_from_dir(&wf_dir).await;
            ...
        });
    }
}
```

This runs concurrently with everything else in `start_background_agents` and
concurrently with the API server binding that happens immediately after
`start_background_agents` returns in `crates/openfang-api/src/server.rs` (line 736).

The fix should restructure startup in `server.rs` as a sequential pipeline:

1. Build `OpenFangKernel` (existing).
2. Call `kernel.bootstrap_workflow_definitions().await` — synchronous, returns `BootstrapResult`.
3. Call `kernel.start_background_agents()` — which must no longer contain workflow loading.
4. Bind the Axum router and start the HTTP listener.

`bootstrap_workflow_definitions` should:

- Use the same `workflows_dir` config path that `load_workflows_from_dir` currently uses.
- Iterate files in deterministic lexicographic order.
- For each file: attempt to deserialize, validate (schema-level only at this stage), and register.
- Return a `BootstrapResult { loaded: usize, skipped: usize, errors: Vec<BootstrapError> }` where `BootstrapError` carries the file path and error message.
- Set `WorkflowRegistryReadiness::Ready` on the `WorkflowEngine` after the loop completes, even if some files were skipped.

The readiness gate in `WorkflowEngine` should be a simple `AtomicBool` or
`OnceLock<()>` initialized to not-ready, set to ready by `bootstrap_workflow_definitions`.
It does not need to be a complex state machine at this stage.

The `GET /api/v1/workflows/{id}/runtime` handler behavior:

- Before readiness: return `{"workflow_id": "...", "loaded": false, "healthy": false}` with HTTP 200.
- After readiness: return the normal runtime projection per API-SPEC.md section 4.

This matches the API-SPEC.md `runtime` resource shape:

```json
{
  "workflow_id": "sdlc",
  "loaded": true,
  "healthy": true,
  "active_runs": 0,
  "waiting_runs": 0,
  "last_run_at": null
}
```

### Relevant Files

- `crates/openfang-kernel/src/kernel.rs` — `start_background_agents` (line 3801), `load_workflows_from_dir` (line 3757), `start_background_agents` workflow spawn block (line 4026)
- `crates/openfang-api/src/server.rs` — `kernel.start_background_agents()` call (line 736) and the HTTP listener bind that follows it
- `crates/openfang-kernel/src/workflow.rs` — `WorkflowEngine` struct, to receive the readiness flag
- `tasks/prd-compozy/docs/DESIGN.md` — section 27 (Safe Delivery Order)
- `tasks/prd-compozy/docs/adrs/021-runtime-first-workflow-hardening.md`
- `tasks/prd-compozy/docs/adrs/005-durable-workflow-runtime.md`
- `tasks/prd-compozy/docs/adrs/037-file-backed-definitions-and-db-ownership.md`

### Dependent Files

- `crates/openfang-api/src/routes.rs` — workflow runtime handler that must check readiness
- Any existing workflow bootstrap or daemon startup tests

## Deliverables

- `bootstrap_workflow_definitions` as a dedicated, synchronous, error-surfacing startup method on `OpenFangKernel`.
- Startup sequence in `server.rs` guarantees workflow bootstrap completes before the HTTP listener binds.
- `WorkflowRegistryReadiness` flag on `WorkflowEngine` that API handlers can inspect.
- Detached workflow autoload spawn removed from `start_background_agents`.
- Tests for boot ordering and startup guarantees.

## Tests

### Unit Tests (Required)

- [x] `bootstrap_workflow_definitions_loads_all_valid_files`: populate a temp directory with N valid workflow `.json` files, call `bootstrap_workflow_definitions`, assert `BootstrapResult.loaded == N` and all workflow IDs are present in the registry.
- [x] `bootstrap_workflow_definitions_skips_invalid_files_with_warning`: populate a temp directory with one valid and one malformed file, call `bootstrap_workflow_definitions`, assert `loaded == 1`, `skipped == 1`, and `errors` contains the malformed file's path.
- [x] `bootstrap_workflow_definitions_tolerates_missing_directory`: call `bootstrap_workflow_definitions` with a non-existent directory path, assert it returns `loaded == 0` with no hard error.
- [x] `workflow_registry_readiness_starts_not_ready`: construct a fresh `WorkflowEngine`, assert `is_ready()` returns `false` before bootstrap runs.
- [x] `workflow_registry_readiness_set_after_bootstrap`: call `bootstrap_workflow_definitions`, assert `is_ready()` returns `true` immediately after.
- [x] `bootstrap_load_order_is_deterministic`: call `bootstrap_workflow_definitions` on a directory with multiple files, assert the load log entries appear in lexicographic filename order.

### Integration Tests (Required)

- [x] `api_server_workflow_list_is_populated_before_first_request`: start the daemon with pre-existing workflow files, make the first `GET /api/workflows` request immediately after the server binds, assert the response contains the expected workflows (not an empty list).
- [x] `restart_with_existing_workflow_files_yields_stable_registry`: load definitions, simulate a restart via `bootstrap_workflow_definitions` on the same directory, assert the registry after restart matches the registry before restart.
- [x] `broken_workflow_files_surface_startup_behavior_consistently`: two successive restarts with the same broken file must produce the same `BootstrapResult` error set — no non-deterministic partial-load behavior.

### Regression and Anti-Pattern Guards

- [x] No detached `tokio::spawn` workflow loading remains in `start_background_agents` after this task.
- [x] No sleep-based readiness polling exists anywhere in the startup path or in tests.
- [x] The API server must not bind the HTTP listener before `bootstrap_workflow_definitions` returns — verify by checking server.rs startup ordering.
- [x] `WorkflowRegistryReadiness::Ready` must not be set before `bootstrap_workflow_definitions` completes its full loop, even in error paths.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- Workflow bootstrap timing is explicit: `bootstrap_workflow_definitions` is a named, synchronous, observable step in the daemon startup sequence.
- Readiness reflects real availability: the `WorkflowEngine` readiness flag is `false` until bootstrap completes, and the `GET /api/v1/workflows/{id}/runtime` handler uses it.
- The startup sequence in `server.rs` has a documented, tested order: bootstrap → background tasks → HTTP bind.
- `BootstrapResult` provides observable load counts and error paths for every startup — no silent empty-registry boots.
- All broken-file scenarios produce consistent, repeatable error logs across restarts.
- Phase 1 durable-run work (task 16) can document this task as a prerequisite and rely on the registry being stable from the first API request onward.
- `cargo test --workspace` passes with zero failures and `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.

---

## Notes

- This task depends on task 7 (definition source-of-truth consistency). Bootstrap correctness requires that the files being loaded are themselves canonical. Do not start this task until task 7 is complete.
- ADR-021 explicitly states: "Early implementation work should prioritize state models, transitions, recovery, and dispatch/HITL handling." Startup-sequence correctness is part of state model hardening.
- The `start_background_agents` method is large and does many things. This task only extracts the workflow loading concern. Do not refactor other parts of `start_background_agents` as part of this task unless they directly interfere with the bootstrap ordering fix.
- Do not introduce a health check endpoint as the readiness mechanism. Readiness is an internal state transition, not a polling surface.
