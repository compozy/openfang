## markdown

## status: pending

<task_context>
<domain>providers/bridge/runtime</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task4,task10,task11,task12</dependencies>
</task_context>

# Task 4R: Arky Provider-to-LlmDriver Bridge (ArkyDriverBridge)

## Problem Statement

Tasks 4, 10, 11, and 12 built the Arky provider compile pipeline:

```
AgentDefinition (TOML)
    -> compile_provider_binding()       [task 11]
ProviderBinding (IR)
    -> build_provider_config()          [task 12]
CompozyProviderConfig (typed config)
    -> ???                              [THIS TASK]
Arc<dyn LlmDriver> (what agent loop needs)
```

The pipeline stops at `CompozyProviderConfig`. There is no code that:
1. Instantiates a live Arky `Provider` from the typed config
2. Wraps that `Provider` behind the `LlmDriver` trait consumed by the agent loop

Without this bridge, Compozy-defined agents compile but **cannot execute**. This
blocks tasks 20-22 (agent CRUD, runtime, messages/SSE).

### Two Separate Type Systems

| Concept | OpenFang (`openfang-runtime`) | Arky (`arky-provider`) |
|---------|-------------------------------|------------------------|
| Driver trait | `LlmDriver` | `Provider` |
| Request | `CompletionRequest` | `ProviderRequest` |
| Response | `CompletionResponse` | `GenerateResponse` |
| Stream event | `StreamEvent` (via mpsc) | `AgentEvent` (via `Stream`) |
| Error | `LlmError` | `ProviderError` |
| Message | `openfang_types::message::Message` | `arky_protocol::Message` |
| Tool def | `openfang_types::tool::ToolDefinition` | `arky_protocol::ToolDefinition` |

These types are structurally similar but live in different crates with different
field names, enums, and semantics. The bridge must convert between them faithfully.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- The bridge must live in `crates/openfang-provider-binding/` since that crate already owns the compile-to-runtime boundary and depends on both `arky-provider` and `arky-protocol`.
- `ArkyDriverBridge` must implement `LlmDriver` (from `openfang-runtime`) by wrapping an `Arc<dyn Provider>` (from `arky-provider`).
- A factory function `binding_to_driver()` must accept a `ProviderBinding` + install-layer `ProviderConfig` and return `Arc<dyn LlmDriver>`, completing the full compile-to-runtime pipeline.
- Type conversions must be explicit, tested, and non-lossy for all fields that the agent loop actually uses (messages, tool calls, stop reason, token usage, thinking blocks).
- The existing `create_driver()` path in `openfang-runtime/src/drivers/mod.rs` must remain untouched — this bridge is additive, not a replacement.
- `openfang-provider-binding` must add `openfang-runtime` (for the `LlmDriver` trait) and `openfang-types` (for message/tool types) as dependencies. Verify no circular dependency with `./scripts/check-deps.sh`.
</requirements>

## Subtasks

### 4R.1 — Type Conversion Module: `convert.rs`

Create `crates/openfang-provider-binding/src/convert.rs` with bidirectional
conversion functions between the two type systems.

#### 4R.1.1 — `CompletionRequest` -> `ProviderRequest`

```rust
pub fn completion_request_to_provider(
    request: &CompletionRequest,
    model_ref: &ModelRef,
    session: SessionRef,
    turn: TurnContext,
) -> ProviderRequest
```

Field mapping:

| CompletionRequest field | ProviderRequest target | Conversion |
|------------------------|----------------------|------------|
| `model: String` | `model: ModelRef` | Use passed `model_ref` (has provider_id from binding) |
| `messages: Vec<of::Message>` | `messages: Vec<arky::Message>` | Convert each message (see 4R.1.2) |
| `tools: Vec<of::ToolDefinition>` | `tools: ToolContext` | Wrap in `ToolContext::new().with_definitions(converted)` |
| `max_tokens: u32` | `settings.max_tokens` | `Some(value)` as `Option<u32>` |
| `temperature: f32` | `settings.temperature` | `Some(value as f64)` |
| `system: Option<String>` | Prepend to `messages` | Insert `Message::system(text)` at index 0 |
| `thinking: Option<ThinkingConfig>` | `settings.reasoning_effort` | Map budget_tokens thresholds to `ReasoningEffort` enum |
| N/A | `session: SessionRef` | Pass through from caller |
| N/A | `turn: TurnContext` | Pass through from caller |

#### 4R.1.2 — Message Conversion

```rust
pub fn of_message_to_arky(msg: &openfang_types::message::Message) -> arky_protocol::Message
pub fn arky_message_to_of(msg: &arky_protocol::Message) -> openfang_types::message::Message
```

ContentBlock mapping:

| OpenFang ContentBlock | Arky ContentBlock | Notes |
|-----------------------|-------------------|-------|
| `Text { text, provider_metadata }` | `Text(String)` | Drop `provider_metadata` going to Arky; set `None` coming back |
| `Thinking { thinking, provider_metadata }` | `Thinking { thinking }` | Drop metadata going; set `None` coming back |
| `ToolUse { id, name, input, provider_metadata }` | `ToolUse { id, name, input }` | Drop metadata |
| `ToolResult { tool_use_id, content, is_error }` | `ToolResult { tool_use_id, content, is_error }` | Direct map |
| `Image { source, media_type, provider_metadata }` | `Image { data, media_type }` | Map source bytes |

Role mapping: `System`, `User`, `Assistant` map 1:1. Arky has additional `Tool`
role — map to `User` with tool result content when converting back.

#### 4R.1.3 — ToolDefinition Conversion

```rust
pub fn of_tool_to_arky(tool: &openfang_types::tool::ToolDefinition) -> arky_protocol::ToolDefinition
```

Fields are structurally identical (`name`, `description`, `input_schema: Value`).
Direct field copy.

#### 4R.1.4 — `GenerateResponse` -> `CompletionResponse`

```rust
pub fn generate_response_to_completion(
    response: GenerateResponse,
) -> Result<CompletionResponse, BridgeError>
```

| GenerateResponse field | CompletionResponse target | Conversion |
|----------------------|-------------------------|------------|
| `message: Message` | `content: Vec<ContentBlock>` | Convert message.content blocks via `arky_message_to_of` |
| `message: Message` | `tool_calls: Vec<ToolCall>` | Extract `ToolUse` blocks from message.content |
| `finish_reason: Option<FinishReason>` | `stop_reason: StopReason` | Map: `Stop->EndTurn`, `Length->MaxTokens`, `ToolUse->ToolUse`, `ContentFilter->ContentFilter`, `Error->Error` |
| `usage: Option<Usage>` | `usage: TokenUsage` | Extract `input_tokens` + `output_tokens`; default to 0 if None |

#### 4R.1.5 — `AgentEvent` -> `StreamEvent`

```rust
pub fn agent_event_to_stream_event(event: &AgentEvent) -> Option<StreamEvent>
```

| AgentEvent variant | StreamEvent | Notes |
|-------------------|-------------|-------|
| `MessageUpdate { delta: StreamDelta::Text(t) }` | `TextDelta { text: t }` | Text chunk |
| `MessageUpdate { delta: StreamDelta::ToolUse { id, name, .. } }` | `ToolUseStart { id, name }` | Tool start |
| `MessageUpdate { delta: StreamDelta::ToolInput(t) }` | `ToolInputDelta { text: t }` | Incremental JSON |
| `ToolExecutionEnd { id, name, output, is_error }` | `ToolExecutionResult { name, result_preview, is_error }` | Tool done |
| `TurnEnd { message, usage, .. }` | `ContentComplete { stop_reason, usage }` | Final event |
| `MessageStart`, `MessageEnd` | Collect terminal state, don't emit | Used for response synthesis |
| `AgentStart`, `AgentEnd`, `TurnStart`, `Custom` | `None` (skip) | Not relevant for LlmDriver consumers |

Events that return `None` are silently skipped — they carry metadata useful for
the Arky runtime but not for the OpenFang agent loop.

#### 4R.1.6 — Error Conversion

```rust
pub fn provider_error_to_llm(err: ProviderError) -> LlmError
```

| ProviderError variant | LlmError target |
|-----------------------|----------------|
| `AuthFailed { message }` | `AuthenticationFailed(message)` |
| `RateLimited { retry_after, .. }` | `RateLimited { retry_after_ms }` |
| `ProtocolViolation { message, .. }` | `Parse(message)` |
| `ProcessCrashed { stderr, .. }` | `Http(format!("provider process crashed: {stderr}"))` |
| `BinaryNotFound { binary, .. }` | `MissingApiKey(format!("provider binary not found: {binary}"))` |
| `StreamInterrupted { message, .. }` | `Http(format!("stream interrupted: {message}"))` |
| `NotFound { provider_id }` | `ModelNotFound(provider_id)` |

---

### 4R.2 — Provider Instantiation: `instantiate.rs`

Create `crates/openfang-provider-binding/src/instantiate.rs` with a function
that takes a compiled `CompozyProviderConfig` and returns a live provider.

```rust
pub fn instantiate_provider(
    config: CompozyProviderConfig,
) -> Result<Arc<dyn Provider>, AdapterError>
```

Implementation:

```rust
match config {
    CompozyProviderConfig::Codex(cfg) => {
        Ok(Arc::new(CodexProvider::with_config(cfg)))
    }
    CompozyProviderConfig::ClaudeCode(cfg) => {
        Ok(Arc::new(ClaudeCodeProvider::with_config(cfg)))
    }
    CompozyProviderConfig::ClaudeCompatible { kind, profile } => {
        // ClaudeCodeProvider supports profiles for all compatible wrappers
        Ok(Arc::new(ClaudeCodeProvider::with_profile(kind, profile)))
    }
}
```

Verify the exact constructor signatures by reading:
- `crates/arky-codex/src/provider.rs` — `CodexProvider::with_config()`
- `crates/arky-claude-code/src/provider.rs` — `ClaudeCodeProvider::new()` / `with_config()` / `with_profile()`

If `with_config()` or `with_profile()` don't exist yet, add minimal constructors
that accept the typed config structs produced by task 12's adapter layer. Do NOT
add complex logic — just store the config for use when `stream()` is called.

---

### 4R.3 — The Bridge: `bridge.rs`

Create `crates/openfang-provider-binding/src/bridge.rs` with the core adapter.

```rust
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use async_trait::async_trait;
use arky_provider::Provider;
use arky_protocol::{SessionRef, TurnContext, TurnId, ModelRef};
use openfang_runtime::llm_driver::{
    CompletionRequest, CompletionResponse, LlmDriver, LlmError, StreamEvent,
};
use tokio::sync::mpsc;
use futures::StreamExt;

use crate::convert;
use crate::ProviderBinding;

/// Wraps an Arky `Provider` as an OpenFang `LlmDriver`.
///
/// This bridge allows the OpenFang agent loop to consume Arky providers
/// (Claude Code, Codex, claude-compatible wrappers) without any changes
/// to the agent loop itself.
pub struct ArkyDriverBridge {
    provider: Arc<dyn Provider>,
    model_ref: ModelRef,
    turn_sequence: AtomicU64,
}

impl ArkyDriverBridge {
    pub fn new(provider: Arc<dyn Provider>, model_ref: ModelRef) -> Self {
        Self {
            provider,
            model_ref,
            turn_sequence: AtomicU64::new(0),
        }
    }

    fn next_turn(&self) -> TurnContext {
        let seq = self.turn_sequence.fetch_add(1, Ordering::Relaxed);
        TurnContext::new(TurnId::new(), seq)
    }
}

#[async_trait]
impl LlmDriver for ArkyDriverBridge {
    async fn complete(
        &self,
        request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
        let session = SessionRef::new(None);
        let turn = self.next_turn();
        let arky_request = convert::completion_request_to_provider(
            &request,
            &self.model_ref,
            session,
            turn,
        );

        let response = self.provider
            .generate(arky_request)
            .await
            .map_err(convert::provider_error_to_llm)?;

        convert::generate_response_to_completion(response)
            .map_err(|e| LlmError::Parse(e.to_string()))
    }

    async fn stream(
        &self,
        request: CompletionRequest,
        tx: mpsc::Sender<StreamEvent>,
    ) -> Result<CompletionResponse, LlmError> {
        let session = SessionRef::new(None);
        let turn = self.next_turn();
        let arky_request = convert::completion_request_to_provider(
            &request,
            &self.model_ref,
            session.clone(),
            turn.clone(),
        );

        let mut event_stream = self.provider
            .stream(arky_request)
            .await
            .map_err(convert::provider_error_to_llm)?;

        // Collect terminal state while streaming events
        let mut terminal_message: Option<arky_protocol::Message> = None;
        let mut finish_reason = None;
        let mut usage = None;

        while let Some(item) = event_stream.next().await {
            let event = item.map_err(convert::provider_error_to_llm)?;

            // Collect terminal state from message events
            convert::collect_terminal_state(
                &event,
                &mut terminal_message,
                &mut finish_reason,
                &mut usage,
            );

            // Convert and forward streamable events
            if let Some(stream_event) = convert::agent_event_to_stream_event(&event) {
                let _ = tx.send(stream_event).await;
            }
        }

        // Synthesize CompletionResponse from collected state
        let message = terminal_message.ok_or_else(|| {
            LlmError::Parse(
                "provider stream ended without terminal message".to_string()
            )
        })?;

        convert::synthesize_completion_response(message, finish_reason, usage)
            .map_err(|e| LlmError::Parse(e.to_string()))
    }
}
```

---

### 4R.4 — End-to-End Factory: `binding_to_driver()`

Add to `crates/openfang-provider-binding/src/lib.rs`:

```rust
/// Completes the Compozy compile-to-runtime pipeline.
///
/// Takes a compiled ProviderBinding (from task 11) + install-layer config
/// (from workspace) and returns a live LlmDriver ready for the agent loop.
pub fn binding_to_driver(
    binding: &ProviderBinding,
    install: &ProviderConfig,
) -> Result<Arc<dyn LlmDriver>, BridgeError>
```

Implementation:
1. Call `build_provider_config(binding, install)` (existing, from task 12)
2. Call `instantiate_provider(config)` (new, from 4R.2)
3. Wrap in `ArkyDriverBridge::new(provider, binding.model.clone())`
4. Return `Arc::new(bridge)`

Define `BridgeError` as a thin enum that unifies `CompileError`, `AdapterError`,
and `InstantiateError` so callers get one error type for the whole pipeline.

---

### 4R.5 — Wire into `lib.rs` exports

Update `crates/openfang-provider-binding/src/lib.rs`:
- Add `mod bridge;`, `mod convert;`, `mod instantiate;`
- Re-export: `ArkyDriverBridge`, `binding_to_driver`, `BridgeError`
- Re-export conversion functions for testing/debugging

---

### 4R.6 — Add Dependencies

Add to `crates/openfang-provider-binding/Cargo.toml`:

```toml
openfang-runtime = { workspace = true }
openfang-types = { workspace = true }
futures = { workspace = true }
tokio = { workspace = true, features = ["sync"] }
```

Run `./scripts/check-deps.sh` to verify no circular dependencies. The dependency
graph should be:

```
openfang-provider-binding
  -> arky-provider (Provider trait)
  -> arky-protocol (Arky types)
  -> arky-config (ProviderConfig)
  -> arky-codex (CodexProvider)
  -> arky-claude-code (ClaudeCodeProvider)
  -> openfang-runtime (LlmDriver trait)   <-- NEW
  -> openfang-types (OpenFang types)      <-- NEW
```

Verify that `openfang-runtime` does NOT depend on `openfang-provider-binding`
(it shouldn't — the bridge is one-directional).

---

### 4R.7 — Tests

#### Unit Tests for `convert.rs`

```
convert_should_map_user_text_message_to_arky_format
convert_should_map_assistant_message_with_tool_calls
convert_should_map_system_prompt_as_first_message
convert_should_map_tool_result_message_round_trip
convert_should_map_thinking_block_preserving_content
convert_should_map_image_block_with_media_type
convert_should_map_tool_definitions_preserving_schema
convert_should_map_completion_request_with_all_fields
convert_should_map_generate_response_extracting_tool_calls
convert_should_map_finish_reason_stop_to_end_turn
convert_should_map_finish_reason_tool_use
convert_should_map_finish_reason_length_to_max_tokens
convert_should_map_usage_with_zero_defaults_when_none
convert_should_map_agent_event_text_delta
convert_should_map_agent_event_tool_execution_end
convert_should_map_agent_event_turn_end_to_content_complete
convert_should_skip_agent_start_events
convert_should_skip_custom_events
convert_should_map_provider_error_auth_to_authentication_failed
convert_should_map_provider_error_rate_limited_with_retry_ms
convert_should_map_provider_error_process_crashed_to_http
```

#### Unit Tests for `instantiate.rs`

```
instantiate_should_create_codex_provider_from_config
instantiate_should_create_claude_code_provider_from_config
instantiate_should_create_claude_compatible_provider_from_config
```

#### Unit Tests for `bridge.rs`

```
bridge_complete_should_convert_request_and_response
bridge_complete_should_propagate_provider_errors
bridge_stream_should_forward_text_deltas_via_channel
bridge_stream_should_forward_tool_events_via_channel
bridge_stream_should_emit_content_complete_at_end
bridge_stream_should_synthesize_response_from_terminal_message
bridge_stream_should_return_error_when_no_terminal_message
bridge_turn_sequence_should_increment_across_calls
```

#### Integration Tests for `binding_to_driver()`

```
binding_to_driver_should_produce_working_llm_driver_for_codex
binding_to_driver_should_produce_working_llm_driver_for_claude_code
binding_to_driver_should_fail_with_unknown_driver
binding_to_driver_should_fail_with_missing_behavior_config
```

Use mock/fake providers implementing `Provider` trait for all bridge tests.
Do NOT require live LLM API access in unit tests.

---

## Implementation Details

### Crate to Modify

All changes go in `crates/openfang-provider-binding/`. New files:

```
crates/openfang-provider-binding/src/
  lib.rs          (add mod declarations + re-exports)
  adapter.rs      (existing - no changes)
  bridge.rs       (NEW - ArkyDriverBridge)
  convert.rs      (NEW - type conversion functions)
  instantiate.rs  (NEW - CompozyProviderConfig -> Arc<dyn Provider>)
```

### Key Types to Read Before Starting

Before writing any code, read these source files completely:

| File | What to understand |
|------|-------------------|
| `crates/arky-protocol/src/message.rs` | Arky `Message`, `ContentBlock`, `Role` definitions |
| `crates/arky-protocol/src/event.rs` | `AgentEvent` enum variants and `StreamDelta` |
| `crates/arky-protocol/src/types.rs` | `FinishReason`, `Usage`, `SessionRef`, `TurnContext` |
| `crates/arky-provider/src/traits.rs` | `Provider` trait: `stream()`, `generate()` |
| `crates/arky-provider/src/error.rs` | `ProviderError` variants |
| `crates/openfang-runtime/src/llm_driver.rs` | `LlmDriver`, `CompletionRequest/Response`, `StreamEvent` |
| `crates/openfang-types/src/message.rs` | OpenFang `Message`, `ContentBlock`, `Role` |
| `crates/openfang-types/src/tool.rs` | OpenFang `ToolDefinition`, `ToolCall` |
| `crates/arky-codex/src/provider.rs` | `CodexProvider` constructors |
| `crates/arky-claude-code/src/provider.rs` | `ClaudeCodeProvider` constructors |

### Session/Turn Context Strategy

The `ProviderRequest` requires `SessionRef` and `TurnContext` that don't exist
in `CompletionRequest`. For the bridge:

- **SessionRef**: Create with `SessionRef::new(None)` for fresh sessions. When
  the agent loop evolves to pass session context (task 22), extend the bridge
  to accept it. Do not over-engineer now.
- **TurnContext**: Use an atomic counter in `ArkyDriverBridge` to track turn
  sequence. Each `complete()` or `stream()` call increments the counter and
  generates a fresh `TurnId`.

### What NOT To Do

- Do NOT modify `openfang-runtime/src/drivers/mod.rs` or `create_driver()` — the
  native driver path remains separate.
- Do NOT modify the agent loop (`agent_loop.rs`) — it already accepts
  `Arc<dyn LlmDriver>` which is what we produce.
- Do NOT add Arky dependencies to `openfang-runtime` — the bridge is in
  `openfang-provider-binding` which sits between the two.
- Do NOT try to handle session resume, MCP passthrough, or steering in the
  bridge — those are future concerns for when the agent loop gains dual-mode
  execution (agentic vs managed).
- Do NOT create new crates — everything fits in `openfang-provider-binding`.

### Dependency Direction Verification

After implementation, the dependency graph must look like:

```
openfang-kernel
  -> openfang-runtime (LlmDriver, agent_loop)
  -> openfang-provider-binding (binding_to_driver) <-- NEW dep
  -> arky-provider (ProviderRegistry)

openfang-provider-binding
  -> arky-provider (Provider trait)
  -> arky-protocol (wire types)
  -> arky-config (ProviderConfig)
  -> arky-codex (CodexProvider)
  -> arky-claude-code (ClaudeCodeProvider)
  -> openfang-runtime (LlmDriver trait only)
  -> openfang-types (message/tool types)

openfang-runtime
  (NO dependency on openfang-provider-binding)
```

Run `./scripts/check-deps.sh` to confirm.

### Relevant Files

- `crates/openfang-provider-binding/src/lib.rs` — main crate entry, add modules
- `crates/openfang-provider-binding/src/adapter.rs` — existing typed adapter (task 12)
- `crates/openfang-provider-binding/Cargo.toml` — add new deps
- `crates/arky-provider/src/traits.rs` — `Provider` trait (read only)
- `crates/arky-provider/src/error.rs` — `ProviderError` (read only)
- `crates/arky-protocol/src/` — Arky wire types (read only)
- `crates/openfang-runtime/src/llm_driver.rs` — `LlmDriver` trait (read only)
- `crates/openfang-types/src/message.rs` — OpenFang message types (read only)
- `crates/openfang-types/src/tool.rs` — OpenFang tool types (read only)
- `crates/arky-codex/src/provider.rs` — CodexProvider constructors (may need small additions)
- `crates/arky-claude-code/src/provider.rs` — ClaudeCodeProvider constructors (may need small additions)

## Deliverables

- `ArkyDriverBridge` struct implementing `LlmDriver` via an Arky `Provider`
- `binding_to_driver()` factory completing the compile-to-runtime pipeline
- `convert.rs` with exhaustive, tested type conversions
- `instantiate.rs` with `CompozyProviderConfig -> Arc<dyn Provider>`
- All new code in `crates/openfang-provider-binding/`
- `./scripts/check-deps.sh` passes — no circular deps
- `make fmt && make lint && make test` all pass

## Tests

### Unit Tests (Required)

- [ ] All `convert_should_*` tests from 4R.7 passing
- [ ] All `instantiate_should_*` tests from 4R.7 passing
- [ ] All `bridge_*` tests from 4R.7 passing

### Integration Tests (Required)

- [ ] All `binding_to_driver_should_*` tests from 4R.7 passing
- [ ] `cargo test -p openfang-provider-binding` passes with zero failures
- [ ] `cargo test -p openfang-runtime` still passes (no regressions)
- [ ] `cargo test -p openfang-kernel` still passes (no regressions)

### Regression and Anti-Pattern Guards

- [ ] No `#[ignore]` added to any test
- [ ] No `unwrap()` in non-test code
- [ ] No `todo!()`, `dbg!()`, or `unimplemented!()`
- [ ] No `log` crate usage — `tracing` only
- [ ] No circular dependencies in `./scripts/check-deps.sh`
- [ ] `openfang-runtime` has zero new dependencies on Arky crates

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `binding_to_driver()` takes a `ProviderBinding` + `ProviderConfig` and returns
  a working `Arc<dyn LlmDriver>` that can be passed to `run_agent_loop()`.
- The bridge correctly converts between all message types, tool calls, streaming
  events, and error types used by the agent loop.
- Tasks 20-22 can call `binding_to_driver()` at runtime to obtain a driver for
  any Compozy-defined agent, without modifying the agent loop.
- The existing native driver path (`create_driver()`) is completely unaffected.

## Notes

- This task is architecturally critical — it is the only code path that connects
  the Compozy compile pipeline (tasks 10-12) to the OpenFang execution runtime
  (agent loop). Without it, compiled agents are dead code.
- The bridge is intentionally one-directional (Arky -> LlmDriver). A future
  dual-mode agent loop (managed vs agentic) would consume the `Provider` trait
  directly for richer features (session resume, MCP passthrough, steering).
  That is out of scope for this task.
- Session context threading (passing real session IDs from the agent loop) will
  be addressed in task 22 when SSE streaming is implemented. The bridge should
  use `SessionRef::new(None)` for now and be easy to extend later.
