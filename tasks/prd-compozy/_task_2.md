## markdown

## status: completed

<task_context>
<domain>engine/infra/bootstrap</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task1</dependencies>
</task_context>

# Task 2.0: Dual-Database Bootstrap In Kernel Startup

## Overview

Make kernel startup open, own, and expose both `runtime.db` and `compozy.db`
before dependent subsystems are constructed.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Per IMPLEMENTATION-PLAN.md Phase 0, the boot sequence must open both databases in a fixed, documented order before any subsystem is constructed: (1) load config, (2) resolve both paths from `PersistenceConfig`, (3) open `runtime.db`, (4) open `compozy.db`, (5) initialize repository/store layer, (6) continue kernel boot.
- Per ADR-003, `runtime.db` and `compozy.db` must be opened as separate, independent SQLite connections. A single shared connection for both databases is not acceptable.
- Per ADR-003's consequence that "cross-system relationships are resolved in application code, not SQL joins," the kernel must hold the two database handles as distinct, named fields — not as a generic collection or a single merged handle.
- The `OpenFangKernel` struct in `crates/openfang-kernel/src/kernel.rs` must gain explicit named handles (or owned manager types) for both databases. The existing `memory: Arc<MemorySubstrate>` field continues to own `runtime.db` until Task 6 completes the proper store split. A new handle for `compozy.db` must be added alongside it.
- Boot failure at either database open must produce a `KernelError::BootFailed` with a message that names the specific database that failed (`runtime.db` vs `compozy.db`), not a generic IO or memory error.
- Per INITIAL-RUNTIME-MIGRATIONS.md section 4, the boot sequence must apply `runtime.db` migrations before `compozy.db` migrations. Neither migration stream may run before both databases are successfully opened.
- The kernel must not expose raw `rusqlite::Connection` objects to subsystems outside `openfang-kernel`. Subsystems receive typed store or repository handles, not direct connection access.
- The dual-database boot must work with the existing test harness pattern in `crates/openfang-api/tests/api_integration_test.rs` — `OpenFangKernel::boot_with_config(config)` must continue to be the single entry point, with no new required setup calls.
</requirements>

## Subtasks

- [x] 2.1 Introduce a `DatabaseManager` or equivalent internal struct in `crates/openfang-kernel/src/` (e.g. `db.rs`) that holds both the `runtime.db` connection/pool and the `compozy.db` connection/pool, each wrapped in `Arc<Mutex<rusqlite::Connection>>` or a typed async pool. This struct is responsible for opening, configuring WAL mode, and setting `busy_timeout` for both databases, following the existing pattern in `crates/openfang-memory/src/substrate.rs` lines ~41-43.
- [x] 2.2 Refactor the `boot_with_config()` function in `crates/openfang-kernel/src/kernel.rs` to open both databases at lines ~554-567 before any other subsystem is constructed. The `runtime.db` path comes from `config.persistence.resolve_runtime_db(&config.data_dir)` (Task 1 output). The `compozy.db` path comes from `config.persistence.resolve_compozy_db(&config.data_dir)`.
- [x] 2.3 Add an explicit `compozy_db` handle to the `OpenFangKernel` struct. The initial handle is a bare `Arc<Mutex<rusqlite::Connection>>` or equivalent; it will be replaced by a typed store layer in Task 9. The existing `memory: Arc<MemorySubstrate>` continues to own `runtime.db` through Task 6.
- [x] 2.4 Ensure bootstrap failure at either database open produces a `KernelError::BootFailed` with an error message that identifies the database by name. The existing error handling pattern is at `crates/openfang-kernel/src/kernel.rs` line ~566: `.map_err(|e| KernelError::BootFailed(format!("Memory init failed: {e}")))`.
- [x] 2.5 Update the `data_dir` creation step (currently at line ~555 in `kernel.rs`) to also ensure parent directories for both database paths exist before attempting to open them, using `std::fs::create_dir_all` on the parent of each resolved path.
- [x] 2.6 Add a `db_health()` or equivalent method to `OpenFangKernel` that can report whether both databases are open and responsive (a simple `PRAGMA integrity_check` or `SELECT 1` is sufficient). This powers health checks and startup readiness reporting.
- [x] 2.7 Update kernel-level integration tests and the existing test server construction pattern in `crates/openfang-api/tests/` to confirm that `boot_with_config()` opens both databases and that the test teardown cleans up both temporary database files.
      </requirements>

## Implementation Details

This task changes the startup order and kernel struct shape. It must not
change the durable workflow model, migration logic, or store implementations —
those belong to Tasks 3, 6, and 9.

### Current State

The current boot sequence opens a single database at
`crates/openfang-kernel/src/kernel.rs` lines ~558-567:

```
let db_path = config
    .memory
    .sqlite_path
    .clone()
    .unwrap_or_else(|| config.data_dir.join("openfang.db"));
let memory = Arc::new(
    MemorySubstrate::open(&db_path, config.memory.decay_rate)
        .map_err(|e| KernelError::BootFailed(format!("Memory init failed: {e}")))?,
);
```

`MemorySubstrate::open()` in `crates/openfang-memory/src/substrate.rs` line
~40 opens a `rusqlite::Connection`, runs WAL pragma, and runs migrations in
one call. The pattern to replicate for `compozy.db` is the same WAL/timeout
setup without `run_migrations` (Task 3 introduces the migration runner).

The `OpenFangKernel` struct currently holds only `memory: Arc<MemorySubstrate>`
at line ~72. It has no field for a second database.

The test harness in `crates/openfang-api/tests/api_integration_test.rs`
constructs the kernel with:

```
let kernel = OpenFangKernel::boot_with_config(config).expect("Kernel should boot");
```

This pattern must continue to work unchanged after this task.

### What Needs To Change

- `boot_with_config()` must open the `compozy.db` connection immediately after
  `runtime.db` (i.e., after `MemorySubstrate::open()`), before the credential
  resolver, LLM driver, and all other subsystem construction that follows at
  line ~569 onward.
- A new internal module or struct (e.g. `crates/openfang-kernel/src/db.rs`)
  should encapsulate the open-and-configure logic for a raw SQLite connection
  so it is not repeated twice in `boot_with_config()`. It should apply
  `PRAGMA journal_mode=WAL` and `PRAGMA busy_timeout=5000` to match the
  existing `MemorySubstrate` setup.
- `OpenFangKernel` gains a `compozy_db: Arc<Mutex<rusqlite::Connection>>` field
  (or equivalent typed handle). The field is `pub(crate)` at minimum; Task 9
  will replace it with a typed store layer, so keep the field easily
  substitutable.
- `KernelError::BootFailed` messages for database failures must name the
  specific database file, e.g.:
  `"Failed to open runtime.db at {path}: {e}"` and
  `"Failed to open compozy.db at {path}: {e}"`.

### Integration Points

- `crates/openfang-kernel/src/kernel.rs` — `boot_with_config()` is the sole
  entry point; all changes are local to this function and the `OpenFangKernel`
  struct definition.
- `crates/openfang-kernel/src/error.rs` — `KernelError::BootFailed(String)`
  already exists; no new error variants needed for this task.
- `crates/openfang-memory/src/substrate.rs` — `MemorySubstrate::open()` at
  line ~40 is the reference pattern for WAL/timeout setup. Do not change this
  function; replicate the pattern for the `compozy.db` raw connection.
- `crates/openfang-api/tests/api_integration_test.rs` — the `start_test_server()`
  helper at line ~49 uses `boot_with_config()`. The new `compozy_db` field
  must have a `Default` or be fully initialized inside `boot_with_config()`.
- `crates/openfang-api/src/routes.rs` — the health endpoint or status endpoint
  may need to call `kernel.db_health()` to surface dual-database readiness.

### Boot Sequence After This Task

Per INITIAL-RUNTIME-MIGRATIONS.md section 4, the canonical boot order becomes:

1. Load and validate config
2. Ensure data directory exists (`std::fs::create_dir_all`)
3. Resolve `runtime.db` path from `config.persistence`
4. Open `runtime.db` (via `MemorySubstrate::open()` for now)
5. Resolve `compozy.db` path from `config.persistence`
6. Open `compozy.db` raw connection (WAL + busy_timeout)
7. Apply `runtime.db` migrations (Task 3's runner, wired in Task 3)
8. Apply `compozy.db` migrations (Task 3's runner, wired in Task 3)
9. Initialize repository/store layers
10. Continue remainder of subsystem construction

Steps 7 and 8 are stubs in this task — the migration runner does not exist
yet. This task opens the connections and proves boot succeeds; Task 3 adds
the migration runner and wires it here.

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — primary change target; `boot_with_config()` and `OpenFangKernel` struct
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/error.rs` — `KernelError::BootFailed`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/substrate.rs` — reference pattern for WAL/timeout setup at line ~40-44
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` — health/status endpoints
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/api_integration_test.rs` — test harness that must continue to work
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/daemon_lifecycle_test.rs` — lifecycle test harness
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/INITIAL-RUNTIME-MIGRATIONS.md` — boot sequence spec (section 4)

### Dependent Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — Task 3 wires the migration runner into the boot sequence here
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/` — Task 6 replaces the raw `runtime.db` connection with typed store adapters
- Workflow store modules (to be created in Task 9) will receive the `compozy_db` handle

## Deliverables

- Kernel boot that opens both databases as distinct, named handles before any subsystem is constructed
- `OpenFangKernel` struct with an explicit `compozy_db` field alongside the existing `memory` field
- Clear, database-specific error messages for boot failures at either database
- A `db_health()` method or equivalent readiness check on `OpenFangKernel`
- All existing integration tests passing with the new boot sequence

## Tests

### Unit Tests (Required)

- [x] `boot_should_fail_clearly_when_runtime_db_path_is_unwritable()` — configure `runtime_db` to a path under a non-existent and non-creatable directory; confirm `boot_with_config()` returns `KernelError::BootFailed` containing `"runtime.db"` in the message.
- [x] `boot_should_fail_clearly_when_compozy_db_path_is_unwritable()` — same for `compozy_db`, confirm the error message contains `"compozy.db"`.
- [x] `boot_should_open_runtime_db_before_compozy_db()` — use a logging or instrumentation shim to verify open order; or verify that a failure on `compozy.db` does not prevent an already-opened `runtime.db` from being reported distinctly.
- [x] `boot_should_initialize_compozy_db_handle_as_non_null()` — after a successful boot, confirm `kernel.compozy_db` is a live connection by executing `SELECT 1` against it.
- [x] `db_health_should_return_healthy_after_successful_boot()` — confirm `kernel.db_health()` returns a healthy status for both databases.
- [x] `boot_should_not_hide_partial_failure_as_degraded_success()` — if `compozy.db` fails to open, `boot_with_config()` must return `Err`, not `Ok` with a degraded kernel.

### Integration Tests (Required)

- [x] `start_test_server()` in `crates/openfang-api/tests/api_integration_test.rs` passes without modification — the new `compozy_db` field is transparent to test harnesses that use `..KernelConfig::default()`.
- [x] Daemon lifecycle tests in `crates/openfang-api/tests/daemon_lifecycle_test.rs` pass with both database files present in the test temp directory after boot.
- [x] A fresh boot with no pre-existing database files creates both `runtime.db` and `compozy.db` in `data_dir` — confirmed by asserting both files exist after `boot_with_config()` returns.
- [x] A second boot against an already-initialized `data_dir` succeeds without error — idempotent open behavior.
- [x] The health endpoint at `/api/health` returns a successful response after dual-database boot — confirms the API layer is not broken by the new kernel struct shape.

### Regression and Anti-Pattern Guards

- [x] No subsystem construction step (credential resolver, LLM driver, scheduler, workflow engine, etc.) may run before both database open calls complete in `boot_with_config()`. Verify by reading the function top-to-bottom after the change.
- [x] No global or thread-local static is introduced to avoid passing the `compozy_db` handle through the kernel struct. The handle must be a named field on `OpenFangKernel`.
- [x] Boot must not log a success message before both databases are confirmed open. The existing `info!("Booting OpenFang kernel...")` at line ~543 must not be the last boot log before a database failure.
- [x] No subsystem outside `openfang-kernel` may receive a raw `rusqlite::Connection` as a public parameter from this task.
- [x] The `memory: Arc<MemorySubstrate>` field must not be removed or renamed by this task — it remains the owner of `runtime.db` state until Task 6.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- Both databases are opened as distinct, named resources during `boot_with_config()` before any subsystem is constructed.
- The boot sequence order matches the spec in INITIAL-RUNTIME-MIGRATIONS.md section 4.
- A failure at either database open surfaces a `KernelError::BootFailed` that names the failing database.
- All existing integration tests pass without modification.
- Task 3 can wire the migration runner into `boot_with_config()` without needing to restructure the boot sequence again.
- Task 9 can replace the raw `compozy_db` handle with a typed store layer by changing only the field type, not the boot order.

---

## Notes

- This task is the earliest structural prerequisite for durable workflow work.
