# Task 19 Review: Restart Recovery And Durable Run Control Surfaces

## Status: PASS

## Checklist
- [x] Startup recovery scan implemented — `recover_running_runs()` on `WorkflowRunRepository`, called during `boot_with_config_and_migrations` in `kernel.rs`
- [x] `WorkflowRunStatus::Paused` and `WorkflowRunStatus::Cancelled` variants — present in `workflow_store.rs`
- [x] `WorkflowRunRepository` backed by `compozy.db` with `find_by_id`, `list`, `update_status`, `insert_checkpoint`, `find_checkpoints_for_run` — present in `workflow_store.rs`
- [x] Recovery scan uses SQLite transaction (atomic) — `recover_running_runs_with` uses an immediate transaction
- [x] Recovery only downgrades `running` → `paused`; `waiting_signal` and `waiting_hitl` rows left untouched
- [x] Terminal runs (`completed`, `failed`, `cancelled`) not touched
- [x] `run_recovered_needs_resume` checkpoint kind added with `data.previous_status = "running"`
- [x] `RunPaused`, `RunResumed` checkpoint kinds added
- [x] `POST /api/v1/runs/{id}/pause`, `/resume`, `/cancel` route handlers implemented and registered in `server.rs`
- [x] Pause only valid from `running` or `waiting_signal`; resume only from `paused`; cancel from any non-terminal
- [x] All run read surfaces (`GET /api/v1/runs`, `/runs/{id}`, `/checkpoints`, `/dispatches`, `/signals`) registered in `server.rs` and backed by `compozy.db`
- [x] `?status` and `?waiting_kind` filters on list endpoint
- [x] Recovery logged at `info` (count) and `debug` (per run_id) levels
- [x] Recovery scan runs before HTTP server accepts requests — called inside `boot_with_config_and_migrations`, before `run_daemon` calls `build_router`
- [x] Unit tests: `running_runs_downgrade_to_paused_on_recovery_scan`, `waiting_signal_runs_survive_recovery_scan_unchanged`, `waiting_hitl_runs_survive_recovery_scan_unchanged`, `terminal_runs_are_not_touched_by_recovery_scan`, `recovery_scan_is_atomic`, `recovery_checkpoints_record_previous_status`, `pause_action_rejects_invalid_source_status`, `resume_action_only_valid_from_paused` — all present in `workflow_store.rs`
- [x] Recovery idempotency test `recovery_scan_is_idempotent` — present
- [x] Integration tests: `test_v1_recovered_run_is_paused_after_restart`, `get_run_list_reflects_recovered_state`, `restart_preserves_waiting_state_and_outstanding_signals`, `pause_resume_cancel_round_trip_through_db`, `waiting_signal_run_still_accepts_signal_after_restart` — all present in `api_integration_test.rs`
- [x] `signal_and_checkpoint_history_intact_after_restart` — covered by `restart_preserves_waiting_state_and_outstanding_signals` (asserts both signals and checkpoints survive restart)

## Findings
- All deliverables fully implemented. Recovery scan is correctly placed at kernel boot time, before the HTTP server starts accepting requests.
- The `WorkflowRunStatus` enum includes all required variants including `Pending`, which was not explicitly mentioned in the spec but is a correct addition.
- The backward-compatibility shim in `parse_db_text` (`"waiting" | "waiting_signal"` and `"paused" | "interrupted"`) cleanly handles legacy rows.
- The `CheckpointKind::RunCancelled` checkpoint is present (for cancel action), satisfying the 19.6 requirement even though the spec names it as part of 19.6.
- The named integration test `signal_and_checkpoint_history_intact_after_restart` is implemented as `restart_preserves_waiting_state_and_outstanding_signals` — same semantics, different name.
- No unresolved issues found.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/workflow_store.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/api_integration_test.rs`
