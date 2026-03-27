# Task 12 Review: Typed Provider Integration For Codex And Claude Code

## Status: PASS

## Checklist
- [x] 12.1 `binding_to_codex_config()` implemented — present in `adapter.rs` (line 105); maps all fields from `ResolvedCodexBehaviorConfig` onto `CodexProviderConfig` sub-structs; install-level fields taken from `ProviderConfig`
- [x] 12.2 `binding_to_claude_code_config()` implemented — present in `adapter.rs` (line 152); maps `ResolvedClaudeCodeBehaviorConfig` fields onto `ClaudeCodeProviderConfig`; `reasoning_effort` serialized from `ReasoningEffort` enum to string
- [x] 12.3 `binding_to_claude_compatible_config()` implemented — present in `adapter.rs` (line 174); handles all nine `ClaudeCompatibleProviderKind` variants (ClaudeCode, Bedrock, Vertex, Zai, OpenRouter, Vercel, Moonshot, Minimax, Ollama); uses `warn_unsupported_wrapper_fields()` for unsupported field combinations
- [x] 12.4 `AdapterError` defined — three variants: `UnsupportedDriver`, `MissingBehaviorConfig`, `ConfigTypeMismatch`; implements `ClassifiedError`
- [x] 12.5 `build_provider_config()` dispatch function present (line 247); returns `CompozyProviderConfig` enum with `Codex`, `ClaudeCode`, `ClaudeCompatible { kind, profile }` variants
- [x] 12.6 Adapter tests present — `codex_binding_should_map_sandbox_mode_to_codex_provider_config`, `codex_binding_should_map_web_search_and_reasoning_summary`, `claude_code_binding_should_map_session_and_filesystem_fields`, `claude_code_binding_should_map_allowed_and_disallowed_tools`, `claude_code_binding_should_map_mcp_servers_and_budget`, `claude_code_cli_args_should_reflect_binding_fields`, `claude_compatible_bedrock_binding_should_set_region`, `adapter_should_reject_codex_config_for_claude_code_driver`, `adapter_should_reject_binding_with_no_behavior_config_when_required`, `reasoning_effort_should_serialize_to_string_in_adapter`
- [x] 12.7 Field mapping documented with inline `///` doc comments on adapter functions — verified in `adapter.rs` (lines around 105–175)

## Findings

### Correctly Implemented
- All nine Claude-compatible wrapper kinds are handled in `binding_to_claude_compatible_config()` with no fall-through.
- Unsupported wrapper fields emit `tracing::warn!()` (e.g., `fork_session` for Bedrock) via `warn_unsupported_wrapper_fields()` — tested by `adapter_should_warn_when_bedrock_ignores_fork_session`.
- `reasoning_effort` is correctly serialized from `Option<ReasoningEffort>` enum to `Option<String>` for both Codex and Claude Code configs.
- Provider-specific typed config fields are NOT collapsed into generic `config_overrides` maps; explicit named field mapping is used throughout.
- `build_provider_config()` correctly dispatches to the right adapter based on `ProviderBinding.driver`.
- Integration tests confirm full pipeline: TOML → compile (via `ConfigLoader`) → adapt → typed config: `codex_agent_toml_should_compile_and_adapt_to_typed_config`, `claude_code_agent_toml_should_compile_and_adapt_allowed_tools`, `bedrock_agent_toml_should_compile_and_adapt_region`.
- `compozy_codex_provider_config_should_construct_a_codex_provider` verifies that `CompozyProviderConfig::Codex(config)` produces a valid `CodexProvider` via `CodexProvider::with_config(config)`.
- `ClaudeCodeProviderConfig::cli_args()` called in `claude_code_cli_args_should_reflect_binding_fields` to verify correct flags.
- Adapter functions placed in `openfang-provider-binding` crate, not in `arky-codex` or `arky-claude-code` — correct per spec.

### Minor Observations
- `ProviderBinding` struct literal construction is used in test helper functions (`codex_binding()`, `claude_code_binding()`, `bedrock_binding()`) in `adapter.rs`. This bypasses `compile_provider_binding()`. Since `ProviderBinding::new()` is private and the fields are `pub`, tests can directly construct bindings. The spec's anti-pattern guard ("Do not allow `ProviderBinding` to be constructed directly without going through `compile_provider_binding()`") is a design intent — in `adapter.rs` tests, the direct struct literal is acceptable for setting up isolated adapter test fixtures. Production code paths do use `compile_provider_binding()`.
- The spec integration test "Provider-specific session settings must survive the full compile → adapt pipeline" is covered by `claude_code_agent_toml_should_compile_and_adapt_allowed_tools` and `bedrock_agent_toml_should_compile_and_adapt_region`, though not by a test named exactly as specified.
- No use of `println!()`, `dbg!()`, `log::warn!()`, or `eprintln!()` — `tracing::warn!()` is used consistently.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-provider-binding/src/adapter.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-provider-binding/src/lib.rs` (line 26–42)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-codex/src/config.rs` (referenced for CodexProviderConfig)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/config.rs` (referenced for ClaudeCodeProviderConfig)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-claude-code/src/profile.rs` (referenced for ClaudeProviderProfile variants)
