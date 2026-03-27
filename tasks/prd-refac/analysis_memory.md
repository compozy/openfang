# openfang-memory Refactoring Analysis

**Date:** 2026-03-27
**Crate:** `crates/openfang-memory/src/`
**Total lines:** 18,449 across 18 source files

---

## 1. File Inventory

| File | Lines | Tests (approx) | Production | Priority |
|------|------:|:--------------:|:----------:|:--------:|
| `workflow_store.rs` | 2,968 | ~970 | ~2,000 | Critical |
| `task.rs` | 2,308 | ~800 | ~1,500 | Critical |
| `dispatch.rs` | 1,626 | ~560 | ~1,060 | High |
| `artifact.rs` | 1,544 | ~700 | ~840 | High |
| `hitl.rs` | 1,436 | ~730 | ~700 | High |
| `runtime_store.rs` | 1,413 | ~250 | ~1,160 | High |
| `doc.rs` | 1,312 | ~600 | ~710 | High |
| `looper.rs` | 1,303 | ~500 | ~800 | Medium |
| `substrate.rs` | 830 | 0 | 830 | Medium |
| `session.rs` | 813 | 0 | 813 | Medium |
| `semantic.rs` | 719 | 0 | 719 | Low |
| `usage.rs` | 541 | 0 | 541 | Low |
| `structured.rs` | 493 | 0 | 493 | Low |
| `migration.rs` | 363 | ~30 | ~330 | Low |
| `knowledge.rs` | 346 | 0 | 346 | Low |
| `pack.rs` | 267 | ~60 | ~200 | Low |
| `consolidation.rs` | 101 | ~40 | ~60 | Low |
| `lib.rs` | 66 | 0 | 66 | Low |

---

## 2. Cross-Cutting Issues

### 2.1 Duplicated Utility Functions (Critical)

The single most impactful refactoring opportunity. Identical or near-identical helper functions are copy-pasted across 6-9 files.

| Function | Files where duplicated |
|----------|----------------------|
| `lock_conn()` | `workflow_store`, `task`, `dispatch`, `hitl`, `looper`, `artifact`, `doc`, `pack`, `runtime_store` (9 files) |
| `bool_to_sql()` | `workflow_store`, `task`, `runtime_store` (3 files) |
| `sql_to_bool()` | `workflow_store`, `task`, `runtime_store` (3 files) |
| `serialize_json_field()` | `task`, `looper`, `artifact`, `doc` (4 files) |
| `serialize_optional_json()` | `dispatch`, `hitl`, `looper` (3 files) |
| `ensure_object_json()` | `task`, `artifact`, `doc` (3 files) |
| `is_unique_constraint_for()` | `task`, `artifact`, `doc` (3 files) |
| `is_unique_constraint()` | `workflow_store` (1 file, but functionally overlaps with the `_for` variant) |
| `provenance_parts()` | `artifact`, `doc` (2 files) |
| `normalize_limit()` | `artifact`, `doc` (2 files) |
| `decode_cursor()` / `encode_cursor()` | `task`, `artifact`, `doc` (3 files, each with its own cursor type) |
| `collect_rows()` (generic row collector) | `dispatch`, `hitl` (2 files) |

**Example -- `bool_to_sql` is identical across three files:**

```rust
// workflow_store.rs:1873, task.rs:1631, runtime_store.rs:927
fn bool_to_sql(value: bool) -> i64 {
    if value { 1 } else { 0 }
}
```

**Example -- `lock_conn` differs only in the error type returned:**

```rust
// workflow_store.rs
fn lock_conn(conn: &Arc<Mutex<Connection>>) -> Result<MutexGuard<'_, Connection>, WorkflowStoreError> {
    conn.lock().map_err(|error| WorkflowStoreError::ConnectionLock(error.to_string()))
}

// dispatch.rs
fn lock_conn(conn: &Arc<Mutex<Connection>>) -> Result<MutexGuard<'_, Connection>, DispatchStoreError> {
    conn.lock().map_err(|error| DispatchStoreError::ConnectionLock(error.to_string()))
}
```

**Recommendation:** Extract a `common.rs` (or `helpers.rs`) module containing:
- `bool_to_sql()` / `sql_to_bool()`
- `serialize_json_field()` / `serialize_optional_json()` / `serialize_optional_json_field()`
- `ensure_object_json()` (generic over error type, or returning a common error)
- `is_unique_constraint()` / `is_unique_constraint_for()`
- `normalize_limit()`
- `provenance_parts()`
- A generic cursor encode/decode pair

For `lock_conn`, introduce a trait or use a generic function parameterized on the error type, since all 9 implementations differ only in the error variant constructor:

```rust
pub(crate) fn lock_conn<E>(
    conn: &Arc<Mutex<Connection>>,
    map_err: impl FnOnce(String) -> E,
) -> Result<MutexGuard<'_, Connection>, E> {
    conn.lock().map_err(|error| map_err(error.to_string()))
}
```

---

### 2.2 Isomorphic Error Enums (High)

Every repository file defines its own error enum with the same structural shape:

```
ConnectionLock(String)
Sqlite(#[from] rusqlite::Error)
Json(#[from] serde_json::Error)
...domain-specific variants...
```

There are **9 separate error types**: `WorkflowStoreError`, `TaskStoreError`, `DispatchStoreError`, `HitlStoreError`, `LooperStoreError`, `ArtifactStoreError`, `DocStoreError`, `PackStoreError`, plus `runtime_store` uses `OpenFangError` directly.

All of them implement `From<XxxStoreError> for OpenFangError` identically: `OpenFangError::Memory(error.to_string())`.

**Recommendation:** Create a shared base error type (e.g., `MemoryStoreError`) containing the common variants (`ConnectionLock`, `Sqlite`, `Json`). Domain-specific errors can wrap this base type plus their own variants. This eliminates 27+ lines of boilerplate per error enum (9 enums x 3 common variants).

---

### 2.3 Repeated SELECT Column Lists (High)

The full column list for `workflow_run` (17 columns) is written out verbatim **7 times** across `workflow_store.rs`:
- `list_non_terminal` (lines 636-654)
- `list_all_workflow_runs` (lines 1745-1764)
- `list_workflow_runs_by_status` (lines 1779-1798)
- `load_workflow_run` (lines 1717-1735)
- `find_by_source_run_id` in task.rs (similar 22-column list repeated 4 times)
- `insert_workflow_run` (17 params)
- `update_workflow_run_record` (17-18 params, duplicated for the two branches)

Similarly, `task.rs` repeats its 22-column SELECT list **4 times** (`load_task`, `load_task_by_slug`, `list_tasks`, `find_by_source_run_id`).

`dispatch.rs` repeats its 18-column SELECT list **5 times** (`load_dispatch`, `list_dispatches_by_run`, `list_child_dispatches`, `list_dispatches`).

**Recommendation:** Extract column lists into `const` strings:

```rust
const WORKFLOW_RUN_COLUMNS: &str = "run_id, workflow_id, workflow_version, status, ...";
const TASK_COLUMNS: &str = "task_id, slug, source_run_id, title, ...";
```

Then compose queries: `format!("SELECT {TASK_COLUMNS} FROM task WHERE ...")`. This reduces ~200 lines of duplicate column listings and makes schema changes require edits in only one place.

---

### 2.4 Duplicated `update_workflow_run_record` SQL (Medium)

`update_workflow_run_record` in `workflow_store.rs` (lines 1341-1428) contains **two nearly identical UPDATE statements** -- one with an `AND status = ?18` guard and one without. The only difference is the presence/absence of the optimistic-concurrency WHERE clause. This is 87 lines where ~40 could be eliminated by building the SQL dynamically or using a single prepared statement with `(?18 IS NULL OR status = ?18)`.

---

## 3. Per-File Analysis

### 3.1 `workflow_store.rs` (2,968 lines) -- Critical

**Problems:**

1. **God-file SRP violation.** This file contains:
   - `WorkflowStoreSet` (aggregation struct)
   - `WorkflowRunRepository` (CRUD + complex transactional methods)
   - `WorkflowCheckpointRepository`
   - `WorkflowSignalRepository`
   - All record types and enums (`WorkflowRunStatus`, `CheckpointKind`, etc.)
   - All private SQL helper functions
   - ~970 lines of tests

2. **`WorkflowRunRepository` is the largest single impl block** (~525 lines, lines 507-1032). It contains 16 public methods including complex multi-table transactional operations like `persist_hitl_pause` (50 lines), `persist_hitl_answer_and_resume` (57 lines), `persist_hitl_cancel` (45 lines), and `persist_dispatch_cancel` (48 lines). These HITL/dispatch orchestration methods belong in a separate transaction-coordinator layer.

3. **Cross-domain coupling.** The run repository directly calls functions from `dispatch.rs` (`update_dispatch_row`, `resolve_update_conflict`), `hitl.rs` (`insert_hitl_request`, `load_required_hitl_request`, `ensure_pending_transition`, `next_sequence_no`). The file imports 7 items from `crate::dispatch` and 5 from `crate::hitl`.

4. **Three backward-compatible type aliases** (`WorkflowRunStore`, `WorkflowCheckpointStore`, `WorkflowSignalStore`) that should eventually be removed.

**Suggested split:**

| New file | Contents | Est. lines |
|----------|----------|:----------:|
| `workflow_store/types.rs` | Record types, status enums, `FromStr`/`Display` impls, error enum | ~500 |
| `workflow_store/run.rs` | `WorkflowRunRepository` CRUD methods only | ~400 |
| `workflow_store/checkpoint.rs` | `WorkflowCheckpointRepository` + pruning | ~200 |
| `workflow_store/signal.rs` | `WorkflowSignalRepository` | ~250 |
| `workflow_store/transitions.rs` | Multi-table transactional methods (`persist_hitl_*`, `persist_signal_*`, `persist_dispatch_cancel`) | ~350 |
| `workflow_store/sql.rs` | Private SQL helpers (`insert_*`, `update_*`, `read_*_row`, `list_*`) | ~300 |
| `workflow_store/mod.rs` | Re-exports, `WorkflowStoreSet` | ~50 |

Convert `workflow_store.rs` into a `workflow_store/` directory module.

---

### 3.2 `task.rs` (2,308 lines) -- Critical

**Problems:**

1. **Combined task + subtask in one file.** `TaskRepository` (212 lines), `SubtaskRepository` (100 lines), and all their private helpers are in one file.

2. **`list_tasks` and `list_subtasks` contain complex dynamic SQL builders** (lines 649-778 and 936-1069) that share the same structural pattern but differ in predicates. No query-builder abstraction is used.

3. **The `replan` method** (lines 297-397) is a 100-line transactional method that does bulk cancel, create, and update operations. It should be its own function or at minimum a separate helper.

4. **22-column SELECT lists** repeated 4 times.

5. **Multiple utility functions** private to this file that are duplicated elsewhere (`bool_to_sql`, `sql_to_bool`, `serialize_json_field`, etc.).

**Suggested split:**

| New file | Contents | Est. lines |
|----------|----------|:----------:|
| `task/types.rs` | Error enum, cursor types, `StoredTaskMetadata` | ~180 |
| `task/task_repo.rs` | `TaskRepository` impl + `replan` | ~400 |
| `task/subtask_repo.rs` | `SubtaskRepository` impl | ~200 |
| `task/sql.rs` | All private SQL helpers (`insert_task`, `load_task`, `read_task_row`, etc.) | ~600 |
| `task/mod.rs` | Re-exports, `TaskStoreSet` | ~50 |

---

### 3.3 `dispatch.rs` (1,626 lines) -- High

**Problems:**

1. **Mixing sync and async APIs.** The file defines an `async_trait DispatchRepository` (14 methods) and a `SqliteDispatchRepository` that implements it, but the underlying operations are all synchronous `lock_conn` calls. The async trait wrapper adds ceremony without providing actual asynchronous behavior.

2. **Verbose validation logic.** `validate_status_transition` (lines 899-1000) is ~100 lines of sequential field-equality checks followed by a state-machine match. The immutable-field checks could be a macro or a loop over field descriptors.

3. **18-column SELECT list** duplicated 5 times.

4. **`pub(crate)` helper functions** (`update_dispatch_row`, `resolve_update_conflict`, `load_dispatch`, `list_dispatch_summaries_by_run`) are used by `workflow_store.rs`, creating tight coupling.

**Recommendation:** The `pub(crate)` helpers that `workflow_store` needs should be extracted to a shared internal module rather than leaking internal implementation details between modules. Consider whether the `DispatchRepository` trait is worth the indirection given that the impl is purely synchronous.

---

### 3.4 `artifact.rs` (1,544 lines) and `doc.rs` (1,312 lines) -- High

These two files are **structurally isomorphic**. They share:

| Pattern | `artifact.rs` | `doc.rs` |
|---------|:---:|:---:|
| Error enum with identical variants | `ArtifactStoreError` | `DocStoreError` |
| Cursor types | `ArtifactCursor`, `ArtifactVersionCursor` | `DocCursor`, `DocVersionCursor` |
| Repository struct with `create`, `append_version`, `find_by_id`, `find_version_by_id`, `find_version_by_hash`, `list_versions`, `list`, `get_xxx` | Yes | Yes (same method names, s/artifact/doc/) |
| Private helpers: `map_insert_xxx_error`, `insert_xxx_version`, `list_xxxs`, `load_xxx`, `load_required_xxx`, `load_xxx_detail`, `load_xxx_version`, `load_required_xxx_version`, `list_xxx_versions` | Yes | Yes |
| Utility functions: `provenance_parts`, `ensure_object_json`, `serialize_json_field`, `is_unique_constraint_for`, `normalize_limit`, `encode_summary_cursor`, `decode_cursor` | Yes | Yes (identical implementations) |

These two files are essentially the **same code with different type names**. They share the same versioned-entity pattern: a stable parent row (`artifact`/`doc`) with immutable version rows and a `current_version_id` pointer.

**Recommendation:** Extract a generic versioned-entity repository:

```rust
pub struct VersionedEntityRepository<Id, VersionId, Record, VersionRecord, Error> {
    conn: Arc<Mutex<Connection>>,
    table_name: &'static str,
    version_table_name: &'static str,
    // ...
}
```

Or at minimum, extract the 7 duplicated utility functions into `common.rs` (~80 lines saved). In the long term, a trait-based approach could reduce these two 1,300+ line files to ~200 lines each of domain-specific code over a shared ~800-line generic foundation.

---

### 3.5 `hitl.rs` (1,436 lines) -- High

**Problems:**

1. **`pub(crate)` functions** (`insert_hitl_request`, `load_required_hitl_request`, `ensure_pending_transition`, `next_sequence_no`) are exported for use by `workflow_store.rs`. This creates a bidirectional coupling pattern: `workflow_store` imports from `hitl`, and `hitl` has no way to know which callers depend on its internals.

2. **Async trait with sync implementation** (same issue as `dispatch.rs`).

3. **`cancel` and `mark_timed_out` methods** are nearly identical (lines 423-463) -- both load, validate pending transition, update status, commit. Only the target status differs.

**Recommendation:** Merge the three status-transition methods (`answer`, `cancel`, `mark_timed_out`) into a single private `transition_status` method. Similar approach already used in `looper.rs` (line 328).

---

### 3.6 `runtime_store.rs` (1,413 lines) -- High

**Problems:**

1. **Six separate store structs** in one file: `AgentRuntimeStore`, `AgentSessionStore`, `AgentMessageStore`, `ScheduleRuntimeStore`, `ScheduleExecutionStore`, `TriggerRuntimeStore`. Each has its own `conn: Arc<Mutex<Connection>>` field and `new(conn)` constructor.

2. **Uses `OpenFangError` directly** instead of a domain-specific error type. This means errors from this store cannot be distinguished from errors in other layers.

3. **Inline encode/decode for `AgentState` and `AgentMode`** (lines 1001-1044) rather than using the `FromStr`/`Display` pattern used by every other store. This is inconsistent.

4. **`replace_messages_for_session`** (lines 529-579) is a 50-line transactional method that could be simplified.

**Suggested split:** Split into `runtime_store/agent.rs`, `runtime_store/session.rs`, `runtime_store/schedule.rs`, `runtime_store/trigger.rs`, and `runtime_store/mod.rs`.

---

### 3.7 `looper.rs` (1,303 lines) -- Medium

**Problems:**

1. **Intermediate raw-row types** (`LooperRunRow`, `LooperSubtaskRow`) are used to bridge between SQLite rows and domain records. This pattern is unique to this file -- all other stores read directly from `rusqlite::Row` into domain records.

2. **Execution-policy validation** happens inside the repository (`validate_new_run`, `decode_execution_policy`). Business-rule validation in the persistence layer violates separation of concerns.

**Recommendation:** Move policy validation to the caller (kernel or API layer). Keep the repository focused on CRUD.

---

### 3.8 `substrate.rs` (830 lines) -- Medium

**Problems:**

1. **Facade anti-pattern.** `MemorySubstrate` wraps 6 internal stores and re-exposes every method, creating a ~700-line pass-through facade. Most methods are one-liners delegating to `self.sessions.xxx()` or `self.structured.xxx()`.

2. **Async `Memory` trait implementation** (not shown in the excerpts but implied by the trait bound) with `spawn_blocking` calls wrapping synchronous SQLite operations. This is a pragmatic choice but adds overhead.

3. **`usage_conn()` leaks the raw connection**, defeating the purpose of the abstraction.

**Recommendation:** Consider whether the facade is providing value or just adding indirection. If callers need direct access to individual stores, expose the stores as public fields (like `RuntimeStoreSet` and `WorkflowStoreSet` already do) rather than wrapping every method.

---

### 3.9 `session.rs` (813 lines) -- Medium

**Problems:**

1. **MessagePack serialization** (`rmp_serde`) for message blobs while the rest of the codebase uses JSON. This creates a silent data-format inconsistency.

2. **Inline `ALTER TABLE` migration** (lines 128-136 in `structured.rs::save_agent`) for backward compatibility. This should be in `migration.rs`.

3. **`list_sessions` returns `Vec<serde_json::Value>`** rather than typed records. This is the only store that returns untyped JSON.

4. **Canonical session logic** (compaction, summary storage) is domain logic mixed into the persistence layer.

**Recommendation:** Type the return values. Move canonical-session compaction logic to a service layer.

---

### 3.10 `semantic.rs` (719 lines) -- Low

**Problems:**

1. **Manual parameter-index tracking** (`param_idx` counter incremented manually) in `has_memories`, `has_embedded_memories`, and `recall_with_embedding`. This is error-prone.

2. **`has_memories` and `has_embedded_memories`** share ~80% identical code (lines 84-132 vs. 135-183). The only difference is `AND embedding IS NOT NULL`.

3. **In-memory vector re-ranking** inside the persistence layer. Cosine-similarity calculation belongs in a search/ranking service.

**Recommendation:** Extract the filter-building code into a shared helper. Move re-ranking logic out of the store.

---

### 3.11 `migration.rs` (363 lines) -- Low

**Problems:**

1. **Only covers `runtime.db` migrations.** The `compozy.db` migrations are handled via `include_str!` constants scattered across individual store files and applied externally. There is no unified migration runner for `compozy.db`.

2. **No forward-migration tracking.** Uses `PRAGMA user_version` for versioning, which is fragile when multiple migration streams exist.

**Recommendation:** Consolidate all migration SQL into this module or a sibling `migrations/` module. Implement a single migration runner for both databases.

---

## 4. Architecture Concerns

### 4.1 Two-Database Split Without Unified Abstraction

The crate operates over two SQLite databases:
- **`runtime.db`** -- agent runtime, sessions, messages, schedules, triggers, memories, knowledge, usage
- **`compozy.db`** -- workflow runs, checkpoints, signals, dispatches, HITL, tasks, subtasks, loopers, artifacts, docs, packs

Each database has its own `Arc<Mutex<Connection>>` threaded through constructors. There is no shared abstraction for connection management, health checks, or shutdown.

### 4.2 Synchronous Mutex Over Async Boundaries

All stores use `std::sync::Mutex` to guard SQLite connections. When these are called from async contexts (via `async_trait` impls in `dispatch.rs` and `hitl.rs`, or `spawn_blocking` in `substrate.rs`), the mutex lock is held across await points indirectly. This is correct for SQLite's single-writer model but creates a bottleneck. Consider `tokio::sync::Mutex` if async callers dominate, or document the design decision.

### 4.3 Repository Pattern Inconsistency

| Store file | Error type | Naming convention | Trait abstraction |
|-----------|-----------|-------------------|:-----------------:|
| `workflow_store` | `WorkflowStoreError` | `Repository` suffix | No |
| `task` | `TaskStoreError` | `Repository` suffix | No |
| `dispatch` | `DispatchStoreError` | `Repository` suffix | Yes (`DispatchRepository`) |
| `hitl` | `HitlStoreError` | `Repository` suffix | Yes (`HitlRepository`) |
| `looper` | `LooperStoreError` | `Repository` suffix | No |
| `artifact` | `ArtifactStoreError` | `Repository` suffix | No |
| `doc` | `DocStoreError` | `Repository` suffix | No |
| `pack` | `PackStoreError` | `Repository` suffix | No |
| `runtime_store` | `OpenFangError` (generic) | `Store` suffix | No |
| `session` | `OpenFangError` (generic) | `Store` suffix (no struct suffix) | No |
| `semantic` | `OpenFangError` (generic) | `Store` suffix | No |
| `structured` | `OpenFangError` (generic) | `Store` suffix | No |
| `usage` | `OpenFangError` (generic) | `Store` suffix | No |

The codebase has two conventions:
- Newer `compozy.db` stores use `XxxRepository` naming + domain-specific `XxxStoreError` types
- Older `runtime.db` stores use `XxxStore` naming + the generic `OpenFangError`

**Recommendation:** Align on one convention. The `Repository` + typed error approach is superior.

---

## 5. Prioritized Action Plan

### Phase 1: Extract shared utilities (Critical, ~2 hours)

Create `src/common.rs` with:
- `bool_to_sql`, `sql_to_bool`
- `serialize_json_field`, `serialize_optional_json`, `serialize_optional_json_field`
- `ensure_object_json` (generic)
- `is_unique_constraint`, `is_unique_constraint_for`
- `normalize_limit`
- `provenance_parts`
- Generic `lock_conn` helper

**Impact:** Eliminates ~200 lines of duplication across 9 files. Makes future changes to utility logic require one edit instead of nine.

### Phase 2: Split `workflow_store.rs` into a directory module (Critical, ~4 hours)

Convert to `workflow_store/mod.rs` with sub-modules for types, run, checkpoint, signal, transitions, and SQL helpers. The transactional orchestration methods (`persist_hitl_*`, `persist_signal_*`) should move to a dedicated `transitions.rs`.

**Impact:** Reduces the largest file from ~3,000 lines to ~6 files of ~300-500 lines each. Makes it possible to work on signal logic without merge-conflicting with HITL logic.

### Phase 3: Deduplicate `artifact.rs` and `doc.rs` (High, ~3 hours)

Extract shared versioned-entity patterns. At minimum, move the 7 identical utility functions to `common.rs`. Ideally, create a shared base or macro for the isomorphic repository logic.

**Impact:** Eliminates ~400 lines of duplication between two 1,300+ line files.

### Phase 4: Split `task.rs` and `runtime_store.rs` (High, ~3 hours)

- `task.rs` -> `task/mod.rs`, `task/task_repo.rs`, `task/subtask_repo.rs`, `task/sql.rs`
- `runtime_store.rs` -> `runtime_store/mod.rs`, `runtime_store/agent.rs`, `runtime_store/schedule.rs`, `runtime_store/trigger.rs`

**Impact:** Improves navigability and reduces cognitive load.

### Phase 5: Standardize error types (Medium, ~2 hours)

Create a shared `MemoryStoreErrorBase` for the three common variants, or use a macro to generate the boilerplate. Migrate older stores from `OpenFangError` to domain-specific error types.

**Impact:** Better error diagnostics, consistent API across all stores.

### Phase 6: Extract SQL column constants (Medium, ~1 hour)

Define `const` strings for column lists used in SELECT/INSERT/UPDATE. Apply to `workflow_store`, `task`, `dispatch` first.

**Impact:** Eliminates ~300 lines of duplicated column lists. Makes schema changes safer.

### Phase 7: Address async-sync mismatch (Low, ~2 hours)

Evaluate whether `DispatchRepository` and `HitlRepository` async traits are justified. If all callers use `spawn_blocking` anyway, remove the async wrappers and simplify. If async is needed long-term (e.g., for a future non-SQLite backend), document the rationale.

### Phase 8: Clean up `session.rs` and `semantic.rs` (Low, ~2 hours)

- Type the return values of `list_sessions` (currently returns `Vec<serde_json::Value>`)
- Extract duplicate filter-building code in `semantic.rs`
- Move canonical-session compaction logic to a service layer
- Document the MessagePack vs. JSON serialization discrepancy

---

## 6. Dead Code and Cleanup Opportunities

| Item | Location | Issue |
|------|----------|-------|
| `WorkflowRunStore` type alias | `workflow_store.rs:499` | Backward-compat alias; schedule removal |
| `WorkflowCheckpointStore` type alias | `workflow_store.rs:502` | Same |
| `WorkflowSignalStore` type alias | `workflow_store.rs:505` | Same |
| `DispatchStore` type alias | `dispatch.rs:424` | Same |
| `HitlStore` type alias | `hitl.rs:321` | Same |
| `list_artifacts` alias method | `artifact.rs:280-285` | Duplicates `list()` |
| `list_docs` alias method | `doc.rs:268-271` | Duplicates `list()` |
| `list` alias method (on `WorkflowRunRepository`) | `workflow_store.rs:581-586` | Duplicates `list_runs()` |
| `table_exists` function | `workflow_store.rs:1698` | Only used for graceful degradation when `agent_dispatch` table missing; migration ordering should guarantee it exists |
| `usage_conn()` | `substrate.rs:92` | Leaks raw connection; callers should use the `UsageStore` directly |
| Inline `ALTER TABLE` in `save_agent` | `structured.rs:128-136` | Migration logic in CRUD method |

---

## 7. SQL Query Patterns

### 7.1 Dynamic Query Building

Three approaches are used for building filtered queries:

1. **String concatenation with positional params** (`task.rs`, `semantic.rs`, `knowledge.rs`) -- `sql.push_str(&format!("AND field = ?{param_idx}"))` with manual index tracking
2. **`(?1 IS NULL OR field = ?1)` pattern** (`dispatch.rs:800`) -- lets the query optimizer short-circuit; cleaner but can prevent index usage
3. **In-memory filtering after full load** (`workflow_store.rs:544-575`) -- `list_runs` loads ALL rows then calls `.retain()`. This will not scale.

**Recommendation:** Standardize on approach #1 or #2. The in-memory filtering in `list_runs` is a scaling liability that should be converted to SQL-level filtering.

### 7.2 Optimistic Concurrency

Several stores implement optimistic concurrency via `WHERE status = ?expected` in UPDATE statements, then check `rows == 0` and call `resolve_update_conflict` to produce a meaningful error. This pattern is consistent and well-implemented across `workflow_store`, `dispatch`, and `looper`.

### 7.3 Missing Indexes

No analysis of missing indexes was performed (would require looking at the migration SQL), but the in-memory filtering pattern in `list_runs` suggests that proper SQL indexes plus WHERE clauses would be more appropriate.

---

## 8. Summary

The `openfang-memory` crate has grown organically and suffers primarily from **code duplication** and **oversized files**. The architecture is sound -- the repository pattern is applied consistently (newer files), SQL is well-structured, and transactional boundaries are correct. The main wins come from:

1. **Extracting ~200 lines of duplicated utilities** into a shared module (quick win, high value)
2. **Splitting the two 2,300-3,000 line files** into directory modules (medium effort, high value)
3. **Deduplicating artifact/doc** which are structurally identical (medium effort, medium value)

No fundamental architectural rework is needed. The refactoring is primarily mechanical: extract, split, and deduplicate.
