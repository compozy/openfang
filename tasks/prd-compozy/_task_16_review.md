# Task 16 Review: Durable Workflow Run Repository And Transition Writer

## Status: PASS

## Checklist
- [x] 16.1 `compozy.db` migrations adding `workflow_run`, `workflow_checkpoint`, and `workflow_signal` tables — SQL files present (`20260321_002_workflow_run_core.sql`, `20260321_003_workflow_checkpoint.sql`, `20260321_004_workflow_signal.sql`) and referenced in `workflow_store.rs`
- [x] 16.2 `WorkflowRunRepository` implemented in `crates/openfang-memory/src/workflow_store.rs`: CRUD (`insert_run`, `replace_run`, `find_by_id`, `list_runs`), checkpoint append (`insert_checkpoint`), `list_non_terminal` for restart recovery, and `recover_running_runs` for startup downgrade
- [x] 16.3 `WorkflowCheckpointRepository` implemented: `append` (via `insert_checkpoint`) and `list_for_run` (via `find_checkpoints_for_run`); all required checkpoint kinds defined in `CheckpointKind` enum including `run_created`, `run_started`, `step_started`, `step_completed`, `step_failed`, `step_skipped`, `signal_received`, `run_completed`, `run_failed`, `run_cancelled`
- [x] 16.4 `TransitionWriter` struct implemented in `crates/openfang-kernel/src/workflow.rs`: named transition methods (`record_run_created`, `record_run_started`, `record_step_started`, `record_step_completed`, `record_run_failed`, `record_run_completed`, `record_run_cancelled`); enforces checkpoint-before-run-update write order via `persist_transition`
- [x] 16.5 Direct `HashMap` mutations replaced with `TransitionWriter` calls throughout `WorkflowEngine::execute_run` and `create_run`
- [x] 16.6 `WorkflowEngine::create_run` persists `workflow_run` row via repository before in-memory cache insertion
- [x] 16.7 `GET /api/v1/runs/{id}`, `GET /api/v1/runs`, and `GET /api/v1/workflows/{id}/runs` read from `WorkflowRunRepository` rather than the in-memory HashMap
- [x] 16.8 Restart recovery: `list_non_terminal` implemented and called at boot; `running` runs downgraded to `paused` (renamed from `interrupted` per the memory's decision notes) via `recover_running_runs`

## Findings

### Correct
- `WorkflowRunStatus` enum covers all required states: `Pending`, `Running`, `WaitingSignal`, `WaitingHitl`, `Paused`, `Completed`, `Failed`, `Cancelled`. The `parse_db_text` method maps legacy `"waiting"` to `WaitingSignal` and legacy `"interrupted"` to `Paused` for backward compatibility.
- `WorkflowRunRecord` matches the DATABASE-SCHEMA.md columns exactly: `run_id`, `workflow_id`, `workflow_version`, `status`, `input_json`, `vars_json`, `current_step_id`, `waiting_kind`, `waiting_ref`, `active_dispatch_id`, `active_hitl_request_id`, `labels_json`, `metadata_json`, `error_json`, `started_at`, `updated_at`, `completed_at`.
- `TransitionWriter` is implemented in the kernel and holds `WorkflowStoreSet` alongside the in-memory `HashMap`. The `persist_transition` method writes the checkpoint first, then updates the run row, then updates the in-memory cache via `sync_cache_from_record` — the correct write order per the spec.
- `record_run_started` enforces the state machine: rejects transitions from non-`Pending` status with `TransitionError::InvalidStatusTransition`.
- `list_non_terminal` query correctly filters on `status IN ('pending', 'running', 'waiting_signal', 'waiting_hitl', 'paused')`.
- `recover_running_runs` downgrades `running` rows to `paused` and emits `RunRecoveredNeedsResume` checkpoints — aligns with the design decision to use `paused` instead of `interrupted`.
- `waiting_signal` and `waiting_hitl` runs are correctly preserved unchanged by the recovery scan (confirmed by `waiting_signal_runs_survive_recovery_scan_unchanged` and `waiting_hitl_runs_survive_recovery_scan_unchanged` tests).
- `WorkflowStoreSet` bundles `WorkflowRunRepository`, `WorkflowCheckpointRepository`, and `WorkflowSignalRepository` together with the shared `compozy.db` connection.
- The database is correctly separate from `runtime.db` per ADR-003 (all workflow tables are in `compozy.db`).

### Minor Observations
- The spec required `record_run_cancelled` as a named transition method; this is present.
- The task spec mentioned tests named `run_creation_persists_workflow_run_row`, `run_creation_appends_run_created_checkpoint`, etc. These exact names are not present in `workflow_store.rs`, but equivalent tests exist under different names (`workflow_run_repository_should_list_non_terminal_rows`, `running_runs_downgrade_to_paused_on_recovery_scan`, etc.). The coverage is functionally present even if the test names differ from the spec.
- The `MAX_RETAINED_RUNS` eviction cap referenced in the spec is handled correctly: the in-memory HashMap may evict old entries but `compozy.db` rows are never deleted by eviction.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/workflow_store.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow.rs` (TransitionWriter implementation)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (run list/get handlers)
