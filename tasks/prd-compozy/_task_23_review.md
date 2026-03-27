# Task 23 Review: agent_dispatch Schema And Persistence Layer

## Status: PASS

## Checklist
- [x] Migration `20260324_008_agent_dispatch.sql` adds `agent_dispatch` table with all required columns — all 18 columns present including reserved `provider_driver`, `session_id`, `provider_resume_token`
- [x] `CHECK` constraint on `status` column covers all six states — present in SQL
- [x] `CHECK` constraint on `kind` column covers all three modes — present
- [x] `CHECK` constraint linking terminal `status` to non-null `completed_at` — present
- [x] Indexes: `idx_dispatch_run`, `idx_dispatch_parent`, `idx_dispatch_status` — all present in migration SQL
- [x] `DispatchKind` enum (`Call`, `Send`, `Spawn`) with `Display`, `FromStr`, `serde` derives — implemented in `dispatch.rs`
- [x] `DispatchStatus` enum (`Pending`, `Running`, `WaitingHitl`, `Completed`, `Failed`, `Cancelled`) with `Display`, `FromStr`, `serde` derives — implemented
- [x] `DispatchRecord` struct with all schema columns — implemented including optional resume columns
- [x] `DispatchRepository` async trait (`Send + Sync`) with `create`, `find_by_id`, `find_by_run`, `find_children`, `update_status`, `update_result`, `update_error`, `mark_completed`, `mark_failed` — all present
- [x] `increment_attempt` method — present (in addition to the spec-required methods)
- [x] `SqliteDispatchRepository` backed by `Arc<Mutex<Connection>>` — implemented consistently with WAL pattern
- [x] Legal status transitions enforced in `validate_status_transition` — comprehensive state machine including `Pending→Running`, `Running→WaitingHitl`, `WaitingHitl→Running`, `Running→Completed`, `Running→Failed`, `Running→Cancelled`, `Failed→Pending` (retry), `Cancelled→Pending` (retry)
- [x] JSON columns use `serde_json` serialization — `input_json`, `result_json`, `error_json`
- [x] Parameterized queries throughout — no string interpolation in SQL
- [x] Parent-child lineage via `parent_dispatch_id` + `find_children` — implemented
- [x] Attempt counter increments correctly on retry — `increment_attempt` resets to `Pending` and increments counter
- [x] `DispatchSummaryRecord` for run-scoped list surfaces — present
- [x] Unit tests: `dispatch_record_should_persist_all_required_fields`, `dispatch_parent_child_linkage_should_persist_and_be_queryable`, `dispatch_status_transitions_should_enforce_legality`, `dispatch_attempt_counter_should_increment_on_retry`, `dispatch_kind_spawn_should_store_spawned_agent_id`, `dispatch_find_by_run_should_return_all_run_dispatches`, `dispatch_of_kind_send_should_not_require_result` — all present in `dispatch.rs`
- [x] Integration tests: `compozy_db_migration_should_add_dispatch_table_cleanly`, `compozy_db_migration_should_add_hitl_table_cleanly` (covers indexes), `dispatch_repository_should_survive_connection_restart`, `dispatch_repository_should_handle_concurrent_status_updates` — all present
- [x] `compozy_db_migration_should_be_idempotent` — covered by `compozy_db_migration_should_be_idempotent_with_hitl_table` which runs migrations twice and asserts both `agent_dispatch` and `hitl_request` tables exist without error; `CREATE TABLE IF NOT EXISTS` ensures SQL-level idempotency

## Findings
- All deliverables fully implemented. The repository layer is clean, using the same `Arc<Mutex<Connection>>` with WAL pattern established in `substrate.rs`.
- Status transition validation is thorough: it guards immutable fields (`dispatch_id`, `run_id`, `kind`, `input_json`, etc.), validates attempt counter increments, and enforces terminal-state `completed_at` consistency.
- The `payload_update` guard (only allows `update_result`/`update_error` while `Running` or `WaitingHitl`) is an additional correctness safeguard beyond the spec minimum.
- The concurrent update test uses an `Arc<Barrier>` pattern to reliably race two status transitions and asserts exactly one succeeds and one fails with `UnexpectedDispatchState`, verifying the optimistic-lock design.
- The spec task name for the standalone idempotency test (`compozy_db_migration_should_be_idempotent`) does not have an exact name match, but the functionality is covered by `compozy_db_migration_should_be_idempotent_with_hitl_table` which runs all migrations twice.
- The `WorkflowStoreSet` in `workflow_store.rs` integrates `SqliteDispatchRepository` as `stores.dispatch`, making it available kernel-wide.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/dispatch.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260324_008_agent_dispatch.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/workflow_store.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/db_migration.rs`
