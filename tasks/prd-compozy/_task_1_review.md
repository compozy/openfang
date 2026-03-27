# Task 1 Review: Split Persistence Config For Dual Databases

## Status: PASS

## Checklist
- [x] `PersistenceConfig` struct defined in `crates/openfang-types/src/config.rs` with `runtime_db: Option<PathBuf>` and `compozy_db: Option<PathBuf>`
- [x] `resolve_runtime_db(&self, data_dir: &Path) -> PathBuf` and `resolve_compozy_db` helpers implemented on `PersistenceConfig`
- [x] `persistence: PersistenceConfig` field added to `KernelConfig`
- [x] `Default` impl for `KernelConfig` includes `persistence: PersistenceConfig::default()`
- [x] `sqlite_path` removed from `MemoryConfig` — grep for `sqlite_path` returns zero production hits
- [x] All call sites updated: kernel boot reads `config.persistence.resolve_runtime_db(...)` instead of `memory.sqlite_path`
- [x] `openfang.db` string does not appear in any production Rust source
- [x] `PersistenceConfig::validate_paths()` implemented and called via `KernelConfig::validate_persistence_paths()` during boot
- [x] `#[serde(default)]` on `PersistenceConfig`; `Default` impl returns `None` for both fields (resolves at runtime via helpers)
- [x] TOML doc comments present on the `[persistence]` table and its two fields
- [x] Unit tests: all 8 required tests present and named correctly
- [x] Integration: existing test fixtures use `..KernelConfig::default()` struct-update syntax — unaffected by new field
- [x] CLI doctor now tests `persistence` config fields (test `test_doctor_persistence_config_fields` in `openfang-cli/src/main.rs`)

## Findings

**Correctly implemented:**
- `PersistenceConfig` is the single source of truth for both DB paths; `MemoryConfig` no longer carries `sqlite_path`.
- Default resolution is clean: `None` field → `data_dir.join("runtime.db"/"compozy.db")` via the resolve helpers.
- Validation properly catches a directory-as-path and a path whose parent cannot be created.
- The `Default` impl uses private `default_runtime_db_path()` / `default_compozy_db_path()` helpers that both return `None`, ensuring serde round-trip for missing TOML keys.
- All 8 required unit tests are present with correct names. Regression guard tests (`kernel_config_default_should_not_contain_openfang_db_anywhere`, `memory_config_should_not_have_sqlite_path_field`) are present.

**Minor notes:**
- The CLI "doctor" no longer hard-codes `openfang.db` as a file check path; it validates `persistence` config deserialization instead. The subtask's original text described checking `openfang_dir.join("data").join("openfang.db")` — that specific line is gone and replaced with config-aware tests. Fully acceptable.
- `PersistenceConfig::Default` impl returns `None` for both fields (not hardcoded paths), satisfying the `serde(default)` requirement. The comment above it notes the default resolution is done via `resolve_*` helpers at boot time.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/config.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-cli/src/main.rs`
