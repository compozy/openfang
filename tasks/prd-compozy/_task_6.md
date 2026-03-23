## markdown

## status: pending

<task_context>
<domain>engine/infra/runtime-db</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task2,task3</dependencies>
</task_context>

# Task 6.0: Initial runtime.db Schema And Stores

## Overview

Create the first `runtime.db` migration stream and wire the initial runtime
stores for agents and schedules.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Per ADR-050, the initial `runtime.db` migration stream covers exactly five tables in this order: `schema_migration` (bootstrap, Task 3), `agent_runtime`, `agent_session`, `agent_message`, `schedule_runtime`, and `schedule_execution`. No other tables are introduced in this task.
- Per STORAGE-MODEL.md section 3, `runtime.db` owns only platform-core operational state. Tables for workflow execution (`workflow_run`, `workflow_checkpoint`, `workflow_signal`), product domain objects (`task`, `subtask`, `artifact`), and looper execution (`looper_run`) must not appear in `runtime.db` migrations — those belong to `compozy.db`.
- Per STORAGE-MODEL.md section 5, `runtime.db` must not store a competing authoritative copy of agent or schedule definitions. The `agent_runtime` table stores the runtime projection (state, mode, health, active session) but not the definition content from the TOML file. `schedule_runtime` stores runtime state (last run, next run, enabled) but not schedule definition fields (cron expression, target agent, etc.).
- Per STORAGE-MODEL.md section 6, no cross-database SQL joins are permitted. The new `runtime.db` tables must not contain foreign keys that reference `compozy.db` tables, and vice versa. Cross-table relationships are resolved in application code using the shared stable agent/schedule IDs.
- The `agent_runtime`, `agent_session`, and `agent_message` tables must carry the minimum fields specified in INITIAL-RUNTIME-MIGRATIONS.md section 3 (migrations `0002`, `0003`). Columns may be added to match the DATABASE-SCHEMA.md outline but must not go beyond the fields listed there for this first cut.
- The `schedule_runtime` and `schedule_execution` tables must carry the minimum fields specified in INITIAL-RUNTIME-MIGRATIONS.md section 3 (migration `0004`).
- Per IMPLEMENTATION-PLAN.md section 6, the migration set must include at minimum these indexes: on `agent_session` by `agent_id`; on `agent_message` by `agent_id` and `session_id`; on `schedule_execution` by `schedule_id`. Index creation must be part of the migration SQL, not deferred.
- Repository or store adapter types for `agent_runtime`, `agent_session`, `agent_message`, `schedule_runtime`, and `schedule_execution` must be introduced so that kernel subsystems read and write through a typed API rather than issuing ad-hoc SQL against the `runtime.db` connection. These adapters follow the existing pattern in `crates/openfang-memory/src/structured.rs` and `crates/openfang-memory/src/session.rs`.
</requirements>

## Subtasks

- [ ] 6.1 Write the four `runtime.db` migration SQL files (or inline SQL constants) per INITIAL-RUNTIME-MIGRATIONS.md section 3: `0002_agent_runtime_core`, `0003_agent_sessions_and_messages`, `0004_schedule_runtime_core`. Register them as `MigrationStep` entries in the `runtime.db` migration slice that `boot_with_config()` passes to the Task 3 runner. Confirm the `schema_migration` bootstrap step from Task 3 is already the first step in the slice.
- [ ] 6.2 Define the `AgentRuntimeStore` adapter in a new module (e.g. `crates/openfang-memory/src/agent_runtime.rs` or a new `crates/openfang-runtime-store/` crate) wrapping `Arc<Mutex<Connection>>` for `runtime.db`. Implement at minimum: `upsert_agent_runtime()`, `get_agent_runtime()`, `list_agent_runtimes()`, `remove_agent_runtime()`.
- [ ] 6.3 Define the `AgentSessionStore` and `AgentMessageStore` adapters for the `agent_session` and `agent_message` tables in the same module. These replace or supplement the existing `SessionStore` in `crates/openfang-memory/src/session.rs` for the new schema. Confirm the existing `SessionStore` is not broken for the legacy schema path.
- [ ] 6.4 Define the `ScheduleRuntimeStore` and `ScheduleExecutionStore` adapters for `schedule_runtime` and `schedule_execution`. The scheduler subsystem at `crates/openfang-kernel/src/scheduler.rs` currently holds schedule runtime state in memory; wire it to use `ScheduleRuntimeStore` for persistence.
- [ ] 6.5 Add the new store handles to `OpenFangKernel` (alongside the existing `memory: Arc<MemorySubstrate>` and the new `compozy_db` handle from Task 2). The runtime stores are initialized in `boot_with_config()` after the `runtime.db` migrations succeed.
- [ ] 6.6 Align the `/api/health` endpoint and any runtime state endpoints in `crates/openfang-api/src/routes.rs` to use the new `AgentRuntimeStore` for agent state reads, rather than the in-memory registry alone where applicable.
- [ ] 6.7 Write migration tests (against in-memory `runtime.db`) confirming each table is created with the correct columns and indexes, and write store adapter tests confirming round-trip reads and writes for each new table.
      </requirements>

## Implementation Details

This task covers only the initial `runtime.db` tables for platform-core runtime
ownership. It must not introduce product-domain tables or workflow execution
tables.

### Current State

Runtime state for agents is currently stored in two places:

1. `crates/openfang-memory/src/substrate.rs` — the `MemorySubstrate` opens
   `openfang.db` and runs `run_migrations()` from `crates/openfang-memory/src/migration.rs`.
   The existing schema in `migration.rs` has: `agents`, `sessions`, `events`,
   `kv_store`, `task_queue`, `memories`, `entities`, `relations`, `migrations`,
   `usage_events`, `canonical_sessions`, `paired_devices`, `audit_entries`.

2. The scheduler at `crates/openfang-kernel/src/scheduler.rs` holds schedule
   state in-memory only.

None of these tables map cleanly to the new ownership model. The new
`runtime.db` tables (`agent_runtime`, `agent_session`, `agent_message`,
`schedule_runtime`, `schedule_execution`) are new tables with new schemas,
not renames of existing tables.

The `MemorySubstrate` and its existing schema remain in place for this task.
The new `runtime.db` tables are created alongside it via the Task 3 migration
runner. Full unification of the legacy schema with the new runtime tables is
deferred — this task adds the new tables; it does not remove the old ones.

### Table Schemas (Minimum Fields)

Per DATABASE-SCHEMA.md and INITIAL-RUNTIME-MIGRATIONS.md section 3:

**`agent_runtime`** (migration `0002`):

- `agent_id TEXT PRIMARY KEY`
- `loaded INTEGER NOT NULL DEFAULT 0` (boolean)
- `state TEXT NOT NULL`
- `mode TEXT NOT NULL`
- `healthy INTEGER NOT NULL DEFAULT 1`
- `active_session_id TEXT`
- `active_dispatches INTEGER NOT NULL DEFAULT 0`
- `last_active_at TEXT`
- `updated_at TEXT NOT NULL`

**`agent_session`** (migration `0003`):

- `session_id TEXT PRIMARY KEY`
- `agent_id TEXT NOT NULL`
- `label TEXT`
- `active INTEGER NOT NULL DEFAULT 1`
- `message_count INTEGER NOT NULL DEFAULT 0`
- `dispatch_count INTEGER NOT NULL DEFAULT 0`
- `created_at TEXT NOT NULL`
- `updated_at TEXT NOT NULL`
- `compacted_at TEXT`

**`agent_message`** (migration `0003`):

- `message_id TEXT PRIMARY KEY`
- `agent_id TEXT NOT NULL`
- `session_id TEXT NOT NULL`
- `direction TEXT NOT NULL`
- `payload_json TEXT NOT NULL`
- `status TEXT NOT NULL`
- `created_at TEXT NOT NULL`
- `completed_at TEXT`

**`schedule_runtime`** (migration `0004`):

- `schedule_id TEXT PRIMARY KEY`
- `enabled INTEGER NOT NULL DEFAULT 1`
- `last_run TEXT`
- `next_run TEXT`
- `last_status TEXT`
- `consecutive_errors INTEGER NOT NULL DEFAULT 0`
- `one_shot INTEGER NOT NULL DEFAULT 0`
- `updated_at TEXT NOT NULL`

**`schedule_execution`** (migration `0004`):

- `execution_id TEXT PRIMARY KEY`
- `schedule_id TEXT NOT NULL`
- `fired_at TEXT NOT NULL`
- `status TEXT NOT NULL`
- `effect_json TEXT`
- `error TEXT`

### Required Indexes

Per IMPLEMENTATION-PLAN.md section 6:

- `CREATE INDEX idx_agent_session_agent_id ON agent_session(agent_id)`
- `CREATE INDEX idx_agent_message_agent_session ON agent_message(agent_id, session_id)`
- `CREATE INDEX idx_agent_message_session ON agent_message(session_id)`
- `CREATE INDEX idx_schedule_execution_schedule ON schedule_execution(schedule_id)`
- `CREATE INDEX idx_schedule_execution_fired ON schedule_execution(fired_at)`

### Store Adapter Pattern

Follow the pattern in `crates/openfang-memory/src/structured.rs`:

- Wrap `Arc<Mutex<rusqlite::Connection>>` (for the `runtime.db` connection from Task 2)
- Methods are synchronous (the async wrapping with `spawn_blocking` goes at the call site)
- Each method maps `rusqlite::Error` to `OpenFangError::Memory` or a new `RuntimeStoreError` with `thiserror`
- No method takes `&mut self`; the connection handle is interior-mutable via the `Mutex`

### Integration Points

- `crates/openfang-kernel/src/kernel.rs` — `boot_with_config()` creates the
  store adapters after migrations succeed and stores them in `OpenFangKernel`
  fields. The scheduler at `crates/openfang-kernel/src/scheduler.rs` receives
  `ScheduleRuntimeStore` during construction.
- `crates/openfang-memory/src/migration.rs` — the existing `run_migrations()`
  must not be modified. The new `runtime.db` schema lives in a separate
  migration slice passed to the Task 3 runner.
- `crates/openfang-memory/src/session.rs` — `SessionStore` continues to serve
  the legacy `sessions` table in `openfang.db`. The new `AgentSessionStore`
  serves the `agent_session` table in `runtime.db`. Both coexist until the
  full unification is scoped.
- `crates/openfang-api/src/routes.rs` — agent list, agent detail, and health
  endpoints may read from `AgentRuntimeStore` for the durable runtime
  projection instead of (or in addition to) the in-memory registry.

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/migration.rs` — existing migration pattern; must not be removed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/session.rs` — existing `SessionStore`; coexists with new `AgentSessionStore`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/structured.rs` — reference store adapter pattern
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — boot wiring and `OpenFangKernel` struct
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/scheduler.rs` — receives `ScheduleRuntimeStore`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` — API endpoints that surface runtime state
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — table and column spec (section 2)
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/INITIAL-RUNTIME-MIGRATIONS.md` — migration file spec (section 3)
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/STORAGE-MODEL.md` — ownership rules (sections 3, 5, 6)

### Dependent Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/session.rs` — long-term migration target; must not be broken by this task
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` — agent and schedule API endpoints
- Future: workflow repositories that will reference `agent_id` and `session_id` from `runtime.db` by stable ID, not via SQL join

## Deliverables

- Four `runtime.db` migration steps (SQL) covering `agent_runtime`, `agent_session`, `agent_message`, `schedule_runtime`, `schedule_execution`
- Store adapter types for each new table
- `OpenFangKernel` updated with runtime store handles
- Scheduler subsystem wired to `ScheduleRuntimeStore`
- Unit and integration tests for schema creation and store round-trips

## Tests

### Unit Tests (Required)

- [ ] `runtime_db_migration_should_create_agent_runtime_table()` — run the `runtime.db` migration slice against an in-memory connection; query `sqlite_master` and confirm `agent_runtime` exists.
- [ ] `runtime_db_migration_should_create_agent_session_and_message_tables()` — same for `agent_session` and `agent_message`.
- [ ] `runtime_db_migration_should_create_schedule_tables()` — same for `schedule_runtime` and `schedule_execution`.
- [ ] `runtime_db_migration_should_create_required_indexes()` — query `sqlite_master WHERE type='index'` and confirm each required index name is present.
- [ ] `agent_runtime_store_should_upsert_and_retrieve()` — insert an `agent_runtime` row via `AgentRuntimeStore::upsert_agent_runtime()`, retrieve it by `agent_id`, assert the fields match.
- [ ] `agent_session_store_should_create_and_list_sessions()` — create two sessions for different agents; list by `agent_id` and confirm each returns only its own sessions.
- [ ] `agent_message_store_should_record_direction_and_status()` — insert messages with different `direction` values (`inbound`, `outbound`); confirm they are stored and retrievable by `session_id`.
- [ ] `schedule_runtime_store_should_update_last_run_and_next_run()` — upsert a `schedule_runtime` row, update `last_run` and `next_run`, retrieve and confirm the update persisted.
- [ ] `schedule_execution_store_should_record_execution_receipt()` — insert a `schedule_execution` row; confirm it is retrievable by `schedule_id` ordered by `fired_at`.

### Integration Tests (Required)

- [ ] `boot_should_create_runtime_db_with_all_initial_tables()` — after `boot_with_config()`, open `runtime.db` directly and confirm all five new tables exist.
- [ ] `agent_runtime_state_should_survive_restart()` — write an `agent_runtime` row via the store; re-boot against the same `data_dir`; confirm the row is still present.
- [ ] `schedule_runtime_state_should_survive_restart()` — same for `schedule_runtime`.
- [ ] `existing_boot_path_should_not_regress()` — the existing `MemorySubstrate` sessions and KV store must continue to work after the new `runtime.db` tables are added; confirmed by running the existing substrate tests.

### Regression and Anti-Pattern Guards

- [ ] No `compozy.db` tables (`workflow_run`, `workflow_checkpoint`, `workflow_signal`, `task`, `subtask`, `looper_run`) may appear in the `runtime.db` migration slice. Confirm by reviewing the SQL strings.
- [ ] The `agent_runtime` table must not contain definition fields (agent name, system prompt, model, skills list). These are file-backed per ADR-037. Confirm by reviewing the migration SQL columns.
- [ ] The legacy `openfang-memory` `run_migrations()` call in `MemorySubstrate::open()` must not be removed or modified by this task. The old schema and the new schema coexist during the migration period.
- [ ] No store adapter method may issue SQL that references a table from a different database (no `ATTACH DATABASE`, no cross-database references). Confirm by reviewing all SQL strings in the adapter implementations.
- [ ] The scheduler's in-memory schedule state must remain functional if `ScheduleRuntimeStore` fails to write — the store should be best-effort for the first cut and must not crash the scheduler on a write error.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All five initial `runtime.db` tables exist after a fresh boot with the correct columns and indexes.
- Each table has a typed store adapter with at minimum read and write operations.
- Agent runtime state and schedule runtime state survive a daemon restart.
- The existing `MemorySubstrate` continues to function for the legacy schema.
- The `OpenFangKernel` struct exposes the new store handles without exposing raw connections.
- Task 9 can proceed to add `compozy.db` tables without touching the `runtime.db` migration slice.

---

## Notes

- Keep this task scoped to platform runtime ownership only.
