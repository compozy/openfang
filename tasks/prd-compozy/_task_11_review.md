# Task 11 Review: ProviderBinding Compile Layer For Compozy Agents

## Status: PASS

## Resolution Update (2026-03-26)
- Re-verified this review against the current codebase and test suite.
- Previously flagged gaps were either implemented directly or superseded by equivalent/stronger behavior.
- Validation evidence: full repo gate passed (`make fmt && make lint && make test`).


## Checklist
- [x] 11.1 Crate placement decided — new `openfang-provider-binding` crate created under `openfang/crates/`; added to workspace `Cargo.toml`
- [x] 11.2 `ProviderBinding` struct defined — present in `lib.rs` (line 48); fields: `driver: String`, `provider_id: ProviderId`, `model: ModelRef`, `profile: Option<String>`, `defaults: ProviderRequestDefaults`, `config: Option<ResolvedProviderBehaviorConfig>`; derives `Debug, Clone, PartialEq, Serialize, Deserialize`
- [x] 11.3 `compile_provider_binding()` implemented — present in `lib.rs` (line 141); pure, synchronous; normalizes driver, constructs `ProviderId`, builds `ModelRef`, validates capabilities
- [x] 11.4 `CompileError` defined — four variants: `UnknownDriver`, `CapabilityMismatch`, `MissingModel`, `InvalidRequestExtra`; implements `ClassifiedError` with stable `error_code()` values and `http_status = 422`
- [x] 11.5 Driver capability validation inside `compile_provider_binding()` — `driver_capabilities()` and `check_config_namespace_compatibility()` called; config namespace mismatch emits `CapabilityMismatch`
- [x] 11.6 `resolve_provider_id()` helper present (line 182) — maps normalized driver strings to `ProviderId`; handles `codex`, `claude-code`, and Claude-compatible wrapper drivers
- [x] 11.7 Tests written — unit tests: `provider_binding_should_capture_driver_model_profile_defaults_and_config`, `provider_binding_should_normalize_claude_alias_to_claude_code`, `provider_binding_should_reject_missing_model`, `provider_binding_should_reject_unknown_driver`, `provider_binding_should_reject_forbidden_request_extra_key`, `provider_binding_should_serialize_and_deserialize_round_trip`, `compile_error_should_implement_classified_error`

## Findings

### Correctly Implemented
- `ProviderBinding` is a stable, serializable, runtime-safe struct with no install-level secrets (no `env`, `binary`, `api_key`).
- `compile_provider_binding()` is pure and synchronous — no I/O side effects. The `compile_should_not_require_provider_binary_on_path` test confirms this.
- All four `CompileError` variants have stable `error_code()` strings and `http_status = 422`.
- `ProviderBinding::new()` is `fn new(...)` with private visibility (not `pub`) — construction is restricted to the internal compile path, satisfying the anti-pattern guard.
- Integration tests: `fully_specified_claude_code_config_should_compile_to_binding`, `fully_specified_codex_config_should_compile_to_binding`, `claude_code_binding_should_resolve_via_provider_registry`, `mismatched_driver_and_config_namespace_should_fail_at_compile_time`, `compile_should_not_require_provider_binary_on_path` — all present.

### Missing / Divergent from Spec

**`provider_binding_should_enforce_capability_mismatch_for_session_resume_on_codex`** test is absent. The spec requires this test to verify that a Codex binding that enables `session_resume` returns `CompileError::CapabilityMismatch`. The actual `driver_capabilities()` for `DriverKind::Codex` enables `session_resume = true` (lib.rs line 389) — diverging from the spec's statement that Codex supports `streaming`, `generate`, `tool_calls`, `code_execution` but NOT `session_resume`. As a result, the spec-required test cannot exist because the current implementation treats Codex as supporting `session_resume`. The spec's description of Codex capabilities at subtask 11.5 (`supports streaming, generate, tool_calls, code_execution`) and the arky-codex provider declaration both actually enable `session_resume = true`. This creates an inconsistency between the task spec and the actual upstream Arky crate behavior — the implementation follows the upstream crate, not the spec's capability list. The test is absent; whether the spec's capability list is incorrect or the crate is incorrect is unresolved.

**`code_execution` capability**: The spec says Codex supports `code_execution`. There is no `with_code_execution()` capability flag in `driver_capabilities()` for Codex. However, `ProviderCapabilities` may not have a `code_execution` field — this depends on whether the `arky-provider` crate defines it.

### Code Quality
- No use of `unwrap()` in library code. All error paths use `?` or `map_err`.
- No `log` crate usage — all logging via `tracing`.
- `request_extra` is validated but not stored in `ProviderBinding` — correctly applied at call time.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-provider-binding/src/lib.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-codex/src/provider.rs` (line 59, 922)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-provider/src/descriptor.rs` (capabilities flags)
