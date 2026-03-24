## markdown

## status: completed

<task_context>
<domain>engine/infra/persistence</domain>
<type>implementation</type>
<scope>configuration</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 1.0: Split Persistence Config For Dual Databases

## Overview

Split the current single SQLite persistence configuration into explicit
`runtime.db` and `compozy.db` configuration paths and ownership boundaries.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Replace the current `MemoryConfig.sqlite_path: Option<PathBuf>` field (in `crates/openfang-types/src/config.rs`) with a `PersistenceConfig` struct that holds explicit, separately-typed paths for `runtime_db` and `compozy_db`. The old `sqlite_path` must not survive as a fallback field.
- Per ADR-003, the two databases live under the same `~/.compozy/data/` directory root. Default paths must resolve to `data_dir.join("runtime.db")` and `data_dir.join("compozy.db")` when the user omits them from config.
- Per ADR-037, definitions remain file-backed; the new config must not introduce any database path for definition storage. The config shape must not suggest or enable a third "definitions" database path.
- Per the STORAGE-MODEL.md cross-boundary rule, the config struct must make the ownership split legible at the type level — a single merged `PathBuf` field is not acceptable. `PersistenceConfig` is the correct granularity.
- Per IMPLEMENTATION-PLAN.md Phase 0 requirements, the new config shape must be stable enough for Task 2 (dual-database bootstrap) and Task 3 (migration runner) to import directly without a breaking change.
- All existing call sites that read `config.memory.sqlite_path` in the kernel boot sequence — currently at `crates/openfang-kernel/src/kernel.rs` line ~559-563 — must be updated to read the new fields. No call site may silently fall back to `openfang.db` after this task.
- The new `PersistenceConfig` must be `#[serde(default)]`-compatible and must provide a correct `Default` impl that resolves both paths relative to `data_dir` without user intervention.
- Config validation must emit a clear, actionable error (not a panic) when either database path is explicitly set to an unusable value (e.g. a directory, a path with missing parent segments that cannot be created).
</requirements>

## Subtasks

- [x] 1.1 Audit all call sites that read `config.memory.sqlite_path` or construct the `openfang.db` path. Confirm the full list: `crates/openfang-kernel/src/kernel.rs` (boot sequence), `crates/openfang-cli/src/main.rs` (doctor check at line ~2287), `crates/openfang-api/src/routes.rs` (config status endpoint at line ~5155), and any integration test fixtures in `crates/openfang-api/tests/`.
- [x] 1.2 Define the `PersistenceConfig` struct in `crates/openfang-types/src/config.rs` with `runtime_db: Option<PathBuf>` and `compozy_db: Option<PathBuf>` fields, both `#[serde(default)]`. Add a `resolve_runtime_db(&self, data_dir: &Path) -> PathBuf` and `resolve_compozy_db(&self, data_dir: &Path) -> PathBuf` helper pair on `KernelConfig` or on `PersistenceConfig` itself.
- [x] 1.3 Add the `persistence: PersistenceConfig` field to `KernelConfig` in `crates/openfang-types/src/config.rs`. Update the `Default` impl to include `persistence: PersistenceConfig::default()`. Keep `memory: MemoryConfig` intact for the memory-substrate decay/embedding fields it still owns; only the `sqlite_path` subfield migrates out.
- [x] 1.4 Remove `sqlite_path` from `MemoryConfig` and update every consumer of that field. The kernel boot sequence in `crates/openfang-kernel/src/kernel.rs` must now read `config.persistence.resolve_runtime_db(&config.data_dir)` for the `MemorySubstrate` path (runtime.db owns the current substrate until Task 6 completes the proper split).
- [x] 1.5 Add config validation logic — either inside `KernelConfig::validate()` (which already exists and is called during boot) or as a dedicated `PersistenceConfig::validate()` method — that checks both resolved paths for viability and returns descriptive `String` warnings or errors per the existing validation pattern.
- [x] 1.6 Update the TOML config documentation comments in `config.rs` to describe the new `[persistence]` table and its two fields, including the default resolution rule.
- [x] 1.7 Write unit tests in `crates/openfang-types/src/config.rs` (or a sibling test module) covering: explicit path parsing, default resolution, and validation error messages for bad paths.
      </requirements>

## Implementation Details

This task only defines the config shape and ownership contract. It must not
yet implement the full bootstrap or migration flow — that is Task 2's scope.

### Current State

The current persistence config lives entirely in `MemoryConfig` in
`crates/openfang-types/src/config.rs`:

```
pub struct MemoryConfig {
    pub sqlite_path: Option<PathBuf>,   // <-- single-DB assumption
    pub embedding_model: String,
    pub consolidation_threshold: u64,
    pub decay_rate: f32,
    ...
}
```

The kernel boot sequence in `crates/openfang-kernel/src/kernel.rs` resolves
it at line ~559-563:

```
let db_path = config
    .memory
    .sqlite_path
    .clone()
    .unwrap_or_else(|| config.data_dir.join("openfang.db"));
```

The CLI doctor command checks the single DB at
`crates/openfang-cli/src/main.rs` line ~2287:
`openfang_dir.join("data").join("openfang.db")`.

### What Needs To Change

- `MemoryConfig` loses `sqlite_path`. It retains `embedding_model`,
  `consolidation_threshold`, `decay_rate`, `embedding_provider`,
  `embedding_api_key_env`, and `consolidation_interval_hours`.
- A new `PersistenceConfig` struct is added to `config.rs` with `runtime_db`
  and `compozy_db` as `Option<PathBuf>`.
- `KernelConfig` gains a `persistence: PersistenceConfig` field in addition
  to the existing `memory: MemoryConfig` field.
- The `Default` impl for `KernelConfig` must not hard-code `openfang.db`
  anywhere. The default resolution must go through `PersistenceConfig`.
- `KernelConfig::validate()` (called during boot in `kernel.rs` line ~549)
  must include a persistence validation step.

### Integration Points

- `crates/openfang-kernel/src/kernel.rs` — `boot_with_config()` reads the
  resolved `runtime_db` path to open `MemorySubstrate`. Until Task 6 creates
  the proper `runtime.db` store layer, the existing `MemorySubstrate` is
  opened against `runtime_db` so boot continues to work.
- `crates/openfang-api/src/routes.rs` — the config status endpoint serializes
  `data_dir`. It may need to surface `persistence.runtime_db` and
  `persistence.compozy_db` in the response for observability.
- `crates/openfang-api/tests/api_integration_test.rs`,
  `crates/openfang-api/tests/daemon_lifecycle_test.rs`,
  `crates/openfang-api/tests/load_test.rs` — all construct `KernelConfig`
  with struct literal syntax (`KernelConfig { home_dir: ..., data_dir: ..., ..KernelConfig::default() }`).
  Adding `persistence` to the struct will not break them if the field has a
  `Default` impl, but if any test uses exhaustive struct construction it must
  be updated.

### Patterns To Follow

- Follow the existing `MemoryConfig` pattern: `#[serde(default)]` on the
  struct, explicit `Default` impl rather than derived, doc comments on every
  public field.
- Use `Option<PathBuf>` for both database paths so TOML users can omit them
  and get sensible defaults. Do not use `PathBuf` with a hard-coded default
  as the field type — that breaks serde round-trip for missing keys.
- Resolution helpers should be pure functions (no `&mut self`, no side
  effects) so they can be called freely during validation and boot.

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/config.rs` — primary change target; `MemoryConfig` at line ~1471, `KernelConfig` at line ~962, `Default` impl at line ~1262
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — `boot_with_config()` at line ~559-567 reads `memory.sqlite_path`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/config.rs` — `load_config()` and `KernelConfig` deserialization; config include resolution
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-cli/src/main.rs` — doctor check at line ~2287 references `openfang.db` by name
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/STORAGE-MODEL.md` — ownership boundaries
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/IMPLEMENTATION-PLAN.md` — Phase 0 requirements

### Dependent Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` — Task 2 reads the new config fields to open both databases
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` — config status endpoint may expose new fields
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/api_integration_test.rs` — test fixtures construct `KernelConfig`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/daemon_lifecycle_test.rs` — same
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/load_test.rs` — same

## Deliverables

- `PersistenceConfig` struct in `crates/openfang-types/src/config.rs` with dual-database path fields and resolution helpers
- `KernelConfig` updated with `persistence: PersistenceConfig` field and validated `Default` impl
- `MemoryConfig.sqlite_path` removed; all call sites updated
- Config validation that catches bad persistence paths before boot fails
- Tests covering the new config shape

## Tests

### Unit Tests (Required)

- [x] `persistence_config_should_resolve_runtime_db_to_data_dir_default()` — when `runtime_db` is `None`, the resolved path equals `data_dir.join("runtime.db")`.
- [x] `persistence_config_should_resolve_compozy_db_to_data_dir_default()` — when `compozy_db` is `None`, the resolved path equals `data_dir.join("compozy.db")`.
- [x] `persistence_config_should_accept_explicit_runtime_db_path()` — when `runtime_db` is set, the resolution returns that exact path.
- [x] `persistence_config_should_accept_explicit_compozy_db_path()` — when `compozy_db` is set, the resolution returns that exact path.
- [x] `kernel_config_default_should_not_contain_openfang_db_anywhere()` — assert that serializing `KernelConfig::default()` to TOML does not contain the string `"openfang.db"`.
- [x] `memory_config_should_not_have_sqlite_path_field()` — compile-time proof: remove the field and confirm the struct still compiles without it.
- [x] `persistence_config_toml_round_trips_correctly()` — serialize a `PersistenceConfig` with explicit paths to TOML and deserialize it back; assert equality.
- [x] `persistence_config_validation_should_reject_path_with_missing_parent()` — confirm validation emits a non-empty warning or error for a path whose parent directory cannot be created (use a path under a non-existent root).

### Integration Tests (Required)

- [x] Existing boot config fixtures in `crates/openfang-api/tests/api_integration_test.rs` continue to boot with `..KernelConfig::default()` without modification (confirms backward compatibility of the Default impl).
- [x] `start_test_server()` in `crates/openfang-api/tests/api_integration_test.rs` passes without changes — the new `persistence` field must be invisible to tests that use struct-update syntax.
- [x] A `KernelConfig` loaded from a TOML file that has no `[persistence]` section still boots — confirms graceful default resolution.
- [x] A `KernelConfig` loaded from a TOML file with an explicit `[persistence]` section that sets both paths uses those paths exactly.
- [x] The CLI doctor check in `crates/openfang-cli/src/main.rs` no longer hard-codes `openfang.db`; it reads from the resolved config paths.

### Regression and Anti-Pattern Guards

- [x] No code path may produce the string `"openfang.db"` as a database filename after this task. Search for the literal and confirm zero hits in non-test production code.
- [x] `memory.sqlite_path` must not survive as a live field in `MemoryConfig`. A grep for `sqlite_path` in the production source tree must return zero hits after this task.
- [x] Config changes must not silently reintroduce a single-DB assumption by having `PersistenceConfig` contain a single merged `path` field that is shared between databases.
- [x] No test-only config branch (e.g. `#[cfg(test)] let db_path = ":memory:"` injected into the production boot path) may be introduced by this task.
- [x] The new `PersistenceConfig` struct must not carry any fields that hint at a third database, a definitions database, or a combined merged path.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- `PersistenceConfig` is the single, stable source of truth for both database paths; no other config struct carries a competing database path field.
- The default resolution for both paths is deterministic, uses `data_dir` as the root, and requires no user config to boot.
- All kernel boot paths read from `PersistenceConfig`; no path reads `memory.sqlite_path` or constructs `openfang.db` by name.
- All existing integration tests pass without modification.
- Config validation emits actionable errors before the boot sequence reaches the database open call.
- Task 2 can import `PersistenceConfig` and call the resolution helpers without any further changes to this task's output.
- The TOML config surface is backward-compatible for users who had no `[persistence]` section.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
