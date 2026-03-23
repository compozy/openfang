## markdown

## status: pending

<task_context>
<domain>providers/arky/adapters</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task11</dependencies>
</task_context>

# Task 12.0: Typed Provider Integration For Codex And Claude Code

## Overview

Connect `ProviderBinding` to the typed provider config and runtime paths for
Codex and Claude Code.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Support `codex` and `claude-code` as the first deep provider integrations. Each must map a `ProviderBinding` plus its corresponding install-layer `ProviderConfig` to a concrete, typed provider config struct that the Arky runtime can use directly.
- The Codex adapter must map to `CodexProviderConfig` in `arky-codex/src/config.rs`. It must apply `ResolvedCodexBehaviorConfig` (from `ProviderBinding.config`) onto the `CodexProviderConfig`'s sandbox, workspace, and capability sub-structs. Installation-level fields (`binary`, `env`, `args`, `request_timeout`, `startup_timeout`, `shared_app_server_key`, etc.) come from `ProviderConfig` (install layer), not from the binding.
- The Claude Code adapter must map to `ClaudeCodeProviderConfig` in `arky-claude-code/src/config.rs`. It must apply `ResolvedClaudeCodeBehaviorConfig` (from `ProviderBinding.config`) onto the config's session, filesystem, tool, and budget sub-structs. Installation-level fields (`binary`, `env`, `extra_args`, `spawn_failure_policy`) come from the install-layer `ProviderConfig`.
- Claude-compatible wrapper providers (Bedrock, Vertex, Zai, OpenRouter, Vercel, Moonshot, MiniMax, Ollama) must be handled through a third adapter path that maps `ResolvedClaudeCompatibleBehaviorConfig` (from `ProviderBinding.config`) to the appropriate wrapper config type in `arky-claude-code/src/profile.rs`. Each wrapper type (`BedrockProviderConfig`, `VertexProviderConfig`, `ZaiProviderConfig`, etc.) has a `base: ClaudeCompatibleProviderConfig` field that is the shared Claude CLI config.
- Adapter functions must be pure and synchronous — no network calls, no binary spawning, no process lifecycle management. They produce typed config structs; the Arky provider constructors (`CodexProvider::with_config()`, `ClaudeCodeProvider::with_profile_config()`, etc.) are called by the runtime, not by the adapters.
- Per ADR-012, provider-specific semantics must be preserved. Do not reduce provider-specific config to generic flags. Each typed config struct field that has a counterpart in `ResolvedCodexBehaviorConfig` or `ResolvedClaudeCodeBehaviorConfig` must be mapped explicitly and named.
- The `ProviderBinding.defaults` fields (`max_tokens`, `reasoning_effort`) must be applied consistently: `reasoning_effort` maps to `ClaudeCodeProviderConfig.reasoning_effort` and `CodexProviderConfig.reasoning_effort`; `max_tokens` maps to `ClaudeCodeProviderConfig.max_turns` (indirectly, as a budget constraint) or carried into `ProviderRequest.settings` at call time rather than baked into the static config.
- Wrapper providers must not silently ignore unsupported fields. If a `ClaudeCompatibleBehaviorLayer` contains fields that a given wrapper does not support (e.g., `fork_session` for Bedrock), emit a warning via `tracing::warn!()` rather than silently discarding the value. Do not use `println!()` or the `log` crate (clippy enforces `tracing`).
</requirements>

## Subtasks

- [ ] 12.1 Implement `binding_to_codex_config()` — a function that takes a `&ProviderBinding` and a `&ProviderConfig` (install layer) and returns `Result<CodexProviderConfig, AdapterError>`. Map fields from `ResolvedCodexBehaviorConfig` onto `CodexProviderConfig` sub-structs (`CodexSandboxConfig`, `CodexWorkspaceConfig`, `CodexCapabilityConfig`). Set installation-level fields from `ProviderConfig`: `binary` from `ProviderConfig::kind()` default or `ProviderConfig::binary()`, `env` from `ProviderConfig::env()`, timeouts from defaults. Preserve all typed fields — do not collapse them into `config_overrides`.
- [ ] 12.2 Implement `binding_to_claude_code_config()` — a function that takes a `&ProviderBinding` and a `&ProviderConfig` (install layer) and returns `Result<ClaudeCodeProviderConfig, AdapterError>`. Map `ResolvedClaudeCodeBehaviorConfig` fields onto `ClaudeCodeProviderConfig`: `session` sub-struct from `ClaudeSessionConfig`, `filesystem` sub-struct from `ClaudeFilesystemConfig`, `allowed_tools`, `disallowed_tools`, `mcp_servers`, `max_budget_usd`, `fallback_model`, and `reasoning_effort` from `ProviderBinding.defaults`. Set install-level fields from `ProviderConfig`.
- [ ] 12.3 Implement `binding_to_claude_compatible_config()` — a function that takes a `&ProviderBinding`, the `ClaudeCompatibleProviderKind` (derived from the driver string), and a `&ProviderConfig` and returns `Result<ClaudeProviderProfile, AdapterError>`. Use `ResolvedClaudeCompatibleBehaviorConfig`'s `base` to populate the shared `ClaudeCompatibleProviderConfig`, and use `selected_model`, `region`, `project_id` to populate wrapper-specific config structs (`BedrockProviderConfig`, `VertexProviderConfig`, etc.).
- [ ] 12.4 Define `AdapterError` using `thiserror`. Required variants:
  - `UnsupportedDriver { driver: String }` — binding driver does not map to a known provider adapter
  - `MissingBehaviorConfig { driver: String }` — binding's `config` field is `None` but the adapter requires typed config
  - `ConfigTypeMismatch { expected: String, found: String }` — binding's `config` variant does not match the expected driver (e.g., `Codex` config for a `claude-code` driver)
  - `AdapterError` must implement `ClassifiedError` from `arky-error/src/lib.rs`.
- [ ] 12.5 Create a dispatch function `build_provider_config()` that takes a `&ProviderBinding` and a `&ProviderConfig` and returns `Result<CompozyProviderConfig, AdapterError>`, where `CompozyProviderConfig` is an enum: `Codex(CodexProviderConfig)`, `ClaudeCode(ClaudeCodeProviderConfig)`, `ClaudeCompatible { kind: ClaudeCompatibleProviderKind, profile: ClaudeProviderProfile }`. This is the single public entry point for the adapter layer.
- [ ] 12.6 Add adapter tests for provider-specific behavior and invalid config combinations (see Tests section). Confirm that `ClaudeCodeProviderConfig::cli_args()` produces expected flags when built from a `ProviderBinding` with known fields.
- [ ] 12.7 Document the field mapping from `ResolvedCodexBehaviorConfig` to `CodexProviderConfig` and from `ResolvedClaudeCodeBehaviorConfig` to `ClaudeCodeProviderConfig` as inline `///` doc comments on the adapter functions, so the mapping is visible without reading two files.

## Implementation Details

### Typed Config Structs Already in the Copied Arky Crates

**`CodexProviderConfig`** (`/Users/pedronauck/Dev/compozy/openfang/crates/arky-codex/src/config.rs`):
The config is large. The fields that come from the **behavior layer** (agent/profile level) are:

- `sandbox.sandbox_mode` ← `ResolvedCodexBehaviorConfig.sandbox.sandbox_mode`
- `sandbox.sandbox_network_access` ← `ResolvedCodexBehaviorConfig.sandbox.sandbox_network_access`
- `workspace.include_plan_tool` ← `ResolvedCodexBehaviorConfig.workspace.include_plan_tool`
- `workspace.resume_last` ← `ResolvedCodexBehaviorConfig.workspace.resume_last`
- `capability.web_search` ← `ResolvedCodexBehaviorConfig.web_search`
- `capability.rmcp_client` ← `ResolvedCodexBehaviorConfig.rmcp_client`
- `reasoning_summary` ← `ResolvedCodexBehaviorConfig.reasoning_summary`
- `model_verbosity` ← `ResolvedCodexBehaviorConfig.model_verbosity`
- `reasoning_effort` ← `ProviderBinding.defaults.reasoning_effort` (as `Option<String>`)

The fields that come from the **install layer** (`ProviderConfig`) are:

- `binary` ← `ProviderConfig::binary()` or default `"codex"`
- `env` ← `ProviderConfig::env()`
- `app_server_args` ← `ProviderConfig::args()`
- Process and timeout fields (`request_timeout`, `scheduler_timeout`, `startup_timeout`,
  `idle_shutdown_timeout`) — use `CodexProviderConfig::default()` values unless `ProviderConfig`
  carries specific overrides (currently it does not; these are install-concern only)
- `client_name`, `client_version` — always defaults from `CodexProviderConfig::default()`

**`ClaudeCodeProviderConfig`** (`/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/config.rs`):
Fields from the **behavior layer**:

- `session.continue_conversation` ← `ResolvedClaudeCodeBehaviorConfig.session.continue_conversation`
- `session.fork_session` ← `ResolvedClaudeCodeBehaviorConfig.session.fork_session`
- `filesystem.additional_directories` ← `ResolvedClaudeCodeBehaviorConfig.filesystem.additional_directories`
- `filesystem.enable_file_checkpointing` ← `ResolvedClaudeCodeBehaviorConfig.filesystem.enable_file_checkpointing`
- `allowed_tools` ← `ResolvedClaudeCodeBehaviorConfig.allowed_tools`
- `disallowed_tools` ← `ResolvedClaudeCodeBehaviorConfig.disallowed_tools`
- `mcp_servers` ← `ResolvedClaudeCodeBehaviorConfig.mcp_servers`
- `max_budget_usd` ← `ResolvedClaudeCodeBehaviorConfig.max_budget_usd`
- `cli_behavior.fallback_model` ← `ResolvedClaudeCodeBehaviorConfig.fallback_model`
- `reasoning_effort` ← `ProviderBinding.defaults.reasoning_effort` (as `Option<String>`)

Fields from the **install layer**:

- `binary` ← `ProviderConfig::binary()` or default `"claude"`
- `env` ← `ProviderConfig::env()`
- `extra_args` ← `ProviderConfig::args()`
- `spawn_failure_policy`, `max_frame_len`, `version_args` — always defaults

**`ClaudeProviderProfile`** (`/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/profile.rs`):
The `ClaudeProviderProfile` enum carries one variant per Claude-compatible provider:
`ClaudeCode`, `Zai(ZaiProviderConfig)`, `OpenRouter(OpenRouterProviderConfig)`,
`Vercel(VercelProviderConfig)`, `Moonshot(MoonshotProviderConfig)`,
`Minimax(MinimaxProviderConfig)`, `Bedrock(BedrockProviderConfig)`,
`Vertex(VertexProviderConfig)`, `Ollama(OllamaProviderConfig)`.

All wrapper configs have a `base: ClaudeCompatibleProviderConfig` field (which is a type alias
for `ClaudeCodeProviderConfig`). The `selected_model`, `region`, and `project_id` fields from
`ResolvedClaudeCompatibleBehaviorConfig` map to wrapper-specific fields (e.g.,
`BedrockProviderConfig.region`, `VertexProviderConfig.project_id`).

The `ClaudeCompatibleProviderKind::from_kind(driver)` function parses the normalized driver string
into a `ClaudeCompatibleProviderKind` enum value, which determines which wrapper variant to
construct.

### Building a `ClaudeCodeProvider` From the Adapted Config

The pattern for constructing a Claude Code provider in Arky is:

```rust
ClaudeCodeProvider::with_profile_config(profile, base_config)
```

where `profile` is a `ClaudeProviderProfile` and `base_config` is a
`ClaudeCodeProviderConfig`. This is the runtime instantiation call — it happens in the runtime
layer (not in the adapter). The adapter only produces the config structs.

The pattern for `CodexProvider` is:

```rust
CodexProvider::new(codex_config)
```

or

```rust
CodexProvider::with_config(codex_config)
```

The adapter produces `CodexProviderConfig`; the runtime creates the `CodexProvider`.

### Crate Placement for Adapters

The adapter functions should live in the same crate as `ProviderBinding` (i.e.,
`openfang-provider-binding`) or in a new sibling module/crate. They must not be placed inside
`arky-codex` or `arky-claude-code` because those are upstream-syncable. The adapters are
Compozy-specific mapping code.

### Field Mapping for `reasoning_effort`

`ProviderBinding.defaults.reasoning_effort` is of type `Option<ReasoningEffort>` (an enum from
`arky-protocol`). Both `CodexProviderConfig` and `ClaudeCodeProviderConfig` store
`reasoning_effort: Option<String>`. The mapping must serialize `ReasoningEffort` to its string
form (`"low"`, `"medium"`, `"high"`, `"max"`) before assigning.

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-codex/src/config.rs` — `CodexProviderConfig`, `CodexProcessConfig`, `CodexSandboxConfig`, `CodexWorkspaceConfig`, `CodexCapabilityConfig`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-codex/src/provider.rs` — `CodexProvider` constructors
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/config.rs` — `ClaudeCodeProviderConfig`, `ClaudeSessionConfig`, `ClaudeFilesystemConfig`, `ClaudeCliBehaviorConfig`, `ClaudePermissionConfig`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/profile.rs` — `ClaudeProviderProfile`, `ClaudeCompatibleProviderKind`, `BedrockProviderConfig`, `VertexProviderConfig`, `ZaiProviderConfig`, `OpenRouterProviderConfig`, `VercelProviderConfig`, `MoonshotProviderConfig`, `MinimaxProviderConfig`, `OllamaProviderConfig`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/provider.rs` — `ClaudeCodeProvider` constructors and `with_profile_config()`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/layered.rs` — `ResolvedCodexBehaviorConfig`, `ResolvedClaudeCodeBehaviorConfig`, `ResolvedClaudeCompatibleBehaviorConfig`, `ResolvedProviderBehaviorConfig`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-error/src/lib.rs` — `ClassifiedError` trait
- The new `ProviderBinding` crate/module from task 11 — provides `ProviderBinding`, `CompileError`
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/adrs/012-arky-provider-depth-for-claude-code-and-codex.md`
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/adrs/027-provider-specific-agent-configuration.md`

### Dependent Files

- Agent-definition compiler (future task) — calls `build_provider_config()` to get the typed config
- Runtime dispatch — uses `CompozyProviderConfig` to instantiate the correct Arky provider and register it in `ProviderRegistry`
- `openfang-runtime/src/drivers/mod.rs` — legacy driver path; must remain independent; do not replace it with the new adapter in this task

## Deliverables

- `binding_to_codex_config()` adapter function with full field mapping
- `binding_to_claude_code_config()` adapter function with full field mapping
- `binding_to_claude_compatible_config()` adapter function for all nine wrapper kinds
- `build_provider_config()` dispatch function returning `CompozyProviderConfig`
- `AdapterError` enum implementing `ClassifiedError`
- Tests for all provider-specific behaviors and invalid config combinations

## Tests

### Unit Tests (Required)

- [ ] `codex_binding_should_map_sandbox_mode_to_codex_provider_config` — a `ProviderBinding` with `ResolvedCodexBehaviorConfig { sandbox_mode: Some("workspace-write"), ... }` must produce a `CodexProviderConfig` with `sandbox.sandbox_mode = Some("workspace-write")`
- [ ] `codex_binding_should_map_web_search_and_reasoning_summary` — verify `CodexCapabilityConfig.web_search` and `CodexProviderConfig.reasoning_summary` are set correctly
- [ ] `claude_code_binding_should_map_session_and_filesystem_fields` — a `ProviderBinding` with `ResolvedClaudeCodeBehaviorConfig { session: { fork_session: true, ... }, filesystem: { additional_directories: [...], ... } }` must produce the correct `ClaudeCodeProviderConfig.session` and `.filesystem`
- [ ] `claude_code_binding_should_map_allowed_and_disallowed_tools` — verify `allowed_tools` and `disallowed_tools` survive the mapping unchanged
- [ ] `claude_code_binding_should_map_mcp_servers_and_budget` — verify `mcp_servers` and `max_budget_usd` are set correctly
- [ ] `claude_code_cli_args_should_reflect_binding_fields` — call `ClaudeCodeProviderConfig::cli_args()` on the adapted config and assert that `--allowed-tools`, `--fork-session`, `--max-budget-usd`, and `--effort` flags appear when the corresponding binding fields are populated
- [ ] `claude_compatible_bedrock_binding_should_set_region` — a `ResolvedClaudeCompatibleBehaviorConfig` with `region = Some("us-east-1")` must produce a `BedrockProviderConfig` with `.region = Some("us-east-1")`
- [ ] `adapter_should_reject_codex_config_for_claude_code_driver` — a `ProviderBinding` with `driver = "claude-code"` but `config = ResolvedProviderBehaviorConfig::Codex(...)` must return `AdapterError::ConfigTypeMismatch`
- [ ] `adapter_should_reject_binding_with_no_behavior_config_when_required` — if the adapter requires typed config but `ProviderBinding.config` is `None`, return `AdapterError::MissingBehaviorConfig`
- [ ] `reasoning_effort_should_serialize_to_string_in_adapter` — `ReasoningEffort::High` from `ProviderBinding.defaults` must map to `reasoning_effort = Some("high")` in both `CodexProviderConfig` and `ClaudeCodeProviderConfig`

### Integration Tests (Required)

- [ ] An agent TOML with `provider.driver = "codex"`, `provider.model = "gpt-4o"`, and a `[provider.config.codex]` block including `include_plan_tool = true` and `web_search = true` must produce — after full compilation through task 10 and 11 — a `CodexProviderConfig` with those fields set correctly
- [ ] An agent TOML with `provider.driver = "claude-code"` and `provider.config.claude_code.allowed_tools = ["read_file", "edit_file"]` must produce a `ClaudeCodeProviderConfig` with those tools in `allowed_tools`
- [ ] An agent TOML with `provider.driver = "bedrock"` and `provider.config.claude_compatible.region = "eu-west-1"` must produce a `BedrockProviderConfig` with `.region = Some("eu-west-1")`
- [ ] Provider-specific session settings (e.g., `fork_session = true` for Claude Code) must survive the full compile → adapt pipeline without being lost or overwritten with defaults
- [ ] A `CompozyProviderConfig::Codex(config)` produced by `build_provider_config()` must be usable to construct a `CodexProvider` (via `CodexProvider::new(config)`) without panics or compile errors

### Regression and Anti-Pattern Guards

- [ ] Do not reduce provider-specific config to generic stringly-typed flags (e.g., do not store `{"sandbox_mode": "workspace-write"}` as an untyped `BTreeMap<String, Value>` when `CodexSandboxConfig.sandbox_mode: Option<String>` already exists)
- [ ] Do not introduce one-off field mappings that bypass the typed config structs — every field in `ResolvedCodexBehaviorConfig` and `ResolvedClaudeCodeBehaviorConfig` must have an explicit named mapping in the adapter
- [ ] Do not let provider wrappers silently ignore unsupported behavior fields — use `tracing::warn!()` for fields that cannot be applied to a specific wrapper kind
- [ ] Do not use `println!()`, `dbg!()`, `log::warn!()`, or `eprintln!()` — use `tracing::warn!()` only (enforced by `.clippy.toml`)
- [ ] Do not collapse `CodexBehaviorLayer` or `ClaudeCodeBehaviorLayer` config into the `config_overrides: BTreeMap<String, Value>` field on `CodexProviderConfig` — that field is for Codex RPC overrides and must not be repurposed

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `binding_to_codex_config()` maps all eight `ResolvedCodexBehaviorConfig` fields to the correct sub-structs of `CodexProviderConfig`.
- `binding_to_claude_code_config()` maps all nine `ResolvedClaudeCodeBehaviorConfig` fields to the correct sub-structs of `ClaudeCodeProviderConfig`.
- All nine Claude-compatible wrapper kinds in `ClaudeCompatibleProviderKind` are handled in `binding_to_claude_compatible_config()`.
- `build_provider_config()` correctly dispatches to the right adapter based on `ProviderBinding.driver`.
- `ClaudeCodeProviderConfig::cli_args()` produces the expected flags when called on an adapted config — confirming the adapter preserves semantics end-to-end.
- Both Codex and Claude Code are reachable from a Compozy agent definition through the full pipeline: TOML → validate → compile → adapt → provider instance.

---

## Prior Implementation Reference

The old TypeScript codebase has provider adapters and runtime integration that show session/config patterns:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/` — Tool adapters for Codex and Claude Code
- `~/Dev/compozy/compozy-code/providers/core/src/` — Provider hooks, MCP bridges, tool bridges, token consumption
- `~/Dev/compozy/compozy-code/providers/runtime/src/` — OpenResponses protocol gateway, AI SDK bridge, session management

The old provider layer uses a Vercel AI SDK bridge. The new one uses Arky crates with typed bindings,
but the old adapters show how provider-specific config and session identity were handled.

## Notes

- These are the strategic providers for the first meaningful Compozy version (per ADR-012).
- The legacy `openfang-runtime/src/drivers/claude_code.rs` and `drivers/mod.rs` implement the old OpenFang provider path. Do not remove or replace them in this task. The new Arky adapter path is additive.
- `ClaudeCodeProviderConfig::cli_args()` in `arky-claude-code/src/config.rs` is a rich, well-tested method. Using it in integration tests as the ground-truth for what flags the Claude CLI will receive is the most reliable integration validation available without spawning a real process.
