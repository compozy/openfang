# Task 29 Review: Dispatch Runtime Integration With Provider-Native Sessions

## Status: PASS

## Checklist
- [x] `DispatchRecord` created with `status = pending` before any provider interaction in step executor
- [x] `dispatch_id` stored in step execution context, survives across `.await` boundary
- [x] Dispatch kind (`call`, `send`, `spawn`) resolved from workflow step definition via `durable_dispatch_kind()`
- [x] `DispatchKind::Call`, `DispatchKind::Send`, `DispatchKind::Spawn` mapped from `WorkflowDispatchMode`
- [x] Dispatch record transitioned to `status = running` after provider session established
- [x] `provider_driver`, `session_id` written to dispatch record after session setup
- [x] `provider_resume_token` column populated (nullable, provider-dependent)
- [x] `call` mode: awaits result, writes `result_json`, transitions to `completed`; on error writes `error_json`, transitions to `failed`
- [x] `send` mode: spawns background tokio task, returns `dispatch_id` immediately while record stays `running`
- [x] `spawn` mode: writes `spawned_agent_id`, transitions to `completed`
- [x] Result/error writes are transactional (JSON + status change atomic)
- [x] Invalid/missing provider binding produces `status = failed` with descriptive `error_json`
- [x] No `Mutex`/`RwLock` held across `.await` points in step executor
- [x] `send` mode background tasks tracked in `JoinSet` (cancellation via `CancellationToken`)
- [x] Dispatch integration goes through `ProviderBinding` layer, not direct manifest field reads
- [x] Unit test: `dispatch_record_should_have_session_id_after_provider_setup`
- [x] Unit test: `dispatch_call_should_complete_with_result_json`
- [x] Unit test: `dispatch_call_should_fail_with_error_json_on_provider_error`
- [x] Unit test: `dispatch_send_should_return_immediately_with_running_status`
- [x] Unit test: `dispatch_spawn_should_write_spawned_agent_id`
- [x] Unit test: `dispatch_invalid_provider_binding_should_fail_cleanly`
- [x] Integration test: `dispatch_call_end_to_end_with_openfang_llm_driver` (in `kernel.rs`)
- [x] Integration test: `dispatch_call_end_to_end_with_arky_provider` (in `kernel.rs`)
- [x] Integration test: `dispatch_session_identity_survives_reconnect` (in `kernel.rs`)
- [x] Integration test: `dispatch_send_background_task_should_complete_and_update_record`
- [x] Integration test: `dispatch_concurrent_call_dispatches_should_not_interfere`
- [x] `spawned_agent_id` column present and written on `spawn` mode
- [x] `DispatchRecord` fields `provider_driver`, `session_id`, `provider_resume_token`, `spawned_agent_id` confirmed in `dispatch.rs`

## Findings

**Implemented correctly:**
- The step executor in `workflow.rs` creates a `DispatchRecord` with `status = pending` before any provider call, storing the `dispatch_id` in the step context.
- All three dispatch modes (`Call`, `Send`, `Spawn`) are implemented with correct runtime semantics. `durable_dispatch_kind()` maps from the workflow definition's `WorkflowDispatchMode` to the storage-level `DurableDispatchKind`.
- Provider identity columns (`provider_driver`, `session_id`, `provider_resume_token`) are populated via `mark_dispatch_running_for_test` / `mark_dispatch_running` after session establishment, and the unit test `dispatch_record_should_have_session_id_after_provider_setup` verifies non-null values.
- For the `send` mode, the background task is tracked and the test `dispatch_send_background_task_should_complete_and_update_record` verifies the record transitions to `completed` when the task finishes.
- For the `spawn` mode, `spawned_agent_id` is written and verified by `dispatch_spawn_should_write_spawned_agent_id`.
- The two end-to-end integration tests live in `kernel.rs` (not `workflow.rs`) and exercise the full path against test double providers, verifying `provider_driver` and session identity are populated durably.
- The `dispatch_session_identity_survives_reconnect` test in `kernel.rs` confirms the session identity is durable across connection drops.
- Concurrent dispatch test `dispatch_concurrent_call_dispatches_should_not_interfere` ensures two simultaneous dispatches produce independent records.

**Minor observations:**
- The end-to-end tests use test double providers (not actual Arky Claude Code or Codex). The Arky integration test (`dispatch_call_end_to_end_with_arky_provider`) uses a simulated arky provider path rather than the real `arky-claude-code` crate. This is acceptable for automated testing without API credentials, but means actual Claude Code / Codex session resumption behavior is not exercised in CI.
- `provider_resume_token` is populated when available but the specific column-write path for providers that return a resume token is covered by the nullable field mechanics rather than a dedicated test case. This is acceptable per the spec (it describes the column as a "nullable escape hatch").

**Code quality:**
- No `Mutex` or `RwLock` held across `.await` points — verified by code structure.
- `thiserror`-based error types at every layer.
- Clean separation between the step executor (workflow engine) and the dispatch repository.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/dispatch.rs`
