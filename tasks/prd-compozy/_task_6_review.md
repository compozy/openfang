# Task 6 Review: Initial runtime.db Schema And Stores

## Status: PASS

## Checklist
- [x] Migration SQL for `agent_runtime` (step 2) present: `20260321_002_agent_runtime_core.sql` embedded via `include_str!`
- [x] Migration SQL for `agent_session` + `agent_message` (step 3) present: `20260321_003_agent_sessions_and_messages.sql`
- [x] Migration SQL for `schedule_runtime` + `schedule_execution` (step 4) present: `20260321_004_schedule_runtime_core.sql`
- [x] All 5 required indexes present: `idx_agent_session_agent_id`, `idx_agent_message_agent_session`, `idx_agent_message_session`, `idx_schedule_execution_schedule`, `idx_schedule_execution_fired`
- [x] Migration steps registered in `RUNTIME_BOOTSTRAP_MIGRATIONS` slice in `db_migration.rs`
- [x] `schema_migration` bootstrap step is step 1 in the runtime slice (verified in `db_migration.rs`)
- [x] `AgentRuntimeStore` adapter in `crates/openfang-memory/src/runtime_store.rs` with `upsert_agent_runtime()`, `get_agent_runtime()`, `list_agent_runtimes()`, `remove_agent_runtime()`
- [x] `AgentSessionStore` and `AgentMessageStore` adapters present
- [x] `ScheduleRuntimeStore` and `ScheduleExecutionStore` adapters present
- [x] `TriggerRuntimeStore` also included (extra migration step 5 for triggers, beyond Task 6 scope — not a defect)
- [x] `RuntimeStoreSet` bundles all runtime stores and is initialized in `boot_with_config()` after migrations
- [x] `OpenFangKernel` struct has `pub runtime_stores: RuntimeStoreSet` field
- [x] `OpenFangKernel` struct has `pub workflow_stores: WorkflowStoreSet` field (compozy.db handle)
- [x] Scheduler (cron subsystem in `cron.rs`) wired to `ScheduleRuntimeStore` and `ScheduleExecutionStore` via `attach_runtime_stores()` in `boot_with_config()`
- [x] `/api/health` endpoint uses `runtime_stores.agent_runtime.list_agent_runtimes()` for runtime projection check
- [x] Store adapters wrap `Arc<Mutex<Connection>>` — synchronous, no `&mut self`, errors mapped to `OpenFangError`
- [x] All 9 required unit tests present in `crates/openfang-memory/src/runtime_store.rs`
- [x] Integration tests present in `crates/openfang-kernel/tests/dual_database_boot_test.rs`: `boot_should_create_runtime_db_with_all_initial_tables`, `agent_runtime_state_should_survive_restart`, `schedule_runtime_state_should_survive_restart`, `existing_boot_path_should_not_regress`
- [x] `agent_runtime` table contains no definition columns (`system_prompt`, `model`, `skills` absent — verified by `runtime_db_migration_should_not_include_agent_definition_fields` test)
- [x] No `compozy.db` domain tables appear in runtime migration SQL (verified by `runtime_db_migration_should_not_include_compozy_domain_tables` test)
- [x] Legacy `MemorySubstrate::open()` and its `run_migrations()` call unchanged

## Findings

**Correctly implemented:**
- The runtime store module (`runtime_store.rs`) follows the `structured.rs` reference pattern exactly: `Arc<Mutex<Connection>>` wrapping, synchronous methods, error mapping to `OpenFangError::Memory`.
- The cron scheduler receives the stores via `attach_runtime_stores()` immediately after initialization in `boot_with_config()`. The stores are `Option<...>` in `CronScheduler` and degrade gracefully to no-ops when `None` (satisfying the "best-effort" regression guard: scheduler must not crash on a store write failure).
- The `db_migration.rs` guard test `runtime_db_migration_should_not_include_agent_definition_fields` checks for `system_prompt`, `skills`, `model`, `agent_name`, `target_agent`, `cron_expression` — confirming the ownership boundary between file-backed definitions and runtime projections is enforced at the migration level.
- `boot_should_create_runtime_db_with_all_initial_tables` in the integration test file verifies all expected tables exist in the real on-disk database after boot.

**Minor notes:**
- The implementation added a 5th runtime migration (`0005_trigger_runtime_core`) that was not in Task 6's spec. This is scope creep from a later task but is harmless and correctly scoped to runtime.db (not compozy.db).
- The task spec referred to `crates/openfang-kernel/src/scheduler.rs` as the target to wire `ScheduleRuntimeStore`. The actual wiring is in `crates/openfang-kernel/src/cron.rs` (the `CronScheduler`). This matches the actual codebase architecture — `scheduler.rs` contains `AgentScheduler` for execution ordering while `cron.rs` contains `CronScheduler` for time-based scheduling. The correct file was modified.
- No raw connections are exposed outside `openfang-memory` or `openfang-kernel`; all external access goes through typed store methods.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/runtime_store.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/lib.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/db_migration.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/cron.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/tests/dual_database_boot_test.rs`
