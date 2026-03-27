# Runtime Crate Refactoring Analysis

**Crate:** `openfang-runtime`
**Date:** 2026-03-27
**Total source lines:** ~39,500 (excluding duplicate counts from `wc -l` globbing)
**Files analyzed:** 49 `.rs` source files across `src/` and `src/drivers/`

---

## Executive Summary

The `openfang-runtime` crate is the largest in the OpenFang workspace and contains the core agent execution engine, LLM drivers, tool execution, and numerous supporting subsystems. The crate has grown organically and now exhibits several structural problems that impede maintainability:

1. **Three files exceed 3,900 lines** (`agent_loop.rs`, `model_catalog.rs`, `tool_runner.rs`), making them difficult to navigate, review, and test in isolation.
2. **Massive code duplication** between `run_agent_loop` and `run_agent_loop_streaming` (~730 lines copy-pasted with trivial differences).
3. **God functions** with 22+ parameters (`execute_tool`, `run_agent_loop`) that are impossible to extend without touching every call site.
4. **Data-as-code anti-pattern** in `model_catalog.rs` where ~3,350 lines of static model/provider data are hardcoded as Rust struct literals.
5. **MIME type detection logic duplicated 3 times** within `tool_runner.rs` alone.

The refactoring recommendations below are ordered by impact and can be executed incrementally without changing external behavior.

---

## File-by-File Analysis

### 1. `agent_loop.rs` (4,556 lines) -- CRITICAL

| Metric | Value |
|--------|-------|
| Lines | 4,556 |
| Public functions | 2 (`run_agent_loop`, `run_agent_loop_streaming`) |
| Private functions | 12 |
| Test lines | ~1,714 (lines 2842-4556) |
| `#[allow(clippy::too_many_arguments)]` | 2 |
| Max function params | 22 (`run_agent_loop`, `run_agent_loop_streaming`) |

#### Problem 1: CRITICAL -- Near-total duplication between sync and streaming loops

`run_agent_loop` (lines 303-1051, ~748 lines) and `run_agent_loop_streaming` (lines 1285-2031, ~746 lines) are almost identical. The streaming variant adds:
- A `stream_tx: mpsc::Sender<StreamEvent>` parameter
- Calls `stream_with_retry` instead of `call_with_retry`
- Sends `StreamEvent::ToolExecutionResult` and `StreamEvent::PhaseChange` events
- Minor log message differences ("(streaming)" suffix)

Everything else -- session setup, memory recall, message preparation, tool execution loop, error handling, hook firing, NO_REPLY detection, phantom action detection, MaxTokens continuation, context overflow recovery -- is duplicated verbatim.

**Suggested approach:** Extract the shared loop body into a single internal function parameterized by a `LoopMode` enum:

```rust
enum LoopMode<'a> {
    Sync,
    Streaming { tx: &'a mpsc::Sender<StreamEvent> },
}
```

The LLM call dispatches to `call_with_retry` or `stream_with_retry` based on the mode. Tool result notifications are conditionally sent. This would eliminate ~700 lines.

#### Problem 2: CRITICAL -- 22-parameter function signatures

Both `run_agent_loop` and `run_agent_loop_streaming` accept 22 parameters. This is a maintenance hazard: every test must pass `None` for 15+ parameters (visible in lines 3044-3067 where tests pass 21 `None` values).

**Suggested approach:** Introduce an `AgentLoopContext` struct:

```rust
pub struct AgentLoopContext<'a> {
    pub manifest: &'a AgentManifest,
    pub session: &'a mut Session,
    pub memory: &'a MemorySubstrate,
    pub driver: Arc<dyn LlmDriver>,
    pub available_tools: &'a [ToolDefinition],
    pub kernel: Option<Arc<dyn KernelHandle>>,
    // ... capabilities grouped as Option fields
}
```

This also makes the `ToolExecutionContext` reusable (see tool_runner below).

#### Problem 3: HIGH -- `recover_text_tool_calls` is 564 lines (lines 2053-2617)

This single function handles 13 different text-based tool call patterns from various LLM providers. Each pattern is 30-50 lines of similar parsing logic with duplicated deduplication checks.

**Suggested approach:** Split into `agent_loop/text_tool_recovery.rs`:
- Define a `ToolCallPattern` trait or a list of pattern functions
- Each pattern returns `Vec<ToolCall>`, deduplication happens once at the end
- The duplicate-check (`if !calls.iter().any(|c| c.name == ... && c.input == ...)`) appears 12 times and should be a helper

#### Problem 4: HIGH -- `call_with_retry` and `stream_with_retry` are 95% identical

`call_with_retry` (lines 1057-1164, 107 lines) and `stream_with_retry` (lines 1169-1277, 108 lines) differ only in calling `driver.complete()` vs `driver.stream()`. The retry logic, error classification, circuit breaker recording, and error message formatting are identical.

**Suggested approach:** Extract a generic retry wrapper parameterized by the call function.

#### Problem 5: MEDIUM -- Tests are 1,714 lines (37.6% of the file)

Tests should live in a separate file or module for better organization. The test helpers (`test_manifest`, mock drivers) are defined inline.

**Suggested approach:** Move to `agent_loop/tests.rs` or `tests/agent_loop_tests.rs`.

---

### 2. `model_catalog.rs` (4,250 lines) -- CRITICAL

| Metric | Value |
|--------|-------|
| Lines | 4,250 |
| `ModelCatalog` struct methods | 17 |
| `builtin_providers()` output | ~45 providers (~380 lines, 420-803) |
| `builtin_aliases()` output | ~90 aliases (~90 lines, 806-896) |
| `builtin_models()` output | ~130+ models (~3,350 lines, 898-4250) |

#### Problem 1: CRITICAL -- Data-as-code (3,350 lines of struct literals)

The `builtin_models()` function (lines 898-4250) is 3,352 lines of `ModelCatalogEntry { ... }` struct literals. Each model entry is 14-16 lines of repetitive field assignments. This is the single biggest contributor to crate size and compile time.

Similarly, `builtin_providers()` is 383 lines of `ProviderInfo { ... }` literals (lines 420-803).

**Suggested approach:**
1. Move model data to a static JSON or TOML file: `data/models.json` and `data/providers.json`
2. Load at build time via `include_str!` + `serde_json::from_str` in a `LazyLock`
3. Or use a build script (`build.rs`) to compile JSON into a static slice
4. This reduces `model_catalog.rs` from 4,250 to ~400 lines (the `ModelCatalog` struct + methods)

#### Problem 2: HIGH -- Provider defaults duplicated across files

Provider metadata (base URL, env var name, key_required) is declared in three places:
1. `model_catalog.rs` `builtin_providers()` (~380 lines)
2. `drivers/mod.rs` `provider_defaults()` (~250 lines)
3. `openfang-types` crate (base URL constants)

**Suggested approach:** Single source of truth. The `ProviderInfo` from `model_catalog.rs` should be the canonical definition, with `drivers/mod.rs` looking up from the catalog instead of maintaining a parallel mapping.

#### Problem 3: MEDIUM -- `read_codex_credential()` does not belong here

Lines 373-414 contain credential file reading logic that is unrelated to the model catalog. It is only called from `detect_auth()`.

**Suggested approach:** Move to `drivers/mod.rs` or a dedicated `auth.rs` module.

---

### 3. `tool_runner.rs` (3,988 lines) -- CRITICAL

| Metric | Value |
|--------|-------|
| Lines | 3,988 |
| `execute_tool` params | 17 (with `#[allow(clippy::too_many_arguments)]`) |
| Tool definitions | ~45 tools in `builtin_tool_definitions()` (~720 lines) |
| Tool implementations | ~30 `tool_*` functions |
| Image utility functions | 5 (`detect_image_format`, `extract_*_dimensions`, etc.) |

#### Problem 1: CRITICAL -- God function `execute_tool` with 17 parameters

`execute_tool` (lines 99-526) takes 17 parameters and dispatches to ~60 tool names via a massive `match` statement. Adding any new tool or context parameter requires modifying this function's signature and every call site.

**Suggested approach:** Introduce a `ToolContext` struct:

```rust
pub struct ToolContext<'a> {
    pub kernel: Option<&'a Arc<dyn KernelHandle>>,
    pub allowed_tools: Option<&'a [String]>,
    pub caller_agent_id: Option<&'a str>,
    pub skill_registry: Option<&'a SkillRegistry>,
    pub mcp_connections: Option<&'a tokio::sync::Mutex<Vec<McpConnection>>>,
    pub web_ctx: Option<&'a WebToolsContext>,
    pub browser_ctx: Option<&'a BrowserManager>,
    pub workspace_root: Option<&'a Path>,
    // ... etc
}
```

Then: `pub async fn execute_tool(id: &str, name: &str, input: &Value, ctx: &ToolContext<'_>) -> ToolResult`

#### Problem 2: CRITICAL -- `builtin_tool_definitions()` is 720 lines of JSON literals

Lines 529-1249 contain 45 `ToolDefinition { ... }` struct literals with inline `serde_json::json!()` schemas. This is pure data that inflates the file.

**Suggested approach:**
1. Move to `data/tool_definitions.json` loaded via `include_str!`
2. Or split into per-category modules: `tools/filesystem.rs`, `tools/web.rs`, `tools/agent.rs`, etc., each exporting their own definitions and implementations

#### Problem 3: HIGH -- MIME type detection duplicated 3 times

Extension-to-MIME mapping appears at:
- Lines 2277-2298 (`tool_channel_send`, 22 extensions)
- Lines 2743-2751 (`tool_media_describe`, 7 extensions)
- Lines 2788-2796 (`tool_media_transcribe`, 6 extensions)
- Lines 2963-2971 (`tool_speech_to_text`, 6 extensions)

**Suggested approach:** Extract a single `fn mime_from_extension(ext: &str) -> &'static str` utility.

#### Problem 4: HIGH -- Browser tool boilerplate (10 arms, identical error handling)

Lines 347-446 contain 10 browser tool match arms with identical boilerplate:
```rust
"browser_X" => match browser_ctx {
    Some(mgr) => {
        let aid = caller_agent_id.unwrap_or("default");
        crate::browser::tool_browser_X(input, mgr, aid).await
    }
    None => Err("Browser tools not available. Ensure Chrome/Chromium is installed.".to_string()),
},
```

**Suggested approach:** Extract a `dispatch_browser_tool` helper:
```rust
fn dispatch_browser_tool(name: &str, input: &Value, ctx: &ToolContext) -> Result<String, String> { ... }
```

#### Problem 5: HIGH -- SRP violation: file contains 7 distinct domains

The file mixes filesystem tools, web tools, shell execution, inter-agent tools, knowledge graph tools, scheduling tools, media tools, browser dispatch, image analysis (with raw byte parsing), location lookup, Docker sandbox, process management, canvas/HTML sanitization, TTS/STT, A2A, and channel send. These are at least 7 distinct domains.

**Suggested approach:** Split into a `tools/` directory:

```
tools/
  mod.rs           -- execute_tool dispatch + ToolContext
  definitions.rs   -- builtin_tool_definitions()
  filesystem.rs    -- file_read, file_write, file_list, apply_patch
  web.rs           -- web_fetch, web_search (legacy)
  shell.rs         -- shell_exec
  agent.rs         -- agent_send, agent_spawn, agent_list, agent_kill, agent_find
  memory.rs        -- memory_store, memory_recall
  collaboration.rs -- task_post, task_claim, task_complete, task_list, event_publish
  scheduling.rs    -- schedule_*, cron_*, parse_schedule_to_cron
  knowledge.rs     -- knowledge_add_entity, knowledge_add_relation, knowledge_query
  media.rs         -- media_describe, media_transcribe, image_analyze, image_generate
  tts.rs           -- text_to_speech, speech_to_text
  browser.rs       -- browser dispatch wrapper
  docker.rs        -- docker_exec
  process.rs       -- process_start, process_poll, process_write, process_kill, process_list
  channel.rs       -- channel_send
  a2a.rs           -- a2a_discover, a2a_send
  canvas.rs        -- canvas_present, sanitize_canvas_html
  location.rs      -- location_get, system_time
  utils.rs         -- validate_path, resolve_file_path, mime_from_extension, format_file_size, image format detection
```

#### Problem 6: MEDIUM -- Image format detection is reimplemented

Lines 2552-2636 contain manual PNG/JPEG/GIF/WebP/BMP magic byte parsing and JPEG SOF marker scanning. This is fragile and could use the `image` crate or `infer` crate for reliable detection.

---

### 4. `compactor.rs` (1,412 lines) -- LOW

| Metric | Value |
|--------|-------|
| Lines | 1,412 |
| Public types | 5 (`CompactionConfig`, `CompactionResult`, `ContextPressure`, `ContextBreakdown`, `ContextReport`) |

**Assessment:** Well-structured. Has clear single responsibility (session compaction). The `estimate_token_count` function is simple but adequate. No urgent issues.

**Minor suggestion:** The `ContextReport` / `ContextPressure` types (lines 136-257) are reporting concerns, not compaction logic. Could be split into `context_report.rs` if the file grows further.

---

### 5. `browser.rs` (1,362 lines) -- MEDIUM

| Metric | Value |
|--------|-------|
| Lines | 1,362 |
| CDP connection | ~100 lines |
| Browser session management | ~200 lines |
| Tool handler functions | ~10 functions |

**Assessment:** Reasonably well-structured for a CDP integration. Has clear layering: `CdpConnection` (transport) -> `BrowserSession` (page operations) -> `BrowserManager` (session lifecycle) -> `tool_browser_*` (tool handlers).

**Issue:** `#[allow(dead_code)]` on `MAX_CONTENT_CHARS` (line 37) suggests unused code.

---

### 6. `drivers/openai.rs` (1,836 lines) -- MEDIUM

| Metric | Value |
|--------|-------|
| Lines | 1,836 |
| Request/response types | ~15 serde structs |
| `complete()` method | ~250 lines |
| `stream()` method | ~350 lines |

**Problem 1: MEDIUM -- Message conversion logic is complex**

The `convert_messages` function handles multiple content block types and provider-specific quirks (Moonshot `reasoning_content`, image base64). This is the kind of logic that benefits from extraction.

**Problem 2: MEDIUM -- `complete()` and `stream()` share request building but diverge on response parsing**

The request construction is duplicated. Could extract a `build_request` helper.

---

### 7. `drivers/gemini.rs` (1,727 lines) -- MEDIUM

| Metric | Value |
|--------|-------|
| Lines | 1,727 |
| Serde types | ~15 structs/enums |
| `complete()` method | ~200 lines |
| `stream()` method | ~300 lines |

**Assessment:** Similar structure to `openai.rs`. The `GeminiPart` enum with `thoughtSignature` handling is complex but necessary.

**Issue:** Both `openai.rs` and `gemini.rs` implement very similar streaming SSE parsing with `futures::StreamExt`. A shared SSE-to-event adapter could reduce duplication.

---

### 8. `drivers/mod.rs` (854 lines) -- HIGH

| Metric | Value |
|--------|-------|
| Lines | 854 |
| Provider matching | ~250 lines in `provider_defaults()` |
| Driver construction | ~200 lines in `create_driver()` |

**Problem 1: HIGH -- `provider_defaults()` duplicates `model_catalog.rs` provider data**

As noted in the model_catalog analysis, provider base URLs and env var names are maintained in two places.

**Problem 2: MEDIUM -- `create_driver()` is a 200-line match on provider name**

This could be simplified with a registry pattern.

---

### 9. `session_repair.rs` (1,234 lines) -- LOW

Well-factored module with clear responsibility. The `RepairStats` struct provides good observability. No urgent refactoring needed.

---

### 10. `llm_errors.rs` (1,047 lines) -- LOW

Classification logic with comprehensive pattern matching. Structure is sound.

---

### 11. Other files (each under 1,000 lines)

| File | Lines | Assessment |
|------|-------|------------|
| `prompt_builder.rs` | 973 | OK -- single responsibility |
| `loop_guard.rs` | 949 | OK -- well-isolated circuit breaker |
| `subprocess_sandbox.rs` | 905 | OK -- security-critical, keep isolated |
| `mcp.rs` | 787 | OK |
| `apply_patch.rs` | 780 | OK |
| `a2a.rs` | 754 | OK |
| `drivers/claude_code.rs` | 723 | OK |
| `auth_cooldown.rs` | 721 | OK |
| `drivers/anthropic.rs` | 696 | OK |
| `host_functions.rs` | 668 | OK |
| `docker_sandbox.rs` | 635 | OK |
| `sandbox.rs` | 607 | OK |
| `media_understanding.rs` | 595 | OK |
| `drivers/qwen_code.rs` | 593 | OK |
| `retry.rs` | 513 | OK |
| `tool_policy.rs` | 478 | OK |
| `web_search.rs` | 467 | OK |
| `web_content.rs` | 449 | OK |
| `think_filter.rs` | 445 | OK |
| `graceful_shutdown.rs` | 442 | OK |
| `embedding.rs` | 426 | OK |
| `python_runtime.rs` | 425 | OK |
| `audit.rs` | 422 | OK |
| `workspace_context.rs` | 415 | OK |
| `routing.rs` | 376 | OK |
| `web_fetch.rs` | 375 | OK |
| `provider_health.rs` | 366 | OK |
| `llm_driver.rs` | 357 | OK -- clean trait definition |
| `shell_bleed.rs` | 354 | OK |
| `context_budget.rs` | 354 | OK |
| `process_manager.rs` | 333 | OK |
| `tts.rs` | 317 | OK |
| `drivers/copilot.rs` | 316 | OK |
| `context_overflow.rs` | 267 | OK |
| `kernel_handle.rs` | 255 | OK -- clean trait definition |
| `reply_directives.rs` | 250 | OK |
| `drivers/fallback.rs` | 250 | OK |
| `hooks.rs` | 242 | OK |
| `link_understanding.rs` | 240 | OK |
| `command_lane.rs` | 223 | OK |
| `image_gen.rs` | 221 | OK |
| `mcp_server.rs` | 186 | OK |
| `copilot_oauth.rs` | 149 | OK |
| `workspace_sandbox.rs` | 148 | OK |
| `web_cache.rs` | 145 | OK |
| `str_utils.rs` | 70 | OK |
| `lib.rs` | 61 | OK |
| `test_support.rs` | 38 | OK |

---

## Cross-Cutting Issues

### Issue A: CRITICAL -- `KernelHandle` trait has 30+ methods

`kernel_handle.rs` defines a trait with 30+ methods (many with default implementations). This is a "god trait" that violates Interface Segregation. Adding any new kernel-backed tool requires extending this trait, which touches the kernel implementation.

**Suggested approach:** Split into focused sub-traits:
- `AgentOps` -- spawn, send, list, kill, find
- `MemoryOps` -- store, recall
- `TaskOps` -- post, claim, complete, list
- `ChannelOps` -- send_message, send_media, send_file_data
- `KnowledgeOps` -- add_entity, add_relation, query
- `CronOps` -- create, list, cancel
- `HandOps` -- list, activate, status, deactivate
- `ApprovalOps` -- requires_approval, request_approval
- `A2aOps` -- list_a2a_agents, get_a2a_agent_url

Compose via `trait KernelHandle: AgentOps + MemoryOps + TaskOps + ...`

### Issue B: HIGH -- Flat module structure with 49 files

`lib.rs` re-exports 48 `pub mod` declarations. There is no sub-module organization. The crate would benefit from grouping:

```
src/
  lib.rs
  agent_loop/
    mod.rs
    text_tool_recovery.rs
    retry.rs
    tests.rs
  drivers/
    mod.rs, anthropic.rs, gemini.rs, openai.rs, ...
  tools/
    mod.rs, filesystem.rs, web.rs, shell.rs, agent.rs, ...
  model_catalog/
    mod.rs       -- ModelCatalog struct + methods
    data.rs      -- loaded from JSON
  llm/
    driver.rs, errors.rs, ...
```

### Issue C: MEDIUM -- Inconsistent error types in tool implementations

All `tool_*` functions return `Result<String, String>`. The error type is always a formatted string. While this is simple, it loses structured error information. Consider using a `ToolError` enum with `Display` for human-readable messages.

### Issue D: MEDIUM -- `reqwest::Client` constructed per-call in legacy tools

`tool_web_fetch_legacy`, `tool_web_search_legacy`, and `tool_location_get` each construct a new `reqwest::Client` per invocation. `reqwest::Client` is designed to be reused (connection pooling).

**Suggested approach:** Share a client via `ToolContext` or a `LazyLock` static.

---

## Prioritized Refactoring Plan

### Phase 1: High-Impact, Low-Risk (estimated: 2-3 days)

| # | File | Change | Priority | Lines saved |
|---|------|--------|----------|-------------|
| 1 | `agent_loop.rs` | Unify `run_agent_loop` and `run_agent_loop_streaming` via `LoopMode` | CRITICAL | ~700 |
| 2 | `agent_loop.rs` | Unify `call_with_retry` and `stream_with_retry` | HIGH | ~100 |
| 3 | `tool_runner.rs` | Extract `ToolContext` struct to replace 17 params | CRITICAL | ~0 (structural) |
| 4 | `agent_loop.rs` | Extract `AgentLoopContext` struct to replace 22 params | CRITICAL | ~0 (structural) |
| 5 | `tool_runner.rs` | Extract `mime_from_extension()` utility | HIGH | ~50 |
| 6 | `tool_runner.rs` | Extract browser tool dispatch helper | HIGH | ~80 |

### Phase 2: Data Extraction (estimated: 1-2 days)

| # | File | Change | Priority | Lines saved |
|---|------|--------|----------|-------------|
| 7 | `model_catalog.rs` | Move model data to JSON file | CRITICAL | ~3,350 |
| 8 | `model_catalog.rs` | Move provider data to JSON file | HIGH | ~380 |
| 9 | `tool_runner.rs` | Move tool definitions to JSON file | HIGH | ~720 |
| 10 | `model_catalog.rs` | Move `read_codex_credential` to auth module | MEDIUM | ~40 |

### Phase 3: Module Splitting (estimated: 3-4 days)

| # | File | Change | Priority | Lines saved |
|---|------|--------|----------|-------------|
| 11 | `tool_runner.rs` | Split into `tools/` sub-modules | CRITICAL | 0 (organizational) |
| 12 | `agent_loop.rs` | Extract `text_tool_recovery.rs` | HIGH | ~564 lines to new file |
| 13 | `agent_loop.rs` | Move tests to separate file | MEDIUM | ~1,714 lines to new file |
| 14 | `drivers/mod.rs` | Unify provider defaults with model catalog | HIGH | ~250 |

### Phase 4: Trait Refinement (estimated: 2-3 days)

| # | File | Change | Priority | Lines saved |
|---|------|--------|----------|-------------|
| 15 | `kernel_handle.rs` | Split into focused sub-traits | HIGH | 0 (structural) |
| 16 | `lib.rs` | Reorganize flat module structure | MEDIUM | 0 (organizational) |

---

## Expected Outcomes After Refactoring

| Metric | Before | After (estimated) |
|--------|--------|-------------------|
| `agent_loop.rs` lines | 4,556 | ~1,200 (core loop only) |
| `model_catalog.rs` lines | 4,250 | ~400 (struct + methods) |
| `tool_runner.rs` lines | 3,988 | ~300 (dispatch + context) |
| Files > 1,000 lines | 8 | 3-4 |
| `#[allow(clippy::too_many_arguments)]` | 4 | 0 |
| MIME detection copies | 3 | 1 |
| Duplicated loop code | ~700 lines | 0 |
| Provider data copies | 3 | 1 |

---

## Risks and Considerations

1. **Test coverage:** The existing tests in `agent_loop.rs` are integration tests that construct mock drivers. They must be preserved and should continue to pass after refactoring. The `AgentLoopContext` struct makes test setup cleaner.

2. **External API stability:** `run_agent_loop` and `run_agent_loop_streaming` are called from `openfang-kernel`. Changing their signatures requires updating the kernel. The `AgentLoopContext` approach provides a migration path: add the new API alongside the old one, migrate callers, then remove the old API.

3. **Compile time impact:** Moving 3,350 lines of model data from Rust struct literals to JSON loaded at runtime should noticeably reduce compile times for the runtime crate.

4. **Incremental execution:** Each phase is independent. Phase 1 can be done first for immediate wins, while Phases 2-4 can follow in any order.
