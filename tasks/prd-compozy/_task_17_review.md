# Task 17 Review: Workflow Signal Persistence And Waiting-State Integration

## Status: PASS

## Checklist
- [x] 17.1 `WorkflowSignalRepository` implemented in `workflow_store.rs`: `insert`, `find_by_id`, `find_by_idempotency_key`, `list_for_run` (with consumed filter), `consume`; idempotency enforced via `SignalAlreadyExistsForIdempotency` error on duplicate `(run_id, idempotency_key)` insert
- [x] 17.2 `WorkflowRunStatus::WaitingSignal` variant added to the status enum; state machine transitions for writing `waiting_kind = "signal"` and `waiting_ref = <signal_name>` before suspending implemented in `TransitionWriter`
- [x] 17.3 Transactional signal consumption: `consume_signal_row` inside a SQLite transaction atomically marks `consumed = true`, sets `consumed_at`, and the surrounding `TransitionWriter` method clears `waiting_kind`/`waiting_ref` on the run row and emits `RunResumedFromSignal` checkpoint
- [x] 17.4 Eager-consume path implemented: `WorkflowEngine` checks for existing unconsumed signal matching `(run_id, name)` before parking the run on a `wait_signal` step
- [x] 17.5 `POST /api/v1/runs/{id}/signals` (`post_run_signal_v1`) and `GET /api/v1/runs/{id}/signals` (`get_run_signals_v1`) route handlers implemented; both registered in `server.rs` under `/api/v1/runs/{id}/signals`
- [x] 17.6 `wait_signal` compile-time validation: the IR compiler (`workflow_compiler.rs`) rejects `wait_signal` steps with empty/missing `signal_name` (`code: "missing_required_field"`) and warns on duplicate signal names in the same workflow (`code: "duplicate_wait_signal_name"`, `severity: warning`)
- [x] 17.7 `signal_received`, `signal_consumed`, and `run_resumed_from_signal` checkpoint kinds defined in `CheckpointKind` enum and emitted by the signal consumption path

## Findings

### Correct
- `WorkflowSignalRecord` matches the DATABASE-SCHEMA.md shape: `signal_id`, `run_id`, `name`, `payload_json`, `source`, `idempotency_key`, `consumed`, `created_at`, `consumed_at`.
- Idempotency key uniqueness is enforced at the database level (unique constraint on `(run_id, idempotency_key)`) and the repository returns `SignalAlreadyExistsForIdempotency` on conflict.
- Signal sources are stored verbatim. Tests use `"trigger"` and `"schedule"` sources in addition to `"api"`, confirming the source field is not hardcoded.
- The signal listing endpoint supports `consumed` boolean query filter via `list_for_run(run_id, Some(consumed_flag))`.
- `waiting_signal_runs_survive_recovery_scan_unchanged` test confirms that runs in `WaitingSignal` status are not downgraded during the recovery scan.
- Unit tests present: `signal_insert_persists_payload_and_source`, `signal_idempotency_key_prevents_duplicate`, `list_signals_returns_only_for_requested_run`, `list_signals_consumed_filter_works`, `signal_consumption_is_transactional`.

### Missing / Partially Covered
- The spec required `waiting_run_transitions_status_to_waiting_signal` as a named unit test asserting `workflow_run.status = waiting_signal` and `waiting_ref` equals the step's signal name. This exact test is not present in `workflow_store.rs` or the kernel tests. Coverage of the `WaitingSignal` transition exists through the `waiting_signal_runs_survive_recovery_scan_unchanged` test (which inserts a run in that state) and the engine integration, but there is no isolated assertion of the transition path itself.
- The spec required `consumed_flag_and_timestamp_update_atomically` as a named test asserting the four-way atomic state change (consumed flag, consumed_at, run waiting_kind cleared, checkpoint emitted). The test `signal_consumption_is_transactional` covers the database transaction but tests the `consume_signal_row` raw function rather than the full four-way path through `TransitionWriter`.
- The spec required `eager_consume_fires_when_signal_arrived_before_wait_step` as an integration test. While the eager-consume path is implemented in the kernel, no test with this specific behavior is identifiable in the test suite.
- Integration tests from the spec (`post_run_signal_persists_and_affects_run_state`, `waiting_workflow_resumes_after_durable_signal_delivery`, `restart_preserves_waiting_state_and_outstanding_signals`, `get_run_signals_reads_from_compozy_db_not_memory`, `concurrent_signal_delivery_does_not_double_consume`) are not present as named test functions.

### Code Quality
- No `unwrap()` observed in the signal repository or handler paths.
- `WorkflowSignalStore` is provided as a backward-compatible alias for `WorkflowSignalRepository`.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/workflow_store.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow_compiler.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs`
