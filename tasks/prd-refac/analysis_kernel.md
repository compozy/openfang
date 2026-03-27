# OpenFang Kernel Crate -- Deep Refactoring Analysis

**Date:** 2026-03-27
**Crate:** `openfang-kernel` (`crates/openfang-kernel/src/`)
**Total lines:** ~37,881 across 30 source files

---

## Executive Summary

The kernel crate has grown into a monolithic God Object centered around `kernel.rs`
(10,936 lines) and `workflow.rs` (9,402 lines). Together these two files account
for 54% of the crate. The `OpenFangKernel` struct holds 64 fields and directly
orchestrates agent lifecycle, workflow dispatch, looper execution, HITL,
MCP connections, cron scheduling, background tasks, OFP networking, WhatsApp
gateway management, driver resolution, tool filtering, and prompt construction.
This violates the Single Responsibility Principle at every level.

The codebase is functional and well-tested. The refactoring recommended here
is about structural health -- making the code easier to navigate, test in
isolation, and extend without increasing coupling.

---

## File Inventory

| File | Total Lines | Prod Lines | Test Lines | Priority |
|------|-------------|------------|------------|----------|
| `kernel.rs` | 10,936 | ~9,346 | ~1,590 | **Critical** |
| `workflow.rs` | 9,402 | ~4,876 | ~4,526 | **Critical** |
| `workflow_compiler.rs` | 1,997 | ~1,414 | ~583 | Medium |
| `looper.rs` | 1,936 | ~1,001 | ~935 | Medium |
| `trigger_v2.rs` | 1,736 | ~1,302 | ~434 | Medium |
| `pack_installer.rs` | 1,541 | ~1,353 | ~188 | Medium |
| `cron.rs` | 1,533 | ~765 | ~768 | Low |
| `db_migration.rs` | 1,115 | ~298 | ~817 | Low |
| `metering.rs` | 806 | -- | -- | Low |
| `triggers.rs` | 734 | -- | -- | Low |
| `config_reload.rs` | 679 | -- | -- | Low |
| `config.rs` | 555 | -- | -- | Low |
| `pairing.rs` | 510 | -- | -- | Low |
| `approval.rs` | 467 | -- | -- | Low |
| `background.rs` | 457 | -- | -- | Low |
| `wizard.rs` | 438 | -- | -- | Low |
| `registry.rs` | 438 | -- | -- | Low |
| `pack_registry.rs` | 374 | -- | -- | Low |
| `heartbeat.rs` | 357 | -- | -- | Low |
| `whatsapp_gateway.rs` | 344 | -- | -- | Low |
| `auth.rs` | 316 | -- | -- | Low |
| `supervisor.rs` | 227 | -- | -- | Low |
| `auto_reply.rs` | 211 | -- | -- | Low |
| `scheduler.rs` | 191 | -- | -- | Low |
| `db.rs` | 164 | -- | -- | Low |
| `event_bus.rs` | 149 | -- | -- | Low |
| `template_renderer.rs` | 115 | -- | -- | Low |
| `capabilities.rs` | 95 | -- | -- | Low |
| `lib.rs` | 39 | -- | -- | Low |
| `error.rs` | 19 | -- | -- | Low |

---

## 1. `kernel.rs` -- Critical (10,936 lines)

### 1.1 God Object: `OpenFangKernel` struct (64 fields)

The struct at lines 156-282 holds 64 fields spanning unrelated concerns:
agent registry, MCP connections, cron scheduler, A2A agents, browser automation,
TTS engine, OFP networking, WhatsApp gateway PID, delivery tracking, and more.

```rust
pub struct OpenFangKernel {
    pub config: KernelConfig,
    pub registry: AgentRegistry,
    pub capabilities: CapabilityManager,
    pub event_bus: EventBus,
    pub scheduler: AgentScheduler,
    pub memory: Arc<MemorySubstrate>,
    // ... 58 more fields ...
    workflow_dispatch_cancel: CancellationToken,
    looper_runtime_tokens: dashmap::DashMap<String, CancellationToken>,
    looper_runtime_cancel: CancellationToken,
    looper_runtime_registry: Arc<LooperRuntimeRegistry>,
    self_handle: OnceLock<Weak<OpenFangKernel>>,
}
```

**Problem:** Every new feature adds a field here. The struct is un-testable in
isolation because constructing it requires initializing 64 subsystems.

**Recommendation:** Extract field groups into focused facade structs:

| Extracted Struct | Fields to Move | New File |
|---|---|---|
| `DispatchManager` | `workflow_dispatch_tasks`, `workflow_dispatch_tokens`, `workflow_dispatch_cancel` + dispatch methods (lines 4571-4770) | `dispatch_manager.rs` |
| `LooperManager` | `looper_runtime_tasks`, `looper_runtime_tokens`, `looper_runtime_cancel`, `looper_runtime_registry` + looper methods (lines 4833-5090) | `looper_manager.rs` |
| `McpManager` | `mcp_connections`, `mcp_tools`, `effective_mcp_servers` + MCP methods (lines 7617-7900) | `mcp_manager.rs` |
| `DriverResolver` | `default_driver`, `model_catalog`, `default_model_override` + `resolve_driver()` (lines 7488-7614) | `driver_resolver.rs` |
| `ToolResolver` | Tool filtering logic (lines 7947-8091) + `build_skill_summary()` + `build_mcp_summary()` | `tool_resolver.rs` |
| `BackgroundOrchestrator` | `start_background_agents()` (lines 6725-7092) and all its spawned tasks | `background_orchestrator.rs` |
| `ChannelBridge` | `channel_adapters`, `delivery_tracker`, `whatsapp_gateway_pid`, `broadcast` | `channel_bridge.rs` |
| `NetworkManager` | `peer_registry`, `peer_node`, `a2a_external_agents`, `a2a_task_store` | `network_manager.rs` |

### 1.2 Massive `impl OpenFangKernel` block (~8,600 lines)

The single `impl` block (lines 690-9346) contains 150+ methods covering:

- Boot & initialization (~300 lines)
- Agent spawn & lifecycle (~500 lines)
- Message sending (5 variants, ~700 lines)
- LLM agent execution (~500 lines)
- Session management (~400 lines)
- Hand lifecycle (~200 lines)
- Config reload (~100 lines)
- Event/trigger management (~100 lines)
- Workflow dispatch orchestration (~2,200 lines)
- Looper execution (~700 lines)
- HITL resume/answer (~500 lines)
- Background agent startup (~400 lines)
- OFP networking (~100 lines)
- MCP connection management (~300 lines)
- Tool filtering (~200 lines)
- Skill & prompt building (~200 lines)
- Credential resolution (~100 lines)

**Recommendation:** Split into focused `impl` blocks in separate files using
module-level organization:

```
kernel/
  mod.rs            -- OpenFangKernel struct + boot + shutdown + pub re-exports
  agent_lifecycle.rs -- spawn_agent, kill_agent, set_agent_state, set_agent_mode
  messaging.rs       -- send_message* variants, execute_llm_agent*
  session.rs         -- reset_session, create/switch/list sessions, compact
  hands.rs           -- activate_hand, deactivate_hand, persist_hand_state
  dispatch.rs        -- workflow dispatch state machine methods
  looper_ops.rs      -- create/list/get looper runs, control plane operations
  hitl.rs            -- HITL resume, answer, context reconstruction
  tools.rs           -- available_tools, build_skill_summary, build_mcp_summary
  drivers.rs         -- resolve_driver, lookup_provider_url
  background.rs      -- start_background_agents, start_heartbeat_monitor
  mcp.rs             -- connect_mcp_servers, reload_extension_mcps
  credentials.rs     -- resolve_credential, store_credential, remove_credential
```

### 1.3 Duplicated Compaction Logic (3 copies)

The session compaction check is copy-pasted at three locations:

- Lines 2242-2280 (streaming path)
- Lines 2596-2610 (streaming LLM inner path)
- Lines 2840-2873 (non-streaming `execute_llm_agent_with_overrides`)

All three repeat the same pattern:

```rust
use openfang_runtime::compactor::{
    estimate_token_count, needs_compaction as check_compact,
    needs_compaction_by_tokens, CompactionConfig,
};
let config = CompactionConfig::default();
let by_messages = check_compact(&session, &config);
let estimated = estimate_token_count(...);
let by_tokens = needs_compaction_by_tokens(estimated, &config);
// ... quota headroom check ...
```

**Recommendation:** Extract into a single method:

```rust
fn should_compact_session(
    &self,
    agent_id: AgentId,
    session: &Session,
    system_prompt: Option<&str>,
) -> bool
```

### 1.4 Duplicated Agent Lookup Pattern (13 occurrences)

The pattern `self.registry.get(agent_id).ok_or_else(|| ...)` appears 13 times
with identical error construction:

```rust
let entry = self.registry.get(agent_id).ok_or_else(|| {
    KernelError::OpenFang(OpenFangError::AgentNotFound(agent_id.to_string()))
})?;
```

**Recommendation:** Extract a helper:

```rust
fn require_agent(&self, agent_id: AgentId) -> KernelResult<AgentEntry> {
    self.registry.get(agent_id).ok_or_else(|| {
        KernelError::OpenFang(OpenFangError::AgentNotFound(agent_id.to_string()))
    })
}
```

### 1.5 Duplicated Session Fallback Pattern (4 occurrences)

The pattern for loading a session with a fallback empty session appears 4 times
(lines 2233, 2832, 3734, 3824):

```rust
let session = self
    .memory
    .get_session(entry.session_id.clone())
    .map_err(KernelError::OpenFang)?
    .unwrap_or_else(|| openfang_memory::session::Session {
        id: entry.session_id.clone(),
        agent_id,
        messages: Vec::new(),
        context_window_tokens: 0,
        label: None,
    });
```

**Recommendation:** Extract:

```rust
fn load_session_or_empty(
    &self,
    session_id: SessionId,
    agent_id: AgentId,
) -> KernelResult<Session>
```

### 1.6 `#[allow(clippy::too_many_arguments)]` Suppression

Two functions suppress the too-many-arguments lint (lines 2785, 2810):

- `execute_llm_agent` (8 params)
- `execute_llm_agent_with_overrides` (10 params)

**Recommendation:** Use the existing `AgentMessageDispatch` struct (or a new
`LlmExecutionContext`) to bundle parameters.

### 1.7 send_message Proliferation (5 variants)

There are 5 `send_message*` public methods that form a delegation chain:

1. `send_message` -> `send_message_with_handle`
2. `send_message_with_blocks` -> `send_message_with_handle_and_blocks`
3. `send_message_with_handle` -> `send_message_with_handle_and_blocks`
4. `send_message_with_handle_and_blocks` -> `send_message_with_handle_and_blocks_for_session`
5. `send_message_with_handle_and_blocks_for_session` (actual implementation)

**Recommendation:** Reduce to 2 methods:

1. `send_message(dispatch: AgentMessageDispatch)` -- non-streaming
2. `send_message_streaming(dispatch: AgentMessageDispatch)` -- streaming

All callers already have an `AgentMessageDispatch` or can trivially construct one.
Add builder methods to `AgentMessageDispatch` for ergonomic construction.

### 1.8 `DeliveryTracker` Does Not Belong in `kernel.rs`

The `DeliveryTracker` struct (lines 284-388) with its 5 methods is a self-contained
utility that has no dependency on the kernel. It should live in its own file or
in `openfang-channels`.

### 1.9 Free Functions at Module Level

Several free functions at the module top level (lines 391-660) are tightly
coupled to agent initialization but not to the kernel struct:

- `ensure_workspace()` -- file I/O
- `generate_identity_files()` -- file I/O (133 lines of string templates)
- `append_daily_memory_log()` -- file I/O
- `read_identity_file()` -- file I/O
- `gethostname()` -- platform utility

**Recommendation:** Move to a `workspace.rs` module. The identity file templates
(SOUL.md, AGENTS.md, BOOTSTRAP.md, etc.) should be in const/static data, not
inline format strings.

### 1.10 KernelHandle Implementation (lines 8605-9275)

The `KernelHandle` trait implementation is 670 lines of mostly thin delegation.
It belongs in its own file (`kernel_handle_impl.rs`) to reduce kernel.rs size
and isolate the trait-method-to-kernel-method mapping.

### 1.11 PeerHandle Implementation (lines 9276-9345)

Similarly, the `PeerHandle for OpenFangKernel` implementation (70 lines)
should be in its own file.

### 1.12 `start_background_agents` Is a 370-Line Method

The `start_background_agents` method (lines 6725-7092) spawns 10+ `tokio::spawn`
tasks for:
- Hand restoration
- Background agent loops
- Heartbeat monitor
- OFP node
- Provider health probes
- Usage data cleanup
- Workflow checkpoint retention
- Memory consolidation
- MCP connection
- Extension health monitor
- Cron scheduler tick loop
- A2A agent discovery
- WhatsApp gateway

Each spawned task is 20-60 lines of inline async code. This method is both too
long and doing too many unrelated things.

**Recommendation:** Extract each background loop into a named async function,
then have `start_background_agents` call them:

```rust
pub fn start_background_agents(self: &Arc<Self>) {
    self.restore_persisted_hands();
    self.start_agent_background_loops();
    self.start_heartbeat_monitor();
    self.start_ofp_node_if_enabled();
    self.probe_local_providers();
    self.schedule_metering_cleanup();
    self.schedule_checkpoint_retention();
    self.schedule_memory_consolidation();
    self.connect_mcp_if_configured();
    self.start_extension_health_monitor();
    self.start_cron_tick_loop();
    self.discover_a2a_agents_if_configured();
    self.start_whatsapp_gateway_if_configured();
}
```

---

## 2. `workflow.rs` -- Critical (9,402 lines)

### 2.1 Mixed Concerns: Types + Engine + Persistence + Execution

This single file contains:

- Type definitions: `WorkflowId`, `WorkflowRunId`, `Workflow`, `WorkflowStep`,
  `StepAgent`, `StepMode`, `ErrorMode`, `WorkflowRunState`, `WorkflowRun`,
  `StepResult`, and 20+ more structs/enums (lines 1-870)
- `WorkflowDefinitionStore` -- file I/O persistence (lines 365-610)
- `TransitionWriter` -- durable state transitions (lines 921-1900+)
- `HitlRegistry` -- HITL oneshot channel management (lines 827-855)
- `WorkflowEngine` -- the main orchestrator (lines 3700-4875)
- Template rendering integration
- Signal submission logic
- 4,526 lines of tests

**Recommendation:** Split into a `workflow/` module directory:

```
workflow/
  mod.rs              -- pub re-exports
  types.rs            -- WorkflowId, WorkflowRunId, Workflow, WorkflowStep, etc.
  definition_store.rs -- WorkflowDefinitionStore (file I/O persistence)
  transition_writer.rs -- TransitionWriter (durable state machine transitions)
  hitl.rs             -- HitlRegistry, HitlResumeContext, HitlAnswer, etc.
  engine.rs           -- WorkflowEngine orchestrator
  execution.rs        -- execute_steps, execute_run, step dispatch logic
  signal.rs           -- submit_signal, resume_after_signal
```

### 2.2 `TransitionWriter` Is Very Long (~800+ lines)

The `TransitionWriter` has 15+ methods handling every possible state transition:
`record_run_created`, `record_run_started`, `record_step_started`,
`record_step_completed`, `record_step_failed`, `record_step_skipped`,
`record_waiting_for_signal`, `record_signal_consumed`, `record_hitl_requested`,
etc. Each follows a similar pattern: load current state, validate transition,
build next state + checkpoint, persist atomically, sync cache.

The repetitive load-validate-mutate-persist-sync pattern could benefit from
a state machine abstraction.

### 2.3 Test File Size (4,526 lines)

The test module alone is nearly as large as the production code. With 60+ test
functions, it deserves its own `workflow/tests/` directory or at minimum
`workflow/tests.rs`. The test helpers (`mock_resolver`, `seed_running_hitl_context`,
etc.) are reused across tests and should be in a shared test fixture module.

### 2.4 Helper Functions at Module Level

Lines 611-750 contain ~15 small helper functions (`transition_error_to_string`,
`workflow_state_from_status`, `parse_rfc3339_utc`, `normalize_workflow_input_json`,
`cache_input_from_json`, etc.) that exist purely for the TransitionWriter. They
should move with TransitionWriter.

---

## 3. `workflow_compiler.rs` -- Medium (1,997 lines)

### 3.1 File Is Reasonably Sized but Has Two Concerns

The file handles both **validation** and **compilation** of workflow definitions.
These are logically distinct phases.

**Recommendation:** Split into:
- `workflow_compiler/validation.rs` -- schema validation, step validation,
  contract validation
- `workflow_compiler/compilation.rs` -- template compilation, IR generation,
  normalization

### 3.2 Duplicated Validation Helper Pattern

Functions like `validate_required_string`, `validate_optional_string_field`,
`validate_optional_bool_field` etc. are duplicated between `workflow_compiler.rs`
and `trigger_v2.rs` (see Section 5.1).

---

## 4. `looper.rs` -- Medium (1,936 lines)

### 4.1 Clean Design, Reasonable Size

`LooperRuntime` and `LooperRuntimeRegistry` are well-factored. The test module
is proportionally large (935 lines) but well-structured with reusable test
fixtures (`ControlledExecutor`, `ImmediateExecutor`).

### 4.2 Minor: `in_memory_stores()` Test Helper Duplication

The `in_memory_stores()` function appears in both `looper.rs` (line 1033)
and `workflow.rs` (line 734) with nearly identical implementation. Both
construct a `WorkflowStoreSet` from in-memory SQLite connections.

**Recommendation:** Extract a shared test utility in the test support module.

---

## 5. `trigger_v2.rs` -- Medium (1,736 lines)

### 5.1 Validation Helpers Duplicated from `workflow_compiler.rs`

Both files independently define:

- `validate_required_string`
- `collect_schema_validation_issues`
- `severity_rank`
- Various `validate_optional_*_field` patterns

The function signatures differ slightly (different error types) but the logic
is identical.

**Recommendation:** Extract a shared `schema_validation` module parameterized
by error type, or use a trait to abstract the validation issue accumulator.

### 5.2 File Is Reasonably Sized

At 1,736 lines with 434 test lines, this file is within acceptable bounds.
The validation + compilation + runtime engine pattern mirrors
`workflow_compiler.rs` and would benefit from the same structural split
if the crate grows further.

---

## 6. `pack_installer.rs` -- Medium (1,541 lines)

### 6.1 Good Encapsulation

The pack installer is self-contained with its own error type
(`PackInstallerError`) and clean internal types (`ResolvedPackObject`,
`ResolvedPackContent`). No urgent refactoring needed.

### 6.2 Minor: `render()` Method Repetition

The `ResolvedPackObjectContent::render()` method (lines 66-78) repeats the
same `toml::to_string_pretty(x).map_err(|e| Serialization(e.to_string()))`
for 4 variants. A small macro or helper function would reduce this.

---

## 7. `cron.rs` -- Low (1,533 lines)

Well-structured with clean separation between `JobMeta` and `CronScheduler`.
Test coverage is strong (768 lines). No urgent issues.

---

## 8. `db_migration.rs` -- Low (1,115 lines)

Test-heavy (817 lines vs 298 production lines). The production code is clean
and well-organized. The large test module exercises migration ordering and
idempotency thoroughly.

---

## Cross-Cutting Issues

### C1. Tight Coupling Between kernel.rs and workflow.rs

`kernel.rs` directly manipulates workflow internals through the
`WorkflowEngine` and knows about `WorkflowAgentDispatchRequest`,
`WorkflowAgentDispatchOutcome`, `HitlResumeContext`, etc. The dispatch
orchestration logic (lines 5096-5578) in kernel.rs duplicates state machine
transitions that conceptually belong to the workflow engine.

**Recommendation:** The workflow engine should expose a high-level
`dispatch(request) -> outcome` API. The kernel should not know about dispatch
state transitions or HITL oneshot channels.

### C2. Inconsistent Error Handling

- `kernel.rs` uses `KernelResult<T>` (wrapping `KernelError`)
- Workflow dispatch methods return `Result<T, String>`
- The `KernelHandle` trait methods return `Result<T, String>`

The `String` error pattern loses type information and makes error matching
impossible for callers.

**Recommendation:** Define typed errors for dispatch (`DispatchError`) and
workflow execution (`WorkflowExecutionError`). Reserve `String` errors for
the `KernelHandle` trait boundary only (where the caller is the tool runner
and just needs a message).

### C3. Test Helpers Duplicated Across Files

| Helper | Appears In |
|---|---|
| `in_memory_stores()` | `looper.rs`, `workflow.rs` |
| `StaticTextDriver` | `kernel.rs` tests |
| `boot_test_config()` | `kernel.rs` tests |
| `register_test_agent()` | `kernel.rs` tests |
| `sample_dispatch_request()` | `kernel.rs` tests |

These should live in a `#[cfg(test)] mod test_support` module shared via
`pub(crate)` visibility.

### C4. Naming Inconsistencies

- `trigger_v2.rs` vs `triggers.rs` -- two trigger systems coexist. If v1 is
  legacy, it should be marked as such or removed.
- `scheduler.rs` (agent resource tracking) vs `cron.rs` (cron job scheduling)
  -- confusing because both are "schedulers" but do different things.
- `db.rs` (DatabaseManager) vs `db_migration.rs` (migration runner) -- the
  names don't clearly communicate the split.

### C5. Dead Code Indicators

- `StubDriver` (lines 77-89) is a placeholder that returns an error. If no
  providers are configured, this is the driver used. It could live in
  `openfang-runtime` instead.
- `StoredAgentDefinitionFile` (lines 114-119) wraps `AgentDefinition` with
  `#[serde(flatten)]` and adds nothing. Likely a vestige of an older format.
- Several `#[cfg(test)]` variants (`run_legacy_workflow_dispatch_call_inner`,
  `run_arky_workflow_dispatch_call_inner`) are test-only wrappers that differ
  from production only by accepting override parameters. This is the
  "test-only method on production struct" anti-pattern.

---

## Prioritized Refactoring Plan

### Phase 1: Critical Structural Decomposition

| Task | Impact | Effort | Files Affected |
|---|---|---|---|
| **1a.** Convert `kernel.rs` into a `kernel/` module directory | Reduces 10,936-line file to ~2,000 lines | High | kernel.rs -> kernel/ |
| **1b.** Convert `workflow.rs` into a `workflow/` module directory | Reduces 9,402-line file to ~1,200 lines | High | workflow.rs -> workflow/ |
| **1c.** Extract `require_agent()` helper | Eliminates 13 duplicate lookups | Low | kernel.rs |
| **1d.** Extract `should_compact_session()` helper | Eliminates 3 duplicate compaction checks | Low | kernel.rs |
| **1e.** Extract `load_session_or_empty()` helper | Eliminates 4 duplicate session loads | Low | kernel.rs |

### Phase 2: Responsibility Extraction

| Task | Impact | Effort | Files Affected |
|---|---|---|---|
| **2a.** Extract `DispatchManager` struct | Isolates dispatch state machine | Medium | kernel.rs |
| **2b.** Extract `ToolResolver` struct | Isolates 200+ lines of tool filtering | Medium | kernel.rs |
| **2c.** Extract `DriverResolver` struct | Isolates 130 lines of driver creation | Medium | kernel.rs |
| **2d.** Move `DeliveryTracker` to own file | Decouples utility from kernel | Low | kernel.rs |
| **2e.** Move workspace/identity functions to `workspace.rs` | Decouples file I/O from kernel | Low | kernel.rs |
| **2f.** Move `KernelHandle` impl to own file | Reduces kernel.rs by 670 lines | Low | kernel.rs |
| **2g.** Move `PeerHandle` impl to own file | Reduces kernel.rs by 70 lines | Low | kernel.rs |

### Phase 3: Code Quality

| Task | Impact | Effort | Files Affected |
|---|---|---|---|
| **3a.** Consolidate `send_message*` variants | Reduces 5 methods to 2 | Medium | kernel.rs |
| **3b.** Remove `#[allow(clippy::too_many_arguments)]` | Use parameter objects | Low | kernel.rs |
| **3c.** Factor `start_background_agents` into named sub-methods | Readability | Medium | kernel.rs |
| **3d.** Extract shared validation helpers between workflow_compiler and trigger_v2 | DRY | Medium | workflow_compiler.rs, trigger_v2.rs |
| **3e.** Consolidate test helpers into shared test support module | DRY | Low | multiple |
| **3f.** Replace `Result<T, String>` in dispatch methods with typed errors | Type safety | Medium | kernel.rs, workflow.rs |

### Phase 4: Cleanup

| Task | Impact | Effort | Files Affected |
|---|---|---|---|
| **4a.** Evaluate removing `triggers.rs` (v1) if superseded by `trigger_v2.rs` | Dead code | Low | triggers.rs, kernel.rs |
| **4b.** Remove `StoredAgentDefinitionFile` if unused | Dead code | Trivial | kernel.rs |
| **4c.** Remove `#[cfg(test)]` dispatch call inner variants | Anti-pattern | Low | kernel.rs |
| **4d.** Rename `scheduler.rs` to `quota_tracker.rs` for clarity | Naming | Trivial | scheduler.rs, lib.rs |
| **4e.** Rename `db.rs` to `database_manager.rs` for clarity | Naming | Trivial | db.rs, lib.rs |

---

## Risk Assessment

- **Phase 1 (1a, 1b)** carries the highest risk because it touches every
  `use crate::kernel::*` and `use crate::workflow::*` import path across
  the entire codebase. Recommend doing one file at a time with `pub use`
  re-exports to avoid breaking downstream crates.

- **Phases 2-4** are lower risk because they extract code without changing
  behavior. Each extraction can be verified by `make fmt && make lint && make test`.

- None of these changes affect the public API surface of the kernel crate.
  All extracted modules would remain `pub(crate)` or `pub` through `lib.rs`
  re-exports.

---

## Metrics (Current vs Target)

| Metric | Current | Target (Post Phase 1) | Target (Post Phase 3) |
|---|---|---|---|
| Largest file (lines) | 10,936 | ~2,000 | ~1,500 |
| Max struct fields | 64 | 64 (Phase 1 doesn't reduce) | ~30 |
| Methods in largest impl block | 150+ | ~50 | ~30 |
| Duplicated patterns | 20+ | ~5 | 0 |
| `#[allow(clippy::*)]` suppressions | 2 | 0 | 0 |
| Files with >1,000 prod lines | 4 | 2 | 0 |
