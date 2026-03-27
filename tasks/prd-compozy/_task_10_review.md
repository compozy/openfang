# Task 10 Review: Provider Layering For Workspace, Profiles, And Agent Config

## Status: PASS

## Checklist
- [x] 10.1 Audited existing `layered.rs` types and identified gaps — evidenced by the implementation
- [x] 10.2 Named provider profiles added to `ArkyConfig` — `profiles: BTreeMap<String, ProviderProfileConfig>` field added to `ArkyConfig` and `PartialArkyConfig` in `loader.rs` (lines 58, 1078); wired through `ArkyConfigBuilder`, `PartialArkyConfig`, and `ConfigLoader::load()`
- [x] 10.3 `ProviderProfileConfigBuilder` added to public API — present in `loader.rs` (line 705) with `driver()`, `model()`, `defaults()`, `config()` methods; `ArkyConfigBuilder::profile()` wired (line 503)
- [x] 10.4 `validate_config()` extended to validate profiles — profile driver mismatch validation present in `validate.rs`; `finalize_for_driver()` called on profile config block; `validate_defaults()` applied
- [x] 10.5 Agent-level provider config validated against resolved driver — `profile_driver_mismatch_should_fail_clearly` test in `validate.rs` confirms cross-validation; `agent_driver_mismatch_should_produce_validation_issue` test in `loader.rs`
- [x] 10.6 `resolve_agent_provider()` function implemented — present in `loader.rs` (line 168); returns `Option<ResolvedAgentProviderConfig<ProviderConfig>>` merging workspace, profile, and agent tiers in correct order using `ProviderBehaviorLayer::merge()` and `ProviderRequestDefaults::merge()`
- [x] 10.7 Tests written — unit and integration tests present in `layered.rs` and `loader.rs`

## Findings

### Correctly Implemented
- Three-tier merge order (workspace < profile < agent) is implemented in `resolve_agent_provider()` and tested by `resolve_agent_provider_should_merge_workspace_profile_agent_in_order` and `profile_defaults_should_override_workspace_and_agent_overrides_profile`.
- `validate_config()` rejects profile driver mismatches and agent-level provider config namespace mismatches with typed `ValidationIssue` values.
- `validate_request_extra()` enforces forbidden keys, depth limit (4), and entry count limit (32) — tested in `layered.rs` with `forbidden_request_extra_key_should_produce_validation_issue`, `request_extra_nesting_beyond_limit_should_produce_validation_issue`, `request_extra_entry_count_beyond_limit_should_produce_validation_issue`.
- Integration tests cover: TOML profile parsing (`profile_table_should_parse_and_validate_to_provider_profile_config`), profile reference resolution (`safe_doc_writer_profile_reference_should_resolve_merged_config`), missing profile validation error (`missing_profile_reference_should_fail_with_profile_name`), `request_extra` with `api_key` fails (`request_extra_api_key_should_fail_validation_before_compilation`).
- All existing `ArkyConfig::from_path()` tests continue to pass (TOML loading test updated to include `profiles: BTreeMap::new()` in the partial config or using defaults).
- `FORBIDDEN_REQUEST_EXTRA_KEYS` list in `layered.rs` covers install-level keys; `is_forbidden_request_extra_key()` enforces the boundary.
- `PartialWorkspaceConfig` also has a `profiles` field (line 1091) matching the workspace config profiles section from the spec.

### Minor Observations
- The spec requires a test named `codex_behavior_layer_merge_should_prefer_overlay_fields` covering all eight fields of `CodexBehaviorLayer`. The test at `layered.rs` line 840 is named exactly that and verifies overlay semantics — matches spec.
- The spec requires a test named `claude_code_behavior_layer_merge_should_prefer_overlay_fields` covering nine fields. Present at line 879 — matches spec.
- No issues found with the layer boundary enforcement or merge order. The implementation is faithful to ADR-043.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/layered.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/loader.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/validate.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/lib.rs`
