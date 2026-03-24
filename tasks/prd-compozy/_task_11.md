## markdown

## status: completed

<task_context>
<domain>providers/arky/binding</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task10</dependencies>
</task_context>

# Task 11.0: ProviderBinding Compile Layer For Compozy Agents

## Overview

Create the new compile layer that turns Compozy `agent_definition.provider`
data into a concrete `ProviderBinding` suitable for runtime use.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Per ADR-029, the public `agent_definition` must compile into three internal layers: `AgentManifest`, `ProviderBinding`, and `AgentProductMetadata`. This task owns `ProviderBinding`. It must carry exactly: provider identity (driver string and `ProviderId`), resolved model (`ModelRef`), profile name reference, request defaults (`ProviderRequestDefaults`), and typed provider behavior config (`ResolvedProviderBehaviorConfig`).
- `ProviderBinding` must be a new type defined in an `openfang`-owned crate (e.g., a new `openfang-provider-binding` crate or a new module within `openfang-kernel`). It must not be defined inside the Arky crates themselves — those remain upstream-syncable. The binding is Compozy's seam between public config surfaces and the Arky provider runtime.
- The compiler that produces `ProviderBinding` from `ResolvedAgentProviderConfig<ProviderConfig>` (task 10's output) must be a pure, synchronous function. No provider boot, no network calls, no binary discovery during compilation. Provider binary existence checks (via `find_binary_on_path()` from `arky-config/src/validate.rs`) belong at installation-check time, not compile time.
- Per ADR-043, `ProviderBinding` must not contain raw environment maps, credential strings, binary paths, or any installation-level secrets. Those remain in the `ProviderConfig` (install layer) and are resolved at provider instantiation time, not at compile time.
- The `ProviderBinding` must expose enough information for the `ProviderRegistry` (in `arky-provider/src/registry.rs`) to look up and instantiate the correct provider. Specifically, it must carry a `ProviderId` (or at minimum the driver string normalized by `normalize_driver()` from `layered.rs`) so the runtime can call `ProviderRegistry::get(&provider_id)`.
- Capability validation must be performed at compile time using `validate_capabilities()` from `arky-provider/src/descriptor.rs`. If the resolved `ProviderBinding` requests capabilities (e.g., `session_resume`, `extended_thinking`) that the named driver cannot support, return a `CompileError` before the binding reaches the runtime. Do not defer this to runtime execution.
- The `ProviderBinding` must be serializable (`serde::Serialize`) and deserializable (`serde::Deserialize`) so that compiled agent definitions can be cached and reloaded without re-compilation. It must also implement `Clone`, `Debug`, and `PartialEq`.
- Keep the compile output separate from raw file-backed agent definitions. Agent TOML files are source-of-truth; `ProviderBinding` is a derived, compiled artifact. The two must not be conflated in API responses or storage.
</requirements>

## Subtasks

- [x] 11.1 Decide the crate placement for `ProviderBinding`. The most natural location is a new `openfang-provider-binding` crate under `openfang/crates/`. It depends on `arky-config` (for `ResolvedAgentProviderConfig`, `ResolvedProviderBehaviorConfig`), `arky-provider` (for `ProviderId`, `ProviderRegistry`, `validate_capabilities`), and `arky-protocol` (for `ModelRef`, `ProviderSettings`). Add it to `openfang/Cargo.toml` workspace members. Alternatively, place it as a `provider_binding` module within `openfang-kernel` if it is tightly coupled there — document the decision.
- [x] 11.2 Define the `ProviderBinding` struct. Required fields:
  - `driver: String` — normalized driver string (e.g., `"claude-code"`, `"codex"`)
  - `provider_id: ProviderId` — from `arky-protocol`, keyed to `ProviderRegistry` lookup
  - `model: ModelRef` — from `arky-protocol`, carries `model_id` and optional `provider_model_id`
  - `profile: Option<String>` — named profile reference, preserved for observability
  - `defaults: ProviderRequestDefaults` — from `arky-config/src/layered.rs`
  - `config: Option<ResolvedProviderBehaviorConfig>` — from `arky-config/src/layered.rs`
  - All fields must be `pub` and the struct must derive `Debug`, `Clone`, `PartialEq`, `Serialize`, `Deserialize`.
- [x] 11.3 Implement the `compile_provider_binding()` function (or a `ProviderBindingCompiler` struct with a `compile()` method) that takes a `ResolvedAgentProviderConfig<ProviderConfig>` and returns `Result<ProviderBinding, CompileError>`. The steps are: (1) normalize driver string via `normalize_driver()`; (2) construct `ProviderId::new(driver)`; (3) construct `ModelRef` from the resolved model string; (4) copy `defaults` and `config` from the resolved config; (5) validate the resolved `config` against the driver's known capabilities.
- [x] 11.4 Define `CompileError` using `thiserror`. Required variants:
  - `UnknownDriver { driver: String }` — driver string does not map to a known `ProviderFamily`
  - `CapabilityMismatch { capability: String, driver: String, message: String }` — the resolved config requests a capability the driver cannot support
  - `MissingModel { driver: String }` — no model is available after profile and agent merge
  - `InvalidRequestExtra { field: String, message: String }` — re-surface validation issues from `validate_request_extra()` as a compile error
  - `CompileError` must implement `ClassifiedError` from `arky-error/src/lib.rs`.
- [x] 11.5 Add a driver capability validation step inside `compile_provider_binding()`. For each known driver, declare the expected `ProviderCapabilities` flags (e.g., `claude-code` supports `streaming`, `generate`, `tool_calls`, `mcp_passthrough`, `session_resume`; `codex` supports `streaming`, `generate`, `tool_calls`, `code_execution`). Use `ProviderCapabilities` from `arky-provider/src/descriptor.rs`. If the resolved config enables a feature the driver cannot support (e.g., `session_resume` for `codex`), emit a `CompileError::CapabilityMismatch`.
- [x] 11.6 Add a `resolve_provider_id()` helper that maps driver strings to `ProviderId` values using the same logic as `infer_provider_id()` in `arky-provider/src/registry.rs`. Codex maps to `ProviderId::new("codex")`; `claude-code` maps to `ProviderId::new("claude-code")`; Claude-compatible wrappers map to their canonical IDs from `CLAUDE_COMPATIBLE_PROVIDER_IDS` in `arky-claude-code/src/profile.rs`.
- [x] 11.7 Write tests for common and invalid `ProviderBinding` compile cases (see Tests section). Place them as `#[cfg(test)]` inline in the new crate or module.

## Implementation Details

### Where `ProviderBinding` Sits in the Compilation Pipeline

The full agent definition compilation pipeline per DESIGN.md section 4 is:

1. Parse TOML/JSON → raw `agent_definition` struct
2. Schema validation (required fields, enum values, known driver)
3. Reference validation (named profile exists in workspace config)
4. Semantic validation (cross-field compatibility)
5. Normalization (fill defaults, canonicalize driver names)
6. **Compile** → `AgentManifest` + `ProviderBinding` + `AgentProductMetadata`
7. Execute only the compiled IR

This task owns step 6 for the `ProviderBinding` output. Steps 1-5 happen in the agent-definition
compiler (future task). This task provides the `compile_provider_binding()` function that step 6
will call once validation has passed.

### What Already Exists in `arky-provider`

The `ProviderRegistry` in `arky-provider/src/registry.rs` is a thread-safe
`Arc<RwLock<BTreeMap<ProviderId, Arc<dyn Provider>>>>`. The `compile_provider_binding()` function
does NOT interact with the registry — it only produces a `ProviderBinding` that the runtime will
later use to call `ProviderRegistry::get(&binding.provider_id)`. Keep compile and runtime strictly
separate.

The `ProviderDescriptor` in `arky-provider/src/descriptor.rs` carries `id: ProviderId`,
`family: ProviderFamily`, and `capabilities: ProviderCapabilities`. The canonical capabilities for
each driver family are already declared in each provider crate:

- `arky-claude-code/src/profile.rs` — `claude_compatible_capabilities()` returns the capabilities
  for all Claude-family providers: `streaming`, `generate`, `tool_calls`, `mcp_passthrough`,
  `session_resume` all `true`.
- `arky-codex/src/provider.rs` — declares Codex capabilities; inspect this file.

### What `ResolvedAgentProviderConfig<TInstall>` Provides

After task 10 completes, `resolve_agent_provider()` returns:

```
ResolvedAgentProviderConfig<ProviderConfig> {
    provider: String,        // named provider entry key
    driver: String,          // normalized driver string
    profile: Option<String>, // named profile if referenced
    install: ProviderConfig, // installation-level config (binary, env, etc.)
    model: Option<String>,   // resolved model string
    defaults: ProviderRequestDefaults,
    config: Option<ResolvedProviderBehaviorConfig>,
    request_extra: BTreeMap<String, Value>,
}
```

The `ProviderBinding` compiler takes this as input and produces the runtime-safe binding by:

- Extracting `driver` and normalizing it
- Converting `model: Option<String>` into `ModelRef` (must not be `None` for runtime agents;
  emit `CompileError::MissingModel` if absent)
- Carrying `defaults` and `config` forward verbatim
- Dropping `install` (credentials/binary — runtime concern) and `request_extra` from the binding
  output (validate `request_extra` separately, surface issues as `CompileError::InvalidRequestExtra`)

### `ModelRef` Construction

`ModelRef` is defined in `arky-protocol/src/request.rs` (or similar). It carries:

- `model_id: String` — the primary model string
- `provider_id: Option<ProviderId>` — optional explicit provider override
- `provider_model_id: Option<String>` — optional provider-specific model alias

For Compozy agents, `ModelRef::new(model_string)` is the standard construction path. If a
Claude-compatible wrapper profile specifies `selected_model`, that goes into `provider_model_id`.

### Crate Dependency Chain for the New Crate

```
openfang-provider-binding
  -> arky-config      (ResolvedAgentProviderConfig, ResolvedProviderBehaviorConfig,
                       ProviderRequestDefaults, validate_request_extra)
  -> arky-provider    (ProviderId, ProviderCapabilities, validate_capabilities)
  -> arky-protocol    (ModelRef, ReasoningEffort)
  -> arky-error       (ClassifiedError)
  -> thiserror        (CompileError derives)
  -> serde + serde_json (Serialize, Deserialize)
```

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/layered.rs` — `ResolvedAgentProviderConfig`, `ResolvedProviderBehaviorConfig`, `ProviderRequestDefaults`, `normalize_driver`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-provider/src/descriptor.rs` — `ProviderDescriptor`, `ProviderCapabilities`, `validate_capabilities()`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-provider/src/registry.rs` — `ProviderRegistry`, `infer_provider_id()`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-protocol/src/request.rs` — `ModelRef`, `ProviderSettings`, `TurnContext`, `SessionRef`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/profile.rs` — `CLAUDE_COMPATIBLE_PROVIDER_IDS`, `claude_compatible_capabilities()`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-codex/src/provider.rs` — Codex provider capabilities declaration
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-error/src/lib.rs` — `ClassifiedError` trait
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/validate.rs` — `find_binary_on_path()` (NOT called during compile — referenced for contrast)
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/DESIGN.md` — "Internal Compilation Model" section
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/adrs/029-agent-definition-public-schema.md`

### Dependent Files

- Agent-definition compiler (future task) — calls `compile_provider_binding()` as part of agent compile
- `openfang-kernel` — will hold a compiled agent registry; needs `ProviderBinding` to be serializable for caching
- Task 12 (`arky-codex` and `arky-claude-code` adapters) — takes `ProviderBinding` and converts it to concrete `CodexProviderConfig` or `ClaudeCodeProviderConfig`
- Runtime dispatch — uses `binding.provider_id` to look up the registered `Arc<dyn Provider>`

## Deliverables

- New `openfang-provider-binding` crate (or module) with `ProviderBinding` struct
- `CompileError` enum implementing `ClassifiedError`
- `compile_provider_binding()` function with full driver normalization and capability validation
- Tests for all common and failure cases

## Tests

### Unit Tests (Required)

- [x] `provider_binding_should_capture_driver_model_profile_defaults_and_config` — verify all fields are correctly set after `compile_provider_binding()` from a fully populated `ResolvedAgentProviderConfig`
- [x] `provider_binding_should_normalize_claude_alias_to_claude_code` — driver string `"claude"` must produce `provider_id = ProviderId::new("claude-code")` in the binding
- [x] `provider_binding_should_reject_missing_model` — a `ResolvedAgentProviderConfig` with `model: None` must return `CompileError::MissingModel`
- [x] `provider_binding_should_reject_unknown_driver` — an unrecognized driver string (e.g., `"some-unknown-llm"`) must return `CompileError::UnknownDriver`
- [x] `provider_binding_should_enforce_capability_mismatch_for_session_resume_on_codex` — a Codex binding that tries to enable `session_resume` (if it is unsupported) must return `CompileError::CapabilityMismatch`
- [x] `provider_binding_should_reject_forbidden_request_extra_key` — a resolved config with `request_extra = { "api_key": "..." }` must return `CompileError::InvalidRequestExtra`
- [x] `provider_binding_should_serialize_and_deserialize_round_trip` — serialize a `ProviderBinding` to JSON and deserialize it back; assert equality
- [x] `compile_error_should_implement_classified_error` — verify `CompileError::UnknownDriver` has a stable `error_code()` and non-zero `http_status()`

### Integration Tests (Required)

- [x] A fully specified `ResolvedAgentProviderConfig` for `claude-code` with `ClaudeCode` behavior config must compile to a `ProviderBinding` with `provider_id = "claude-code"` and a non-None `config`
- [x] A fully specified `ResolvedAgentProviderConfig` for `codex` with `Codex` behavior config must compile to a `ProviderBinding` with `provider_id = "codex"` and a non-None `config`
- [x] A `ProviderBinding` for `claude-code` must have its `provider_id` resolvable via `ProviderRegistry::get()` when a `ClaudeCodeProvider` is registered under `ProviderId::new("claude-code")`
- [x] Bindings with mismatched driver and config namespace (e.g., `driver = "codex"` but `config = ClaudeCode(...)`) must fail at compile time, not silently produce a corrupted binding
- [x] Compilation must not invoke `find_binary_on_path()`, network operations, or any I/O — confirm by running compile in an environment without the provider binary on PATH

### Regression and Anti-Pattern Guards

- [x] Do not store raw untyped `serde_json::Value` as the main runtime provider contract inside `ProviderBinding` — all config must be in typed fields
- [x] Do not include installation secrets (`env`, `binary`, `api_key`, `credentials`) in `ProviderBinding` — they remain in `ProviderConfig` (install layer)
- [x] Do not bypass `validate_capabilities()` where Arky already provides it — call it explicitly during compile
- [x] Do not allow `ProviderBinding` to be constructed directly without going through `compile_provider_binding()` — make the struct fields accessible but restrict construction via `pub(crate)` constructors or a builder pattern
- [x] Do not defer capability validation to runtime execution — it must be a compile-time `CompileError`

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- `ProviderBinding` is a stable, serializable, runtime-safe struct that carries identity, model, defaults, and typed config.
- `compile_provider_binding()` is a pure, synchronous function with no I/O side effects.
- All four `CompileError` variants have stable `error_code()` values implementing `ClassifiedError`.
- Capability validation is enforced at compile time for both `claude-code` and `codex` drivers.
- Agent-definition compilation can call `compile_provider_binding()` without leaking Arky internals directly into the public Compozy agent schema.
- The binding's `provider_id` is always resolvable via `ProviderRegistry::get()` for providers registered in the runtime.

---

## Notes

- This task is the seam between Compozy config surfaces and the Arky provider runtime. Keep it pure and synchronous.
- The `ProviderBinding` is a derived artifact — it must never be the source of truth for agent definitions. Source of truth is always the file-backed agent TOML.
- `request_extra` is validated at compile time but does NOT appear in `ProviderBinding` — it is applied directly to the `ProviderRequest` at call time by the runtime.
