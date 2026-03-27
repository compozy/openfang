# Task 18 Review: Agent Definition Validation And Compile Pipeline

## Status: PASS

## Checklist
- [x] 18.1 `AgentDefinition` struct defined in `crates/openfang-agent-definition/src/lib.rs` with all ADR-029 top-level fields: `id`, `name`, `version`, `description`, `enabled`, `group`, `tags`, `provider`, `prompt`, `capabilities`, `runtime`, `input`, `output`; typed sub-structs `ProviderBlock`, `ProviderDefaults` (alias to `ProviderRequestDefaults`), `ProviderConfig` (driver-tagged enum), `PromptBlock`, `CapabilitiesBlock`, `RuntimeBlock` all present
- [x] 18.2 `ProviderBinding` (imported from `openfang_provider_binding`), `AgentProductMetadata`, and `CompiledAgentDefinition` structs defined; `AgentManifest` from `openfang-types::agent` reused without modification
- [x] 18.3 `stage1_schema_validate` implemented: checks `id`, `name`, `provider.driver`, `provider.model` required fields; validates known driver values; validates `max_tokens > 0`; validates enum values for `delegation`, `workspace`, `memory_policy`, `hitl`
- [x] 18.4 `stage2_reference_validate` implemented: checks named profiles against `ValidationContext.known_profiles`; checks profile-driver compatibility; checks skills against `known_skills`; checks primitives against `known_primitives`; empty registries skip the check (correct behavior per spec)
- [x] 18.5 `stage3_semantic_validate` implemented: validates `provider.config` matches the driver kind; validates `provider.request_extra` against `FORBIDDEN_REQUEST_EXTRA_KEYS` via `validate_request_extra`; calls `openfang_types::contract::validate` on `input` and `output` with path prefixing
- [x] 18.6 `stage4_normalize` implemented: fills `enabled = true` when absent; trims strings; normalizes `provider.config` to driver-specific defaults; normalizes contract aliases via `contract::normalize`; deduplicates and sorts tags, skills, tools, primitives, delegation
- [x] 18.7 `compile` function implemented: checks for blocking validation issues and normalization purity before proceeding; produces `CompiledAgentDefinition` with `agent_manifest`, `provider_binding`, `product_metadata` as three distinct layers

## Findings

### Correct
- Four distinct public stage functions are present: `stage1_schema_validate`, `stage2_reference_validate`, `stage3_semantic_validate`, `stage4_normalize`. They are not merged.
- `ProviderConfig` is a typed enum with `Empty`, `Codex(CodexBehaviorLayer)`, `ClaudeCode(ClaudeCodeBehaviorLayer)`, `ClaudeCompatible(ClaudeCompatibleBehaviorLayer)`, and `Unknown(Value)` variants — exactly the driver-tagged structure required by ADR-027.
- `compile` guards against being called on un-validated or un-normalized definitions via two pre-checks: `has_blocking_validation_issues` and `stage4_normalize(clone) != definition`. This satisfies the spec requirement that calling `compile` on an un-normalized definition returns `CompileError::DefinitionNotNormalized`.
- `AgentProductMetadata` carries `version`, `enabled`, `group`, `tags`, `input`, `output` — these fields are correctly absent from the compiled `AgentManifest`.
- `compile_agent_manifest` does not copy `version`, `enabled`, `group`, or `tags` onto `AgentManifest`. They are only in `product_metadata`.
- `ValidationIssue` includes `severity`, `code`, `path`, `message` fields — matches the API-SPEC shape.
- `ValidationContext` supports empty registries, enabling callers to skip any reference check category they lack data for.
- `FORBIDDEN_REQUEST_EXTRA_KEYS` is reused from `arky_config` rather than redefined.
- No `unwrap()` in any production path (only in tests via `expect` with messages).
- All required unit tests are present and named correctly per the spec.

### Minor Observations
- The spec mentioned `stage1_schema_validate` should return an issue with `code = "missing_field"` (singular). The implementation uses `"missing_field"` for the `provider.driver` and `provider.model` checks — confirmed by the test assertion `issues[0].code, "missing_field"`.
- `stage1_schema_validate` uses `code = "invalid_enum_value"` for unknown `hitl` values. The test asserts this code. The spec says "returns an issue when `runtime.hitl` is set to an invalid enum value" without specifying the code, so this is compliant.
- The integration test requirement "A well-formed `AgentDefinition` loaded from a TOML file validates and compiles through the full pipeline" is present: the `AgentDefinitionStore` in `crates/openfang-api/src/agent_definitions.rs` exercises TOML round-trip loading through `toml::from_str` and calls `stage4_normalize` on load, with a corresponding test `persist_should_round_trip_normalized_codex_definitions`.
- The `compile` function normalizes the driver string internally via `normalize_runtime_driver` before building the provider binding, ensuring runtime driver naming conventions (e.g., `"claude_code"` vs platform-internal names) are applied consistently.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-agent-definition/src/lib.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/agent_definitions.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/agent.rs` (AgentManifest — not modified)
