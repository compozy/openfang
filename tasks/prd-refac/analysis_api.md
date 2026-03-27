# OpenFang API Crate -- Refactoring Analysis

**Date:** 2026-03-27
**Scope:** `crates/openfang-api/src/` (source) and `crates/openfang-api/tests/` (integration tests)
**Analyst:** automated deep-read

---

## 1. File Inventory

| File | Lines | Role |
|------|------:|------|
| `routes.rs` | 26,360 | Route handlers, shared state, helpers, inline tests |
| `channel_bridge.rs` | 1,866 | Channel adapter wiring (Telegram, Slack, etc.) |
| `types.rs` | 1,603 | Request/response DTOs |
| `ws.rs` | 1,363 | WebSocket real-time chat handler |
| `server.rs` | 1,342 | Router construction, CORS, daemon lifecycle |
| `trigger_definitions.rs` | 311 | Trigger definition file store |
| `workflow_definitions.rs` | 285 | Workflow definition file store |
| `middleware.rs` | 269 | API-key auth middleware |
| `stream_chunker.rs` | 244 | Streaming token chunker |
| `agent_definitions.rs` | 219 | Agent definition file store |
| `sse.rs` | 179 | Bounded SSE ring-buffer registry |
| `webchat.rs` | 169 | Static asset serving (logo, manifest, SW) |
| `stream_dedup.rs` | 160 | De-duplication filter for streaming events |
| `rate_limiter.rs` | 99 | GCRA rate limiter factory |
| `session_auth.rs` | 109 | Session token auth helpers |
| `openai_compat.rs` | 773 | OpenAI-compatible /v1/chat/completions |
| `lib.rs` | 21 | Module declarations |
| **Total source** | **35,372** | |

Integration tests (in `tests/`): 15 files, 13,446 lines total.

---

## 2. Critical Issues

### 2.1 `routes.rs` is a 26,360-line monolith (CRITICAL)

This is, by a large margin, the single most pressing problem. `routes.rs` contains:

- **~300 public handler functions** spanning every domain: agents, workflows, triggers, schedules, tasks, subtasks, loopers, dispatches, HITL, artifacts, docs, packs, skills, A2A, channels, uploads, config, auth, budget, usage, MCP, cron, approvals, webhooks, integrations, comms, pairing, and more.
- **~240 private helper functions** for error responses, record conversions, validation, pagination, pack deserialization, SSE streaming, etc.
- **~13 query-parameter structs** and local types (`RunListQueryParams`, `TriggerListQueryParams`, `ScheduleListQueryParams`, etc.).
- **8 inline `#[cfg(test)]` modules** totaling roughly 2,000+ lines of unit tests embedded in production code.
- **Global statics** (`AGENT_DEFINITION_WRITE_LOCK`, `WORKFLOW_DEFINITION_WRITE_LOCK`, `TRIGGER_DEFINITION_WRITE_LOCK`, `SCHEDULE_DEFINITION_WRITE_LOCK`, `PACK_TEMPLATE_WRITE_LOCK`, `UPLOAD_REGISTRY`).
- The **`AppState` struct** definition with 14 fields.

**Impact:** No one can review a diff that touches this file without loading 26K lines of context. Compile times for this single file are disproportionately high. Any merge conflict here is a nightmare.

**Recommended split** (by domain, matching the existing route groupings):

| Proposed module | Approximate line range | Contents |
|----------------|----------------------|----------|
| `routes/mod.rs` | -- | Re-exports, `AppState`, shared helpers (`parse_pagination_limit`, `parse_cursor_offset`, `workflow_v2_error_response`, `agent_error_response`, `operational_action_accepted_response`, etc.) |
| `routes/agents_v1.rs` | 2,700--5,000 | Agent definition CRUD, validation, compilation, runtime control, sessions, messages, dry-run |
| `routes/agents_legacy.rs` | 5,000--5,700 | Legacy `/api/agents` endpoints, spawn, kill, restart, set_model, set_tools, etc. |
| `routes/workflows_v1.rs` | 5,700--7,600 | Workflow definition CRUD, validate, compile, runs, dry-run |
| `routes/workflows_legacy.rs` | 9,900--10,100 | Legacy workflow CRUD endpoints |
| `routes/runs.rs` | 7,600--9,500 | Runs list/detail, checkpoints, dispatches, HITL, signals, pause/resume/cancel, SSE streaming |
| `routes/triggers_v1.rs` | 10,100--11,400 | Trigger definition CRUD, validate, compile, enable/disable, test, event ingress |
| `routes/triggers_legacy.rs` | 11,300--11,500 | Legacy trigger CRUD |
| `routes/schedules_v1.rs` | 20,100--21,700 | Schedule definition CRUD, validate, fork, runtime, enable/disable, run-now |
| `routes/tasks_v1.rs` | 1,500--2,700 | Task/subtask CRUD, replan, linked artifacts/docs/files |
| `routes/loopers_v1.rs` | 1,200--1,500 + 9,400--9,900 | Looper run CRUD, pause/resume/cancel, SSE |
| `routes/packs_v1.rs` | 13,500--15,200 | Pack install/upgrade/uninstall, object list, fork |
| `routes/skills.rs` | 15,200--15,700 | Skill list/detail, ClawHub marketplace |
| `routes/artifacts_docs.rs` | 2,400--2,550 | Artifact/doc list, detail, version history |
| `routes/a2a.rs` | 18,200--18,600 | A2A agent cards, send task, external discovery |
| `routes/channels.rs` | 12,700--13,500 | Channel list, configure, remove, test, reload, WhatsApp QR |
| `routes/uploads.rs` | 22,300--22,600 | Upload, serve, attachment resolution |
| `routes/config.rs` | 22,700--23,100 | Config reload, schema, set |
| `routes/system.rs` | 13,500--13,700 + 16,700--17,100 | Health, status, version, metrics, shutdown, security, tools, audit |
| `routes/budget.rs` | 17,100--17,350 | Budget status, per-agent ranking, update |
| `routes/models.rs` | 17,800--18,200 | Model/provider/alias list, custom model CRUD |
| `routes/sessions_legacy.rs` | 18,600--18,900 | Legacy session management |
| `routes/comms.rs` | 23,700--24,200 | Inter-agent communication topology, events, send, task |
| `routes/auth.rs` | 24,200--24,400 | Login, logout, auth check |
| `routes/hands.rs` | 15,700--16,700 | Hand/MCP server management |
| `routes/integrations.rs` | 19,800--20,100 | Integration list, add, remove, reconnect, health |
| `routes/cron.rs` | 23,000--23,250 | Cron job list, create, delete, toggle, status |
| `routes/webhooks.rs` | 23,200--23,400 | Webhook wake/agent |
| `routes/pairing.rs` | 23,400--23,600 | Device pairing flow |

This yields ~30 files, most under 500 lines, with the largest (agents_v1, runs) around 1,500--2,000 lines. This is a manageable size.

**Priority:** CRITICAL -- every other refactoring effort is blocked by this one file.

---

### 2.2 Duplicated error-response boilerplate (HIGH)

There are **22+ `_error_response` functions** and **13+ `_not_found_response` functions** in `routes.rs`. Many share an identical structure:

```rust
fn run_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({
        "error": { "code": "not_found", "message": "Run not found", "details": [] }
    })))
}

fn dispatch_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(StatusCode::NOT_FOUND, "not_found", "Dispatch not found", None)
}

fn hitl_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(StatusCode::NOT_FOUND, "not_found", "HITL request not found", None)
}

fn looper_run_not_found_response() -> (StatusCode, Json<serde_json::Value>) {
    workflow_v2_error_response(StatusCode::NOT_FOUND, "not_found", "Looper run not found", None)
}
// ... 9 more variants
```

Similarly, there are multiple near-identical `_internal_error_response`, `_pack_conflict_response`, and `_transition_response` functions that differ only in the `code` string and `message` text.

**Recommendation:** Replace with a small `ApiError` enum or a generic `not_found(resource: &str)` helper:

```rust
fn not_found(resource: &str) -> JsonErrorResponse {
    error_response(StatusCode::NOT_FOUND, "not_found", format!("{resource} not found"), None)
}
```

This would eliminate 30+ trivial wrapper functions.

**Priority:** HIGH

---

### 2.3 Duplicated `RouteTestContext` setup across inline test modules (HIGH)

The `routes.rs` file contains **4 separate** `RouteTestContext` structs and `route_test_context()` async factory functions, each in a different `#[cfg(test)]` module:

1. `looper_control_plane_route_tests` (line 94)
2. `pack_v1_route_tests` (line 14,802 -- a `PackRouteTestContext` variant)
3. `workflow_definition_v1_route_tests` (line 24,423)
4. `trigger_definition_v1_route_tests` (line 25,676)
5. `task_control_plane_route_tests` (line 26,001)

Each one repeats the same 30-line kernel boot + AppState construction. Additionally, the `json_response()` helper is copy-pasted into several modules.

**Recommendation:** Extract a shared `#[cfg(test)] mod test_support` module with:
- `RouteTestContext` struct
- `route_test_context()` async factory
- `json_response()` helper
- `sample_task()` / `sample_subtask()` / `sample_looper_policy()` fixture factories

**Priority:** HIGH

---

### 2.4 Duplicated `_store_error_response` match arms (MEDIUM)

Three large match functions handle store errors with nearly identical structure:

| Function | Lines | Matches |
|----------|------:|---------|
| `task_store_error_response` | ~165 | 15 `TaskStoreError` variants |
| `artifact_store_error_response` | ~70 | 8 `ArtifactStoreError` variants |
| `doc_store_error_response` | ~65 | 8 `DocStoreError` variants |

Many of the arms follow the same pattern: internal errors that return 500 with `{ "message": error_string }`. At minimum, the "fallthrough internal error" arms for `Sqlite`, `Json`, `ConnectionLock`, `InvalidJsonField`, etc. should be collapsed via a trait impl:

```rust
impl From<TaskStoreError> for JsonErrorResponse { ... }
```

This would replace ~300 lines of boilerplate with ~50 lines of trait impls.

**Priority:** MEDIUM

---

### 2.5 `apply_task_update` / `apply_subtask_update` are manual field-by-field copy (MEDIUM)

Both functions manually check ~15 `Option` fields with `if let Some(...) { next.field = ... }` patterns. This is fragile: adding a new field to `TaskRecord` or `SubtaskRecord` requires remembering to update both the update function and the create function.

**Recommendation:** Consider a macro or derive for partial-update semantics, or at minimum a merge utility that operates on `serde_json::Value` representations.

**Priority:** MEDIUM

---

### 2.6 Tight coupling: `routes.rs` directly accesses kernel internals (MEDIUM)

Many handler functions reach deep into the kernel via chains like:

```rust
state.kernel.workflow_stores.task.find_by_id(...)
state.kernel.workflow_stores.subtask.list_for_task(...)
state.kernel.workflow_stores.dispatch.list(...)
state.kernel.runtime_stores.agent_runtime.get_agent_runtime(...)
state.kernel.cron_scheduler.get_meta_by_definition_id(...)
state.kernel.trigger_v2.upsert_definition(...)
```

This creates tight coupling between the API layer and kernel internals. If the kernel's internal store structure changes, dozens of route handlers break.

**Recommendation:** Introduce a thin service/facade layer on the kernel that exposes higher-level operations, so routes call `state.kernel.get_task(id)` rather than reaching into store hierarchies.

**Priority:** MEDIUM (defer until after the file split)

---

### 2.7 `server.rs` router construction is 600+ lines of `.route()` calls (MEDIUM)

The `build_router()` function in `server.rs` (lines 134--750+) is a single chain of ~180 `.route()` calls. This is a natural consequence of having all routes in one module, but even after splitting `routes.rs`, the router should be broken up.

**Recommendation:** Each domain module should expose a `fn router() -> Router<Arc<AppState>>` that contributes its routes. The top-level `build_router()` then merges them:

```rust
let app = Router::new()
    .merge(routes::agents_v1::router())
    .merge(routes::workflows_v1::router())
    .merge(routes::triggers_v1::router())
    // ...
```

**Priority:** MEDIUM (do alongside the routes split)

---

### 2.8 `types.rs` mixes domain-specific types with shared types (LOW)

At 1,603 lines, `types.rs` contains request/response types for every domain: agents, workflows, triggers, schedules, tasks, subtasks, loopers, packs, skills, dispatches, HITL, sessions, budget, A2A, etc. While not as severe as `routes.rs`, it would benefit from splitting into domain-aligned modules as part of the broader refactoring.

**Recommendation:** Split `types.rs` into `types/mod.rs` + domain modules that mirror the route split.

**Priority:** LOW (can be deferred)

---

### 2.9 `channel_bridge.rs` has ~50 adapter imports but no dynamic dispatch (LOW)

The file imports 30+ channel adapters individually:

```rust
use openfang_channels::telegram::TelegramAdapter;
use openfang_channels::slack::SlackAdapter;
use openfang_channels::discord::DiscordAdapter;
// ... 27 more
```

Each adapter is instantiated via a large match/if-chain in `start_channel_bridge()`. This is a maintenance burden when adding new channels.

**Recommendation:** Consider an adapter registry pattern where adapters self-register, or at minimum a macro that generates the match arms.

**Priority:** LOW

---

### 2.10 Inline `#[cfg(test)]` modules inflate `routes.rs` by ~2,000 lines (MEDIUM)

The 8 inline test modules add significant bulk. While Rust convention allows inline tests, at this file size they are a liability.

**Recommendation:** Move all inline route tests to dedicated files under `tests/` or a `routes/tests/` submodule. This immediately removes 2,000 lines from `routes.rs`.

**Priority:** MEDIUM (easy win)

---

### 2.11 Global statics for write locks are not extensible (LOW)

Five `LazyLock<Mutex<()>>` statics are used for serializing writes to different definition stores:

```rust
static AGENT_DEFINITION_WRITE_LOCK: LazyLock<Mutex<()>> = ...;
static WORKFLOW_DEFINITION_WRITE_LOCK: LazyLock<Mutex<()>> = ...;
static TRIGGER_DEFINITION_WRITE_LOCK: LazyLock<Mutex<()>> = ...;
static SCHEDULE_DEFINITION_WRITE_LOCK: LazyLock<Mutex<()>> = ...;
static PACK_TEMPLATE_WRITE_LOCK: LazyLock<Mutex<()>> = ...;
```

**Recommendation:** Move these into `AppState` or into the respective definition store structs. This removes global state and makes them testable.

**Priority:** LOW

---

### 2.12 `ws.rs` -- well-scoped but has a 200-line session rendering block (LOW)

The `get_agent_session` handler (in `routes.rs`, not `ws.rs`) has a ~200-line block for building session messages with tool-use/result correlation. This presentation logic belongs in a separate module (e.g., `session_view.rs`).

**Priority:** LOW

---

### 2.13 `openai_compat.rs` -- self-contained, no issues (INFORMATIONAL)

At 773 lines, this module is well-scoped and has a clear single responsibility. No action needed.

---

### 2.14 Dead code / unused imports (LOW)

No significant dead code was observed -- clippy with `-D warnings` enforces this. However, there are several `#[allow(dead_code)]` annotations (e.g., `UploadMeta.filename`) that should be reviewed to determine if the field is genuinely needed.

**Priority:** LOW

---

## 3. Naming Issues

| Current | Issue | Suggested |
|---------|-------|-----------|
| `workflow_v2_error_response` | Used for tasks, subtasks, dispatches, HITL, triggers, schedules -- not just workflows | `api_error_response` |
| `workflow_v2_json_rejection` | Same as above | `json_rejection_response` |
| `task_query_rejection` | Delegates to `workflow_v2_error_response` | `query_rejection_response` |
| `agent_error_response` vs `workflow_v2_error_response` | Two nearly identical functions for constructing JSON error responses | Consolidate into one |
| `run_action_accepted_response` / `agent_action_accepted_response` / `operational_action_accepted_response` | Three functions that do nearly the same thing | Consolidate |
| `list_agents` / `list_agents_legacy` | Inconsistent naming -- the legacy endpoint should be prefixed or deprecated | Deprecate legacy endpoints |

---

## 4. Complexity Hotspots

### Functions > 100 lines:

| Function | Lines (approx.) | Domain |
|----------|------:|--------|
| `get_agent_session` | ~200 | Session message rendering with tool correlation |
| `send_message_stream` | ~180 | SSE streaming with tool events |
| `list_agents_legacy` | ~90 | Agent listing with catalog enrichment |
| `stream_run_events_v1` | ~250 | Durable SSE polling loop |
| `stream_dispatch_events_v1` | ~230 | Durable SSE polling loop |
| `stream_hitl_requests_v1` | ~270 | Durable SSE polling loop |
| `stream_looper_run_events_v1` | ~170 | Looper SSE streaming |
| `upload_file` | ~125 | File upload with audio transcription |
| `task_store_error_response` | ~165 | Error variant matching |
| `set_agent_file` | ~110 | File write with path traversal checks |
| `build_router` (server.rs) | ~600+ | Route registration chain |

The three `stream_*_events_v1` functions share a nearly identical polling-loop structure with minor differences in what's polled and what events are emitted. They should be extracted into a generic durable-SSE-stream function parameterized by a snapshot loader and event transformer.

---

## 5. Architecture Concerns

### 5.1 No service layer between routes and kernel

Route handlers directly orchestrate multi-step business logic: load from store, validate, normalize, compile, persist, register in runtime. This means:
- Business logic is untestable without standing up an HTTP server
- The same logic cannot be reused from the CLI without the API layer

### 5.2 Two API surfaces (legacy + v1) without clear deprecation

The crate maintains both `/api/agents` (legacy) and `/api/v1/agents` (v1) endpoints. Many legacy handlers duplicate logic from v1 handlers in a subtly different way. There is no deprecation header, no migration guide, and no clear timeline for removal.

### 5.3 SSE streaming architecture is fragile

Each SSE endpoint (`stream_run_events_v1`, `stream_dispatch_events_v1`, `stream_hitl_requests_v1`) implements its own polling loop with fingerprinting, keepalive, and reset logic. A change to the polling pattern requires updating 3-4 nearly identical implementations.

---

## 6. Test Coverage Structure

### Inline tests (in `routes.rs`):
- `looper_control_plane_route_tests` -- 6 tests
- `pack_v1_route_tests` -- 4 tests
- `workflow_definition_v1_route_tests` -- ~20 tests
- `trigger_definition_v1_route_tests` -- ~6 tests
- `task_control_plane_route_tests` -- ~6 tests
- `skill_v1_route_tests` -- 2 tests

### Integration tests (in `tests/`):
| File | Lines | Coverage area |
|------|------:|--------------|
| `api_integration_test.rs` | 2,412 | Broad agent/session/message integration |
| `dispatch_hitl_v1_api_test.rs` | 2,527 | Dispatch and HITL lifecycle |
| `agent_v2_api_test.rs` | 1,000 | V1 agent definition API |
| `pack_v1_api_test.rs` | 936 | Pack install/upgrade/fork |
| `looper_v1_api_test.rs` | 918 | Looper run lifecycle |
| `workflow_v2_api_test.rs` | 889 | Workflow definition API |
| `event_ingress_v1_api_test.rs` | 809 | Event ingress and trigger evaluation |
| `schedule_v1_api_test.rs` | 648 | Schedule definition API |
| `task_v1_api_test.rs` | 628 | Task/subtask CRUD |
| `artifact_doc_v1_api_test.rs` | 600 | Artifact and doc endpoints |
| `load_test.rs` | 594 | Load/stress testing |
| `trigger_v2_api_test.rs` | 477 | Trigger v2 API |
| `workflow_definition_consistency_test.rs` | 386 | Cross-definition consistency |
| `skill_v1_api_test.rs` | 314 | Skill listing API |
| `daemon_lifecycle_test.rs` | 308 | Daemon start/stop lifecycle |

Total test code: **~15,500 lines** (inline + integration).

---

## 7. Recommended Refactoring Plan

### Phase 1: Split `routes.rs` (CRITICAL -- do first)

1. Create `src/routes/` directory with `mod.rs`
2. Move `AppState` + shared helpers to `mod.rs`
3. Extract each domain group into its own file (see table in 2.1)
4. Move inline `#[cfg(test)]` modules to `routes/tests/` or to `tests/`
5. Update `server.rs` to merge sub-routers

**Estimated effort:** 2-3 days (mechanical extraction, no logic changes).
**Risk:** Low -- purely structural, tests verify correctness.

### Phase 2: Consolidate error helpers (HIGH)

1. Create `src/routes/error.rs` with unified `api_error_response`, `not_found`, `internal_error`, `transition_conflict`, `json_rejection_response`, and `query_rejection_response`
2. Remove all domain-specific wrapper functions
3. Implement `From<TaskStoreError>`, `From<ArtifactStoreError>`, `From<DocStoreError>` for a unified error type

**Estimated effort:** 1 day.
**Risk:** Low.

### Phase 3: Extract shared test infrastructure (HIGH)

1. Create `src/routes/test_support.rs` (or `tests/common/mod.rs`)
2. Move duplicated `RouteTestContext`, `route_test_context()`, `json_response()`, and fixture factories
3. Update all test modules to import from the shared module

**Estimated effort:** Half day.
**Risk:** None.

### Phase 4: Genericize SSE streaming (MEDIUM)

1. Extract a `DurableSseStream<S, T>` utility that accepts a snapshot loader and event transformer
2. Replace the 3 copy-pasted polling loops with parameterized instances

**Estimated effort:** 1 day.
**Risk:** Medium -- SSE timing is tricky to test.

### Phase 5: Split `server.rs` router registration (MEDIUM)

1. Each route module exposes `fn router() -> Router<Arc<AppState>>`
2. `build_router()` merges sub-routers
3. Each module owns its route paths

**Estimated effort:** Half day (mechanical, follows Phase 1).

### Phase 6: Deprecate legacy endpoints (LOW, ongoing)

1. Add `Deprecation` headers to legacy endpoints
2. Log warnings when legacy endpoints are hit
3. Set a removal timeline

---

## 8. Summary Table

| Issue | File(s) | Priority | Effort | Section |
|-------|---------|----------|--------|---------|
| `routes.rs` is a 26K-line monolith | `routes.rs` | CRITICAL | 2-3 days | 2.1 |
| 22+ duplicated error-response functions | `routes.rs` | HIGH | 1 day | 2.2 |
| 4 duplicated `RouteTestContext` setups | `routes.rs` | HIGH | 0.5 day | 2.3 |
| Duplicated store error match arms | `routes.rs` | MEDIUM | 1 day | 2.4 |
| Manual field-by-field update functions | `routes.rs` | MEDIUM | 0.5 day | 2.5 |
| Routes reach deep into kernel internals | `routes.rs` | MEDIUM | 2+ days | 2.6 |
| 600-line router construction chain | `server.rs` | MEDIUM | 0.5 day | 2.7 |
| 2,000 lines of inline tests | `routes.rs` | MEDIUM | 0.5 day | 2.10 |
| 3 copy-pasted SSE polling loops | `routes.rs` | MEDIUM | 1 day | 4 |
| Mixed naming (`workflow_v2_*` used everywhere) | `routes.rs` | LOW | 0.5 day | 3 |
| types.rs mixes all domains | `types.rs` | LOW | 0.5 day | 2.8 |
| 30+ channel adapter imports | `channel_bridge.rs` | LOW | 0.5 day | 2.9 |
| Global write-lock statics | `routes.rs` | LOW | 0.25 day | 2.11 |
| Legacy + v1 API duplication | `routes.rs`, `server.rs` | LOW | ongoing | 5.2 |
