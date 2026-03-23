# PRD Compozy: Pre-Implementation Decisions

**Date:** 2026-03-23
**Status:** Approved
**Scope:** Resolves all critical gaps identified in the PRD readiness analysis before implementation begins.

---

## 1. Task 9/14/15/17 Ownership Split

These four tasks overlap on workflow persistence. Clear ownership boundaries:

| Responsibility | Owner Task | Details |
|---|---|---|
| Migration SQL (workflow_run, workflow_checkpoint, workflow_signal tables) | **Task 9** | Owns all DDL. No other task creates these tables. |
| Store adapters (CRUD: insert, get, list, update_status) | **Task 9** | Thin wrappers over SQL. Consistent `*Store` naming: `WorkflowRunStore`, `WorkflowCheckpointStore`, `WorkflowSignalStore`. |
| `TransitionWriter` (orchestrates checkpoint + run update atomically) | **Task 14** | Business logic over Task 9's stores. Does NOT create migrations. |
| Signal idempotency, eager-consume path, consume logic | **Task 15** | Extends `WorkflowSignalStore` from Task 9. Does NOT create a parallel `WorkflowSignalRepository`. |
| Recovery scan, pause/resume/cancel control surfaces, API routes | **Task 17** | Operational layer. Uses Task 14's `TransitionWriter` and Task 9's stores. |

**Naming rule:** Use `*Store` consistently (Task 9 creates). `TransitionWriter` is the only higher-level abstraction (Task 14). Tasks 15 and 17 extend stores, never create parallel ones.

---

## 2. Recovered Run Status: `paused`

Runs that were `running` at crash time are downgraded to `paused` on restart. This aligns with 3 of 4 sources (INITIAL-RUNTIME-MIGRATIONS.md, Task 9, Task 17).

To distinguish crash-paused from user-paused, the recovery scan writes a checkpoint with `kind = run_recovered_needs_resume`. No additional status enum variant is needed.

Task 14's reference to `interrupted` status must be corrected to `paused`.

---

## 3. Phantom Dependency Removal

Remove factually incorrect dependencies. Execution remains sequential (task ordering is respected).

| Task | Remove Dep | Reason |
|---|---|---|
| 4 (Arky Crates) | ~~task1~~ → `none` | Does not use PersistenceConfig |
| 5 (Contract Types) | ~~task1~~ → `none` | Does not use PersistenceConfig |
| 7 (Workflow Source-of-Truth) | ~~task2~~ → `none` | Works with filesystem, not dual-db bootstrap |
| 19 (agent_dispatch Schema) | ~~task18~~ → keep task9, task17 | Schema does not need API surfaces |
| 28 (Looper Runtime) | ~~task26~~ → keep task23 | Runtime does not need task API layer |
| 29 (Trigger & Event Ingress) | ~~task27, task21~~ → keep task13 | Does not need dispatch API or workflow CRUD |

---

## 4. Source of Truth Policy

Task files and the techspec table are complementary references. Neither is exclusively canonical. The `docs/` directory provides additional authoritative context.

**Rule:** When a contradiction exists between any two sources, it must be resolved explicitly (not silently ignored). The resolution is written into both sources so they agree.

---

## 5. Task Splits and Renumbering

Five oversized tasks are split. Three new tasks are added. Total: 32 original → 43 tasks.

### Split Map

| Original | New Tasks | Rationale |
|---|---|---|
| **Task 13** (Workflow v2 Definition Schema And Compile Pipeline) | **13**: Workflow v2 Definition Types (step kinds, flow modes, fields) | Types are independent of compile logic |
| | **14**: Workflow v2 Compile Pipeline (validate, normalize, compile → WorkflowIr) | Core compiler, testable in isolation |
| | **15**: Workflow v2 API Endpoints (validate, compile, compiled) | API layer over compiler |
| **Task 18** (Agent Control-Plane Surfaces) | **20**: Agent Definition CRUD And Compile Routes | Definition management |
| | **21**: Agent Runtime Operational Sub-Resources | Runtime state queries |
| | **22**: Agent Sessions Messages And SSE Streaming | Streaming and session management |
| **Task 25** (HITL Mid-Step Pause And Resume) | **30**: HITL Single-Turn Live Pause And Resume | Core mechanism with oneshot channel |
| | **31**: HITL Post-Restart Reconstruction | Recovery path, separate concern |
| **Task 29** (Trigger And Event Ingress) | **35**: Trigger v2 Types And Definition CRUD | Type system and definition management |
| | **36**: Event Ingress Pipeline And Match Engine | Runtime event processing |
| **Task 32** (Final Hardening And E2E Integration) | **41**: Pack System Install Upgrade And Bootstrap | Self-contained feature |
| | **42**: Retention Policies And Remaining SSE Endpoints | Infrastructure hardening |
| | **43**: E2E Integration Test And Restart Recovery Regression | Validation pass |

### New Tasks

| # | Title | Phase | Dependencies | Rationale |
|---|---|---|---|---|
| **27** | Skills Listing Endpoint | 2 | task26 | Control-plane-first principle requires skills to be API-visible |
| **38** | Artifact And Doc Standalone Read Endpoints | 5 | task37 | DB tables exist but no direct API access without going through tasks |
| **40** | Pack List Detail And CRUD Endpoints | 5 | task39 | API spec defines pack endpoints; must exist before install/upgrade |

### Full Renumbering Map

| Old # | New # | Title |
|---|---|---|
| 1 | 1 | Split Persistence Config For Dual Databases |
| 2 | 2 | Dual-Database Bootstrap In Kernel Startup |
| 3 | 3 | Reusable Migration Runner For Both Databases |
| 4 | 4 | Copy Arky Crates Into OpenFang Workspace |
| 5 | 5 | Shared Definition Contract Types |
| 6 | 6 | Initial runtime.db Schema And Stores |
| 7 | 7 | Workflow Definition Source-Of-Truth Consistency |
| 8 | 8 | Workflow Bootstrap And Readiness Semantics |
| 9 | 9 | Initial compozy.db Workflow Core Schema |
| 10 | 10 | Provider Layering For Workspace Profiles And Agent Config |
| 11 | 11 | ProviderBinding Compile Layer For Compozy Agents |
| 12 | 12 | Typed Provider Integration For Codex And Claude Code |
| 13 (split) | **13** | **Workflow v2 Definition Types** |
| 13 (split) | **14** | **Workflow v2 Compile Pipeline** |
| 13 (split) | **15** | **Workflow v2 API Endpoints** |
| 14 | 16 | Durable Workflow Run Repository And Transition Writer |
| 15 | 17 | Workflow Signal Persistence And Waiting-State Integration |
| 16 | 18 | Agent Definition Validation And Compile Pipeline |
| 17 | 19 | Restart Recovery And Durable Run Control Surfaces |
| 18 (split) | **20** | **Agent Definition CRUD And Compile Routes** |
| 18 (split) | **21** | **Agent Runtime Operational Sub-Resources** |
| 18 (split) | **22** | **Agent Sessions Messages And SSE Streaming** |
| 19 | 23 | agent_dispatch Schema And Persistence Layer |
| 20 | 24 | hitl_request Schema And Persistence Layer |
| 21 | 25 | Workflow Definition CRUD Control-Plane Surfaces |
| 22 | 26 | Schedule Control-Plane Surfaces |
| NEW | **27** | **Skills Listing Endpoint** |
| 23 | 28 | Task And Subtask Domain Schema And Repositories |
| 24 | 29 | Dispatch Runtime Integration With Provider-Native Sessions |
| 25 (split) | **30** | **HITL Single-Turn Live Pause And Resume** |
| 25 (split) | **31** | **HITL Post-Restart Reconstruction** |
| 26 | 32 | Task And Subtask Control-Plane Plus Replanning |
| 27 | 33 | Dispatch And HITL Control-Plane Surfaces |
| 28 | 34 | Looper Durable Schema And Runtime |
| 29 (split) | **35** | **Trigger v2 Types And Definition CRUD** |
| 29 (split) | **36** | **Event Ingress Pipeline And Match Engine** |
| 30 | 37 | Artifact And Doc Versioning |
| NEW | **38** | **Artifact And Doc Standalone Read Endpoints** |
| 31 | 39 | Looper Control-Plane And SSE Surfaces |
| NEW | **40** | **Pack List Detail And CRUD Endpoints** |
| 32 (split) | **41** | **Pack System Install Upgrade And Bootstrap** |
| 32 (split) | **42** | **Retention Policies And Remaining SSE Endpoints** |
| 32 (split) | **43** | **E2E Integration Test And Restart Recovery Regression** |

---

## 6. HITL Signal Mechanism: tokio::oneshot Channel

When an agent step needs human input:

1. Step creates a `tokio::oneshot::channel()`
2. Registers `Sender` in a `HashMap<HitlRequestId, oneshot::Sender<HitlAnswer>>` on the workflow engine
3. Writes `hitl_request` row to `compozy.db` with status `pending`
4. Writes checkpoint `kind = hitl_requested`
5. Awaits the `Receiver` future

When the answer arrives via API:

1. Updates `hitl_request.status` to `answered`, writes `response_payload`
2. Looks up `Sender` in the HashMap, sends the answer
3. Step resumes with the answer

**Post-restart reconstruction:** No live channel exists. The recovery scan finds `hitl_request` rows with `status = pending`. These remain pending until answered. When answered, the workflow engine re-executes the step from the checkpoint (the step must be idempotent up to the HITL request point).

---

## 7. Dual-Database Consistency: Leader DB + Reconciliation

Each cross-database operation has a **leader database** where the primary write happens. The secondary write is best-effort with retry.

| Operation | Leader DB | Secondary DB | Reconciliation |
|---|---|---|---|
| Workflow run created by schedule | `compozy.db` (workflow_run) | `runtime.db` (schedule_execution) | Boot scan: orphaned schedule_execution without matching workflow_run → log warning |
| Agent dispatch from workflow | `compozy.db` (agent_dispatch) | `runtime.db` (agent_runtime) | Boot scan: dispatches in `running` with no active agent session → downgrade to `paused` |
| Schedule fires workflow | `compozy.db` (workflow_run) | `runtime.db` (schedule_execution receipt) | Leader write first; if secondary fails, log and retry on next boot |

**Boot reconciliation scan** runs after both databases are open and migrations are applied, before any subsystem starts accepting work.

---

## 8. Template Engine: minijinja

Use `minijinja` for all `{{ vars.foo }}` expressions in workflows and triggers.

- Lightweight, Jinja2 syntax, well-maintained Rust crate
- Used for: workflow step `with` blocks, `outputs` mapping, trigger target payloads
- Evaluation happens at **runtime** (not compile time). The compile phase only validates that template syntax is parseable and referenced variables exist in scope.

---

## 9. Crate Placement: Modules in Existing Crates

No new crates. All new code goes into existing crate modules:

| Component | Location |
|---|---|
| Migration runner | `crates/openfang-kernel/src/db_migration.rs` |
| Workflow stores | `crates/openfang-memory/src/workflow/` (mod.rs, run_store.rs, checkpoint_store.rs, signal_store.rs) |
| Domain stores (task, subtask) | `crates/openfang-memory/src/domain/` |
| Dispatch/HITL stores | `crates/openfang-memory/src/dispatch.rs`, `crates/openfang-memory/src/hitl.rs` |
| Looper stores | `crates/openfang-memory/src/looper.rs` |
| Artifact/Doc stores | `crates/openfang-memory/src/artifact.rs`, `crates/openfang-memory/src/doc.rs` |
| ProviderBinding type | `crates/openfang-types/src/provider_binding.rs` |
| CompozyError enum | `crates/openfang-types/src/error.rs` (extend existing) |
| Contract types | `crates/openfang-types/src/contract.rs` |
| Definition store (file I/O) | `crates/openfang-kernel/src/definition_store.rs` |
| Workflow v2 types | `crates/openfang-types/src/workflow_v2.rs` |
| Workflow compiler | `crates/openfang-kernel/src/workflow_compiler.rs` |
| Agent compiler | `crates/openfang-kernel/src/agent_compiler.rs` |

---

## 10. Migration Numbering: Timestamp Prefix

Format: `YYYYMMDD_NNN_description.sql`

Examples:
```
migrations/runtime/
  20260321_001_agent_runtime_tables.sql
  20260321_002_schedule_runtime_tables.sql
  20260321_003_trigger_runtime_table.sql

migrations/compozy/
  20260321_001_workflow_core_tables.sql
  20260322_001_dispatch_table.sql
  20260322_002_hitl_request_table.sql
  20260323_001_task_subtask_tables.sql
  20260324_001_looper_tables.sql
  20260325_001_artifact_doc_tables.sql
```

The `NNN` counter resets per date. Collisions are impossible if each task uses its implementation date.

---

## 11. Graceful Shutdown Protocol

On SIGTERM (via existing `CancellationToken` infrastructure):

1. Cancel the `CancellationToken` (already wired)
2. Workflow engine: for each `running` workflow run, write checkpoint `kind = shutdown_requested` and update status to `paused`
3. Looper: for each active looper run, write checkpoint and update status to `paused`
4. Wait up to 5 seconds for in-flight DB writes to complete
5. Flush SQLite WAL on both databases (`PRAGMA wal_checkpoint(TRUNCATE)`)
6. Close database connections
7. Exit

On next boot, the recovery scan handles any state that was not cleanly persisted.

---

## 12. Error Handling: CompozyError Enum

Extend `crates/openfang-types/src/error.rs` with a `CompozyError` enum:

```rust
#[derive(Error, Debug)]
pub enum CompozyError {
    #[error("Workflow error: {0}")]
    Workflow(#[from] WorkflowError),

    #[error("Dispatch error: {0}")]
    Dispatch(#[from] DispatchError),

    #[error("HITL error: {0}")]
    Hitl(#[from] HitlError),

    #[error("Task error: {0}")]
    Task(#[from] TaskError),

    #[error("Looper error: {0}")]
    Looper(#[from] LooperError),

    #[error("Persistence error: {0}")]
    Persistence(String),

    #[error("Validation error: {issues:?}")]
    Validation { issues: Vec<ValidationIssue> },
}
```

Each domain defines its own error enum (e.g., `WorkflowError`, `DispatchError`). `CompozyError` aggregates them. The API layer implements `Into<ApiError>` mapping `CompozyError` variants to HTTP status codes and stable error codes.

---

## 13. trigger_runtime Table in Task 6

Add `trigger_runtime` to Task 6's `runtime.db` schema:

```sql
CREATE TABLE IF NOT EXISTS trigger_runtime (
    trigger_id   TEXT PRIMARY KEY,
    enabled      INTEGER NOT NULL DEFAULT 1,
    fire_count   INTEGER NOT NULL DEFAULT 0,
    last_fired_at TEXT,
    loaded_at    TEXT NOT NULL,
    updated_at   TEXT NOT NULL
);
```

This aligns with `schedule_runtime` already in `runtime.db` and provides backing for the API's `runtime_status` response on triggers.

---

## 14. File I/O Infrastructure: definition_store.rs

Shared module at `crates/openfang-kernel/src/definition_store.rs`:

```rust
/// Write a definition to disk atomically (write .tmp, then rename).
pub fn write_definition<T: Serialize>(dir: &Path, id: &str, value: &T) -> Result<PathBuf>;

/// Load all definitions from a directory.
pub fn load_definitions<T: DeserializeOwned>(dir: &Path) -> Result<Vec<(String, T)>>;

/// Delete a definition file.
pub fn delete_definition(dir: &Path, id: &str) -> Result<()>;

/// Atomic write: serialize to .tmp file, then rename to final path.
fn atomic_write(path: &Path, content: &[u8]) -> Result<()>;
```

Tasks 25 (workflow CRUD), 26 (schedule CRUD), and 35 (trigger CRUD) all use this shared infrastructure.

---

## 15. Phase Assignment (Updated)

| Phase | Tasks | Goal |
|---|---|---|
| **0** | 1-9 | Dual-database bootstrap, migration infra, config, Arky workspace, contract types, runtime.db schema, compozy.db workflow schema |
| **1** | 10-22 | Provider layering, workflow v2 compile, durable runs, agent compile, restart recovery, agent control-plane |
| **2** | 23-33 | Dispatch, HITL, workflow CRUD, schedule CRUD, skills, task/subtask domain, control-plane surfaces |
| **3** | 34-36 | Looper runtime, trigger v2, event ingress |
| **4** | 37-40 | Artifact/doc versioning, looper control-plane, pack CRUD |
| **5** | 41-43 | Pack system, retention, E2E integration |
