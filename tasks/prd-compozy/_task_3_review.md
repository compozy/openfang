# Task 3 Review: Reusable Migration Runner For Both Databases

## Status: PASS

## Checklist
- [x] `MigrationStep` type defined in `crates/openfang-kernel/src/db_migration.rs` with `version: u32`, `name: &str`, `sql: &str`
- [x] `DatabaseIdentity` enum (`Runtime` / `Compozy`) used to tag which database a stream belongs to — prevents accidental cross-application
- [x] `run_migrations(conn, database, steps)` is a single parameterized function (not two copy-pasted functions)
- [x] `schema_migration` table bootstrapped with `CREATE TABLE IF NOT EXISTS` before the user steps run — idempotent
- [x] Core migration loop: steps sorted by version, already-applied versions skipped, each new step executed in a transaction with rollback on failure
- [x] Applied step recorded with `name` and `applied_at = datetime('now')` in `schema_migration`
- [x] `MigrationError` defined with `thiserror`: `SchemaBootstrapFailed`, `AlreadyApplied`, `ExecutionFailed { version, name, source }`, `VersionQueryFailed`
- [x] Runner wired into `boot_with_config()` via `apply_migration_stream()`: runtime.db migrations applied before compozy.db migrations
- [x] Bootstrap migration slice for runtime.db (step 1 = `schema_migrations_bootstrap`) and compozy.db (same step 1) both present
- [x] Existing `run_migrations()` in `MemorySubstrate::open()` is not removed or modified
- [x] `PRAGMA user_version` not used anywhere in the new runner
- [x] No `spawn_blocking` inside the runner — sync function; async wrapping at call site
- [x] All 8 required unit tests present with correct names
- [x] All 6 required integration tests present (in `kernel.rs` tests and `dual_database_boot_test.rs`)
- [x] Regression guards: no two-function duplication, no error swallowing, no `PRAGMA user_version`, migrations propagated as `?` to `KernelError::BootFailed`

## Findings

**Correctly implemented:**
- The runner is genuinely reusable: a single `run_migrations()` function parameterized by `DatabaseIdentity` and an ordered `&[MigrationStep<'_>]` slice. Adding new migrations never requires touching the runner.
- The `DatabaseIdentity` enum is an elegant solution to the spec's requirement that the runner's API must "require an explicit database identity parameter or use distinct stream types."
- Sorting by version before applying ensures out-of-order slices are handled correctly (tested by `migration_runner_should_apply_steps_in_version_order`).
- Transaction semantics: each step runs inside `conn.unchecked_transaction()`, so DDL from a failing step is rolled back (tested).
- The runner's schema bootstrap (`ensure_schema_migration_table`) is called unconditionally before the step loop, satisfying the idempotency requirement.
- Rollback test correctly verifies that a duplicate `CREATE TABLE` within one step causes full rollback of that step's DDL.

**Minor notes:**
- The migration slices in `db_migration.rs` include far more than the bootstrap step (they include Task 6 and Task 9 migrations too, since those tasks ran). This is correct sequencing but means the "stub slice" from Task 3's deliverable is never seen in isolation — not a defect, just a consequence of later tasks building on this.
- `migration_runner_should_roll_back_failed_step` uses a duplicate `CREATE TABLE` statement as the failure trigger; this correctly verifies rollback since the second `CREATE TABLE` fails after the first succeeds within the same batch.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/db_migration.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/substrate.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/tests/dual_database_boot_test.rs`
