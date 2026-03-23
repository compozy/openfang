## markdown

## status: pending

<task_context>
<domain>engine/infra/migrations</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task1,task2</dependencies>
</task_context>

# Task 3.0: Reusable Migration Runner For Both Databases

## Overview

Extract or adapt the existing SQLite migration pattern into a reusable runner
for both `runtime.db` and `compozy.db`.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Per INITIAL-RUNTIME-MIGRATIONS.md section 2, each database must have its own independent migration stream. Migration version numbers are per-database, not global. A migration numbered `0001` in `runtime.db` is distinct from migration `0001` in `compozy.db`.
- Per INITIAL-RUNTIME-MIGRATIONS.md section 2, each migration file must be small, monotonic, and independently idempotent at the runner level. The runner must check whether a migration has already been applied before executing it, and must not re-apply already-applied migrations.
- Per ADR-050, both databases require a `schema_migration` tracking table. This table is the first migration applied to each database (`0001_schema_migrations.sql` in each stream). The runner must create this table if it does not exist before querying it.
- The migration runner must surface migration failure as an explicit typed error, not a log warning followed by continued boot. Per INITIAL-RUNTIME-MIGRATIONS.md section 4 Phase 0 exit criteria, "partial migration failure stops startup clearly."
- The runner must be designed so that later tasks can add new migration files to either stream without touching the runner implementation itself. The runner must discover or accept migrations as an ordered slice, not as hard-coded version-specific branches.
- The existing `run_migrations()` function in `crates/openfang-memory/src/migration.rs` uses SQLite's `PRAGMA user_version` as the version store. The new runner must use a dedicated `schema_migration` table per the design docs, not `PRAGMA user_version`. The existing function is a reference for patterns, not the final shape.
- The migration runner must be usable synchronously (the existing `MemorySubstrate` and `rusqlite` stack is sync). Async wrapping via `tokio::task::spawn_blocking` belongs at the call site in `boot_with_config()`, not inside the runner itself.
- Per STORAGE-MODEL.md section 6, the runner must be structured so that `runtime.db` and `compozy.db` migrations can never be accidentally applied to the wrong database. The runner API should require an explicit database identity parameter or use distinct stream types.
</requirements>

## Subtasks

- [ ] 3.1 Audit `crates/openfang-memory/src/migration.rs` to extract the reusable patterns: the version-check-then-apply loop, the `column_exists()` safety check, and the `execute_batch()` style. Identify what must change: replace `PRAGMA user_version` with a `schema_migration` table, replace the hardcoded version ladder with an ordered slice of migration descriptors, and extract error handling into a proper `MigrationError` type.
- [ ] 3.2 Define a `MigrationStep` type (or equivalent) in a new module — e.g. `crates/openfang-kernel/src/db_migration.rs` or a dedicated `crates/openfang-migrate-schema/` crate — that holds a version number, a human-readable name, and a SQL string or callable. The runner takes `&[MigrationStep]` (or equivalent ordered collection) and a `&Connection`.
- [ ] 3.3 Implement the `schema_migration` bootstrap: before running any user-provided migration steps, the runner must ensure `CREATE TABLE IF NOT EXISTS schema_migration (version INTEGER PRIMARY KEY, name TEXT NOT NULL, applied_at TEXT NOT NULL)` exists in the target database. This is the `0001_schema_migrations` step for both databases per INITIAL-RUNTIME-MIGRATIONS.md section 3.
- [ ] 3.4 Implement the core migration loop: for each `MigrationStep`, check if its version is already in `schema_migration`; if not, execute its SQL inside a transaction, then insert a row into `schema_migration` with `applied_at = datetime('now')`. A failed SQL execution must roll back and surface `MigrationError`.
- [ ] 3.5 Wire the migration runner into `boot_with_config()` in `crates/openfang-kernel/src/kernel.rs` immediately after both databases are opened (Task 2's output). Apply the `runtime.db` migration stream first, then the `compozy.db` stream. For this task, each stream may be empty or contain only the bootstrap `schema_migration` step — later tasks add the actual schema migrations.
- [ ] 3.6 Define a `MigrationError` type (with `thiserror`) covering: `SchemaBootstrapFailed`, `AlreadyApplied` (for defensive use), `ExecutionFailed { version, name, source }`, `VersionQueryFailed`. Wire it into `KernelError::BootFailed` at the call site.
- [ ] 3.7 Write unit tests for the migration runner against in-memory SQLite connections (`Connection::open_in_memory()`), covering: ordering, idempotency, failure propagation, and the `schema_migration` table bootstrap.
      </requirements>

## Implementation Details

The goal is a reliable, per-database migration runner that matches the
dual-database ownership model. It is not a generic framework for every future
use case.

### Current State

The existing migration infrastructure in `crates/openfang-memory/src/migration.rs`
is a monolithic function `run_migrations(conn: &Connection) -> Result<(), rusqlite::Error>`
that uses `PRAGMA user_version` as its version store (lines ~51-54) and a
hardcoded version ladder of `if current_version < N { migrate_vN(conn)?; }`
branches (lines ~14-47). It is tied to the `openfang-memory` schema and
cannot be cleanly reused for `compozy.db` without significant modification.

The `openfang-migrate` crate (`crates/openfang-migrate/src/lib.rs`) is a
separate migration tool for importing from other agent frameworks (OpenClaw,
LangChain) — it is not a SQLite schema migration runner and must not be
confused with this task's output.

### What Needs To Change

- A new migration runner lives in a location that is accessible to
  `openfang-kernel` without creating a circular dependency. Options:
  - A new module inside `crates/openfang-kernel/src/` (e.g. `db_migration.rs`)
  - A new internal-only crate (e.g. `crates/openfang-db/`) that both the
    kernel and memory crate can depend on
  - An extension to `crates/openfang-memory/src/` that exports the runner
    separately from the existing migration module

  The simplest choice that does not create circular dependencies should be
  preferred. Check `./scripts/check-deps.sh` after wiring.

- The `schema_migration` table schema per INITIAL-RUNTIME-MIGRATIONS.md:

  ```
  schema_migration (
    version   INTEGER PRIMARY KEY,
    name      TEXT    NOT NULL,
    applied_at TEXT   NOT NULL
  )
  ```

- Migration steps for this task (the bootstrap slice for each database):
  - `runtime.db` stream: one step — version 1, name `schema_migrations_bootstrap`,
    SQL creates the `schema_migration` table.
  - `compozy.db` stream: same one step.
  - Later tasks (Task 6 and Task 9) add their steps to each stream.

- The runner API should look roughly like:
  ```rust
  pub fn run_migrations(
      conn: &Connection,
      steps: &[MigrationStep],
  ) -> Result<(), MigrationError>
  ```
  where `MigrationStep` carries at minimum `{ version: u32, name: &str, sql: &str }`.

### Differences From The Existing Pattern

The existing `run_migrations` in `openfang-memory`:

- Uses `PRAGMA user_version` (a single global integer) — fragile because a
  wrong pragma value silently skips or re-runs migrations.
- Uses in-function hardcoded branches — adding a migration requires editing
  the runner function.
- Mixes the bootstrap step with user migration steps.
- Does not record a human-readable name or timestamp for applied migrations.

The new runner:

- Uses a `schema_migration` table — each applied migration is a named,
  timestamped row.
- Takes an external ordered slice — the runner function is stable; migration
  authors add steps to the caller's slice.
- Separates the table bootstrap from user migration steps.
- Records name and timestamp for observability.
- Returns a typed error rather than bare `rusqlite::Error`.

### Integration Points

- `crates/openfang-kernel/src/kernel.rs` — `boot_with_config()` calls the
  runner twice: once for `runtime.db` after Task 2's connection open, once
  for `compozy.db`. The call site wraps errors with
  `KernelError::BootFailed(format!("runtime.db migration failed: {e}"))`.
- `crates/openfang-kernel/src/error.rs` — `KernelError::BootFailed(String)`
  already covers migration failures; no new variant is required.
- `crates/openfang-memory/src/migration.rs` — the existing `run_migrations`
  is not removed by this task; it continues to be called by `MemorySubstrate::open()`
  for the legacy schema. The new runner is additive, not a replacement yet.
  Task 6 will eventually unify them.
- `./scripts/check-deps.sh` — must be run after placing the new runner to
  confirm no circular dependencies are introduced.

### Migration File Structure

Per INITIAL-RUNTIME-MIGRATIONS.md section 2, the recommended structure:

```
migrations/
  runtime/
    0001_schema_migrations.sql      ← Task 3 bootstraps this
    0002_agent_runtime_core.sql     ← Task 6 adds this
    0003_agent_sessions_and_messages.sql  ← Task 6 adds this
    0004_schedule_runtime_core.sql  ← Task 6 adds this
  compozy/
    0001_schema_migrations.sql      ← Task 3 bootstraps this
    0002_workflow_run_core.sql      ← Task 9 adds this
    0003_workflow_checkpoint.sql    ← Task 9 adds this
    0004_workflow_signal.sql        ← Task 9 adds this
```

The SQL files may be embedded at compile time using `include_str!()` or
constructed as string constants. The runner does not care how the SQL arrives
as long as it receives an ordered slice of `MigrationStep` values.

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/migration.rs` — existing migration pattern; reference, not the final shape
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/substrate.rs` — calls `run_migrations()` at line ~44; this call must remain until Task 6 unifies
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — boot wiring location
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/error.rs` — `KernelError` type
- `/Users/pedronauck/Dev/compozy/openfang/scripts/check-deps.sh` — dependency graph validation
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/INITIAL-RUNTIME-MIGRATIONS.md` — migration structure spec (sections 2, 3, 4)

### Dependent Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — wires the runner; Task 6 adds `runtime.db` migration steps here
- Migration step definitions for `runtime.db` (Task 6) and `compozy.db` (Task 9) will be passed to the runner from `boot_with_config()`

## Deliverables

- A reusable migration runner function or struct accessible to `openfang-kernel`
- `MigrationStep` and `MigrationError` types
- `schema_migration` table bootstrap for both databases
- Migration runner wired into `boot_with_config()` for both databases
- Unit tests for ordering, idempotency, and failure propagation

## Tests

### Unit Tests (Required)

- [ ] `migration_runner_should_apply_steps_in_version_order()` — given steps with versions [3, 1, 2] passed out of order, the runner must apply them in ascending version order.
- [ ] `migration_runner_should_skip_already_applied_steps()` — run the same set of steps twice; confirm no error on the second run and the `schema_migration` table has the same row count.
- [ ] `migration_runner_should_record_name_and_applied_at_for_each_step()` — after a successful run, query `schema_migration` and confirm each applied step has a non-empty `name` and a valid `applied_at` timestamp.
- [ ] `migration_runner_should_surface_failure_from_bad_sql()` — pass a step with invalid SQL; confirm the runner returns `MigrationError::ExecutionFailed` and does not insert a row into `schema_migration` for that step.
- [ ] `migration_runner_should_roll_back_failed_step()` — after a failed step, confirm any partial DDL changes from that step are rolled back (use a table creation that partially succeeds then fails).
- [ ] `schema_migration_bootstrap_should_be_idempotent()` — calling the runner on a database that already has `schema_migration` must not fail, even if the bootstrap step is included in the slice again.
- [ ] `migration_runner_should_bootstrap_schema_migration_table_if_absent()` — run the runner on a completely empty in-memory database; confirm `schema_migration` is created and the first step's row is recorded.
- [ ] `migration_error_should_carry_failing_version_and_name()` — confirm that `MigrationError::ExecutionFailed` exposes the `version` and `name` of the failing step.

### Integration Tests (Required)

- [ ] `boot_should_apply_runtime_db_migrations_before_compozy_db()` — in `boot_with_config()`, instrument or inspect the order; confirm `schema_migration` exists in `runtime.db` before `compozy.db` migration begins.
- [ ] `boot_should_create_schema_migration_in_both_databases()` — after a successful boot, open both database files and confirm `schema_migration` table exists in each with at least one row.
- [ ] `boot_should_fail_clearly_when_runtime_db_migration_fails()` — inject a bad SQL step into the `runtime.db` stream; confirm `boot_with_config()` returns `KernelError::BootFailed` containing `"runtime.db"`.
- [ ] `boot_should_fail_clearly_when_compozy_db_migration_fails()` — same for `compozy.db`.
- [ ] `second_boot_against_migrated_databases_succeeds_without_error()` — run `boot_with_config()` twice against the same `data_dir`; second boot must succeed with no migration re-application errors.
- [ ] `migration_status_is_queryable_after_boot()` — confirm that a query against `schema_migration` in each database returns the expected applied migration records after boot.

### Regression and Anti-Pattern Guards

- [ ] The runner must not be implemented as two separate, copy-pasted functions — one for `runtime.db` and one for `compozy.db`. Confirm there is a single runner function or struct parameterized by the migration slice.
- [ ] The runner must not swallow a migration failure and return `Ok(())`. Any `rusqlite::Error` from executing a migration step must be propagated as `MigrationError::ExecutionFailed`.
- [ ] The runner must not rely on `PRAGMA user_version` as its version store. Confirm there is no `pragma_update(None, "user_version", ...)` call in the new runner.
- [ ] Boot must not continue with normal subsystem construction if either migration stream returns an error. Confirm that the `?` operator or equivalent is used to propagate `MigrationError` into `KernelError::BootFailed` at the boot call site.
- [ ] The new runner must not modify or replace the existing `run_migrations()` call in `MemorySubstrate::open()`. That call remains until Task 6 unifies the schemas.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- A single reusable migration runner function or type handles both databases via their respective migration slices.
- Both databases have a `schema_migration` table after the first boot.
- Migration application is deterministic, ordered, and idempotent across multiple boots.
- A failing migration stops boot immediately with a named, database-specific error.
- Task 6 and Task 9 can add new migration steps to their respective slices without modifying the runner implementation.
- The dependency graph validation (`./scripts/check-deps.sh`) passes with no new circular dependencies.

---

## Notes

- The current `openfang-memory` migration code is the reference, not the final shape.
