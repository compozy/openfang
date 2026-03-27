# Task 9 Review: Initial compozy.db Workflow Core Schema

## Status: PASS

## Resolution Update (2026-03-26)
- Re-verified this review against the current codebase and test suite.
- Previously flagged gaps were either implemented directly or superseded by equivalent/stronger behavior.
- Validation evidence: full repo gate passed (`make fmt && make lint && make test`).


## Checklist
- [x] 9.1 Three `compozy.db` migration SQL files written — `20260321_002_workflow_run_core.sql`, `20260321_003_workflow_checkpoint.sql`, `20260321_004_workflow_signal.sql` present in `openfang-memory/migrations/compozy/`; registered as `MigrationStep` entries
- [x] 9.2 `WorkflowRunStore` implemented — exposed as a type alias for `WorkflowRunRepository` in `openfang-memory/src/workflow_store.rs` (line 499); provides `insert_run`, `find_by_id`, `list_runs`, `update_status`, `persist_transition` (transition writer)
- [x] 9.3 `WorkflowCheckpointStore` implemented — `WorkflowCheckpointRepository` with `append`, `list_for_run`; `CheckpointKind` enum covers all 10 required kinds plus `run_recovered_needs_resume`
- [x] 9.4 `WorkflowSignalStore` implemented — `WorkflowSignalRepository` with `insert`, `list_for_run`, `consume`; consumed flag semantics are correct
- [x] 9.5 Raw `compozy_db` handle replaced by typed `WorkflowStoreSet` — `OpenFangKernel.workflow_stores: WorkflowStoreSet` present (kernel.rs line 172); `WorkflowEngine` receives typed stores via `workflow_stores` field
- [x] 9.6 Startup recovery scan implemented — `recover_workflow_runs_on_startup` called in `boot_with_config_and_migrations` (kernel.rs line 779) before workflow engine starts; downgrades `running` → `paused`, appends `run_recovered_needs_resume` checkpoint
- [x] 9.7 Store adapter tests written — `workflow_run_repository_should_insert_and_load_rows`, `workflow_run_repository_should_persist_checkpoint_before_row_update_atomically`, `running_runs_downgrade_to_paused_on_recovery_scan`, `waiting_signal_runs_survive_recovery_scan_unchanged`, and others present in `workflow_store.rs`

## Findings

### Correctly Implemented
- All three migration files exist with the required tables and indexes (verified SQL).
- `workflow_checkpoint` and `workflow_signal` tables use `REFERENCES workflow_run(run_id) ON DELETE CASCADE` — no cross-database FK violations since both reference rows in the same `compozy.db`.
- All required indexes are present in the migration SQL: `idx_workflow_run_workflow_id`, `idx_workflow_run_status`, `idx_workflow_run_updated_at`, `idx_workflow_checkpoint_run`, `idx_workflow_signal_run`, `idx_workflow_signal_run_consumed`, `idx_workflow_signal_run_name`.
- `persist_transition` implements the combined run-status + checkpoint atomic transaction write specified by the Transition Writer Contract.
- Recovery scan tests cover: running → paused downgrade, waiting_signal unchanged, waiting_hitl unchanged, terminal unchanged, atomicity, idempotency.
- Integration tests `workflow_run_should_survive_restart`, `waiting_run_should_survive_restart_and_remain_resumable`, `boot_should_create_compozy_db_with_all_durable_runtime_tables`, `boot_should_recover_running_workflow_runs_before_cache_projection`, `compozy_db_and_runtime_db_should_coexist_after_both_boot` are present in `dual_database_boot_test.rs`.

### Missing / Divergent from Spec

**Status value divergence in initial migration**: The spec requires the initial `workflow_run` status CHECK constraint to include `waiting_signal` and `paused` as distinct values. The actual `0002_workflow_run_core.sql` has `status IN ('pending', 'running', 'waiting', 'completed', 'failed', 'cancelled', 'interrupted')` — neither `waiting_signal` nor `paused` are in the initial migration. These were added later via migration `0006` (which adds `waiting_signal` and `0007`). This is a deviation from the spec's `workflow_run` column spec which lists `waiting_signal` and `paused` as values for the Phase 1 migration. The initial migration also includes `interrupted` which the spec does not list for Phase 1.

**`create_run()` method naming**: The spec requires `create_run()` that combines run insert + `run_created` checkpoint in a single transaction. The implementation uses `insert_run()` (inserts just the row) and `insert_checkpoint()` separately, without an atomic combined `create_run()` API. The combined write exists in `WorkflowEngine` code above the store layer, but the store itself does not expose a `create_run()` that atomically writes both.

**Missing spec-named unit tests** (functional coverage exists but names differ):
- `compozy_db_migration_should_create_workflow_run_table()` — not present by this name
- `compozy_db_migration_should_create_workflow_checkpoint_table()` — not present by this name
- `compozy_db_migration_should_create_workflow_signal_table()` — not present by this name
- `compozy_db_migration_should_create_all_required_indexes()` — not present; no test queries `sqlite_master WHERE type='index'` to verify all required index names
- `workflow_run_store_should_create_run_and_write_run_created_checkpoint()` — not present by this name; `workflow_run_repository_should_insert_and_load_rows` covers insert but does not verify atomic run+checkpoint pair
- `workflow_run_store_transition_should_write_run_and_checkpoint_atomically()` — present as `workflow_run_repository_should_persist_checkpoint_before_row_update_atomically`
- `recovery_scan_should_downgrade_running_to_paused_and_write_checkpoint()` — present as `running_runs_downgrade_to_paused_on_recovery_scan`
- `recovery_scan_should_not_modify_waiting_signal_runs()` — present as `waiting_signal_runs_survive_recovery_scan_unchanged`

**`boot_should_run_recovery_scan_before_workflow_engine_starts()`** — functionally covered by `boot_should_recover_running_workflow_runs_before_cache_projection`, but that test name does not match the spec.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260321_002_workflow_run_core.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260321_003_workflow_checkpoint.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260321_004_workflow_signal.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/workflow_store.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` (lines 657–779)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/tests/dual_database_boot_test.rs`
