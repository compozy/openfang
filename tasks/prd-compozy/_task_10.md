## markdown

## status: pending

<task_context>
<domain>providers/arky/config</domain>
<type>implementation</type>
<scope>configuration</scope>
<complexity>high</complexity>
<dependencies>task4</dependencies>
</task_context>

# Task 10.0: Provider Layering For Workspace, Profiles, And Agent Config

## Overview

Implement the provider-layering model for Compozy using the internal Arky crates:
installation/workspace config, named profiles, and per-agent config.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Per ADR-043, provider configuration must be split into exactly three tiers: (1) installation/workspace configuration owns credentials, binary paths, transport setup, environment maps, and lifecycle plumbing; (2) named profiles own reusable behavior presets (model defaults, portable request defaults, typed provider behavior); (3) per-agent configuration owns driver/model selection, agent-local overrides, and typed behavior settings appropriate at agent level.
- Per ADR-027, the public `agent_definition` provider block must expose `provider.driver`, `provider.model`, `provider.profile`, `provider.defaults`, `provider.config`, and optionally `provider.request_extra`. No other provider fields may appear at the agent level without explicit justification.
- The `arky-config` crate's `layered.rs` module (already present in `openfang/crates/arky-config/src/layered.rs`) contains the core types: `ProviderBehaviorLayer`, `ProviderRequestDefaults`, `CodexBehaviorLayer`, `ClaudeCodeBehaviorLayer`, `ClaudeCompatibleBehaviorLayer`, `ResolvedAgentProviderConfig<TInstall>`, and `validate_request_extra()`. This task must extend and integrate these types rather than replacing them.
- Validation must enforce the layer boundary: installation/workspace settings (`binary`, `env`, `credentials`, `client_name`, `client_version`, `transport`, `startup_timeout`, `shared_app_server_key`, `cache_dir`, etc.) must be rejected if they appear in agent-level or profile-level config blocks. The `FORBIDDEN_REQUEST_EXTRA_KEYS` list in `layered.rs` already seeds this; it must be extended to cover the full classification rule from ADR-043.
- `provider.request_extra` must remain a constrained escape hatch: request-level only, bounded to `MAX_REQUEST_EXTRA_ENTRIES` (32) entries and `MAX_REQUEST_EXTRA_DEPTH` (4) nesting levels. The `validate_request_extra()` function in `layered.rs` already implements this. Do not relax these limits.
- Named profiles must be stored in workspace config (e.g., `~/.compozy/config.toml` under a `[profiles.<name>]` section) and must compile into `ProviderProfileConfig` (driver, model, defaults, config). Agent definitions reference profiles by name via `provider.profile`.
- The three-tier merge order must be deterministic: workspace install config is the base, profile overrides install defaults, agent config overrides profile. The `ProviderBehaviorLayer::merge()` and `ProviderRequestDefaults::merge()` methods in `layered.rs` already implement correct merge semantics.
- Keep infrastructure-level provider plumbing (binary paths, env maps, credential wiring, timeout tuning) out of agent definition files. These live only in workspace/install config backed by the `ProviderConfig` struct in `arky-config/src/loader.rs`.
</requirements>

## Subtasks

- [ ] 10.1 Audit the existing `arky-config/src/layered.rs` in the openfang workspace and map which types already cover the three-tier model. Identify gaps: `ResolvedAgentProviderConfig<TInstall>` uses a generic `TInstall` type parameter for the install layer — determine what concrete type `TInstall` should be for Compozy's workspace install layer (likely a projection of `ProviderConfig` from `loader.rs` minus credential fields).
- [ ] 10.2 Extend `arky-config/src/loader.rs` to support named provider profiles under a new `[workspace.profiles]` table. The `ArkyConfig` struct currently holds `workspace`, `providers`, and `agents` — add a `profiles: BTreeMap<String, ProviderProfileConfig>` field and wire it through `ArkyConfigBuilder`, `PartialArkyConfig`, and `ConfigLoader::load()`. The `PartialProviderProfileConfig` struct in `layered.rs` is already the input shape.
- [ ] 10.3 Add `ProviderProfileConfigBuilder` to the public API of `arky-config`. It must expose methods for `driver()`, `model()`, `defaults()` (accepting `ProviderRequestDefaults`), and `config()` (accepting `PartialProviderBehaviorConfig`). Wire it through `ArkyConfigBuilder::profile()`.
- [ ] 10.4 Extend `validate_config()` in `arky-config/src/validate.rs` to validate profiles: each profile must have a non-empty `driver`, its `config` block must match the declared driver's expected namespace via `PartialProviderBehaviorConfig::finalize_for_driver()`, and `validate_defaults()` must be run on the profile's `defaults` block.
- [ ] 10.5 Extend `validate_config()` to validate agent-level provider config: reject any agent whose `provider.config` namespace mismatches the resolved driver (after profile lookup). Confirm that the `finalize_for_driver()` logic in `PartialProviderBehaviorConfig` is called with the agent's effective driver string.
- [ ] 10.6 Add a `resolve_agent_provider()` function (or method on `ArkyConfig`) that takes an agent name and returns a `ResolvedAgentProviderConfig<ProviderConfig>`: merge workspace install config, profile defaults, and agent overrides in the correct tier order, applying `ProviderBehaviorLayer::merge()` and `ProviderRequestDefaults::merge()` at each boundary.
- [ ] 10.7 Write tests covering all precedence and boundary cases (see Tests section). Place them in `arky-config/src/layered.rs` and `arky-config/src/loader.rs` as inline `#[cfg(test)]` modules.

## Implementation Details

### Current State of `arky-config` in the OpenFang Workspace

The openfang copy of `arky-config` (`/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/`)
already has a `layered.rs` module that does not exist in the upstream Arky workspace. This module
defines the core layering infrastructure:

- `ProviderRequestDefaults` — `{ max_tokens: Option<u32>, reasoning_effort: Option<ReasoningEffort> }` with `merge()` semantics (overlay wins).
- `CodexBehaviorLayer` — partial, optional Codex agent-level fields: `sandbox_mode`, `sandbox_network_access`, `include_plan_tool`, `resume_last`, `web_search`, `rmcp_client`, `reasoning_summary`, `model_verbosity`.
- `ClaudeCodeBehaviorLayer` — partial, optional Claude Code agent-level fields: `continue_conversation`, `fork_session`, `additional_directories`, `enable_file_checkpointing`, `allowed_tools`, `disallowed_tools`, `mcp_servers`, `max_budget_usd`, `fallback_model`.
- `ClaudeCompatibleBehaviorLayer` — wrapper-specific overlay with `base: Option<Box<ClaudeCodeBehaviorLayer>>`, `selected_model`, `region`, `project_id`.
- `ProviderBehaviorLayer` — enum discriminating `Codex(CodexBehaviorLayer)`, `ClaudeCode(ClaudeCodeBehaviorLayer)`, `ClaudeCompatible(Box<ClaudeCompatibleBehaviorLayer>)`.
- `PartialProviderBehaviorConfig` — the TOML/env deserialize target: `{ codex, claude_code, claude_compatible }` — calls `finalize_for_driver()` to resolve to a `ProviderBehaviorLayer`.
- `ProviderProfileConfig` — finalized profile: `{ driver, model, defaults, config }`.
- `PartialProviderProfileConfig` — partial profile input: `{ driver, model, defaults, config }`.
- `ResolvedAgentProviderConfig<TInstall>` — fully merged agent view: `{ provider, driver, profile, install: TInstall, model, defaults, config, request_extra }`.
- `validate_request_extra()` — enforces forbidden keys and nesting limits on `request_extra`.
- `normalize_driver()` — normalizes `"claude"` to `"claude-code"`, replaces `_` with `-`.

### What the Upstream `arky-config/src/loader.rs` Currently Has

`ArkyConfig` holds:

- `workspace: WorkspaceConfig` — `{ name, default_provider, data_dir, env }`
- `providers: BTreeMap<String, ProviderConfig>` — each entry: `{ kind, binary, model, args, env }`
- `agents: BTreeMap<String, AgentConfig>` — each entry: `{ provider, model, instructions, max_turns, tools, env }`

The `ProviderConfig` struct carries installation-level fields (binary path, env maps, args). It is
the correct `TInstall` candidate for `ResolvedAgentProviderConfig<TInstall>`. A thin projection
that strips credential-sensitive fields from `ProviderConfig` before handing it to agent-level
code may be needed.

The `WorkspaceConfig` struct does not yet have a `profiles` field. That is the primary addition
this task must make in `loader.rs`.

### Layer Classification Reference (from ADR-043)

**Installation/workspace only (never in agent or profile):**

- `binary`, `args`, `env` (raw environment maps), `credentials`, `api_key`, `auth_token`
- `transport`, `shared_app_server_key`, `startup_timeout`, `scheduler_timeout`, `idle_shutdown_timeout`
- `client_name`, `client_version`, `cache_dir`, `runtime_dir`, `version_args`
- `sanitize_environment`, `experimental_api`, `allow_npx`, `app_server_args`

**Profile-appropriate (reusable behavior presets):**

- `driver`, `model`, `defaults.max_tokens`, `defaults.reasoning_effort`
- `config.codex.*` (behavior flags safe for reuse), `config.claude_code.*` (behavior flags)
- `config.claude_compatible.*` (wrapper-specific overrides)

**Agent-level appropriate (per-agent overrides):**

- `driver`, `model`, `profile` (reference to a named profile)
- `defaults.max_tokens`, `defaults.reasoning_effort`
- All fields in `CodexBehaviorLayer`, `ClaudeCodeBehaviorLayer`, `ClaudeCompatibleBehaviorLayer`
- `request_extra` (constrained escape hatch, request-level only)

### Integration Points with arky-codex and arky-claude-code

The `ResolvedCodexBehaviorConfig` and `ResolvedClaudeCodeBehaviorConfig` types (produced by
`ProviderBehaviorLayer::resolve()`) must map cleanly onto the typed provider config structs in
task 12. Specifically:

- `ResolvedCodexBehaviorConfig` maps to fields on `CodexProviderConfig` in
  `arky-codex/src/config.rs`
- `ResolvedClaudeCodeBehaviorConfig` maps to fields on `ClaudeCodeProviderConfig` in
  `arky-claude-code/src/config.rs`
- `ResolvedClaudeCompatibleBehaviorConfig` maps to wrapper-specific config types in
  `arky-claude-code/src/profile.rs`

### Relevant Files

- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/lib.rs` — public exports, already includes `layered` module
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/layered.rs` — core three-tier types (already exists, extend here)
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/loader.rs` — `ArkyConfig`, `WorkspaceConfig`, `ProviderConfig`, `AgentConfig`, builders
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/merge.rs` — merge helpers for partial configs
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/validate.rs` — `validate_config()`, `check_provider_prerequisites()`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-config/src/error.rs` — `ConfigError`, `ValidationIssue`
- `/Users/pedronauck/Dev/compozy/openfang/crates/arky-protocol/src/request.rs` — `ProviderSettings`, `ReasoningEffort` (used in `ProviderRequestDefaults`)
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/DESIGN.md` — sections on provider block schema and three-tier layering
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/adrs/043-provider-layering-and-constrained-request-extra.md`
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/docs/adrs/027-provider-specific-agent-configuration.md`

### Dependent Files

- `crates/arky-codex/src/config.rs` — `CodexProviderConfig` — consumed by task 12 adapter
- `crates/arky-claude-code/src/config.rs` — `ClaudeCodeProviderConfig` — consumed by task 12 adapter
- `crates/arky-claude-code/src/profile.rs` — wrapper profiles — consumed by task 12 adapter
- Agent-definition compiler (task 11) — consumes `ResolvedAgentProviderConfig` and `ProviderProfileConfig`
- `ProviderBinding` compile layer (task 11) — depends on the output of `resolve_agent_provider()`

## Deliverables

- Extended `arky-config` crate with profile support in `WorkspaceConfig` and `ArkyConfig`
- `ProviderProfileConfigBuilder` in the public API
- `resolve_agent_provider()` function returning `ResolvedAgentProviderConfig<ProviderConfig>`
- Validation in `validate_config()` covering profiles, agent-level provider config, and layer boundaries
- Tests for precedence, merge order, and layer enforcement (see Tests section)

## Tests

### Unit Tests (Required)

- [ ] `provider_request_defaults_merge_should_prefer_overlay` — verify overlay `max_tokens` and `reasoning_effort` win over base values; missing overlay fields fall back to base
- [ ] `codex_behavior_layer_merge_should_prefer_overlay_fields` — verify all eight fields of `CodexBehaviorLayer` merge correctly
- [ ] `claude_code_behavior_layer_merge_should_prefer_overlay_fields` — verify all nine fields of `ClaudeCodeBehaviorLayer` merge correctly
- [ ] `profile_driver_mismatch_should_produce_validation_issue` — a `[profiles.default]` block with `driver = "codex"` and a `claude_code` behavior namespace must fail validation with a clear `ValidationIssue`
- [ ] `agent_driver_mismatch_should_produce_validation_issue` — an agent config with `driver = "claude-code"` and a `codex` behavior namespace must fail with a `ValidationIssue`
- [ ] `forbidden_request_extra_key_should_produce_validation_issue` — each key in `FORBIDDEN_REQUEST_EXTRA_KEYS` (e.g., `"api_key"`, `"binary"`, `"env"`) placed in `request_extra` must produce a `ValidationIssue`
- [ ] `request_extra_nesting_beyond_limit_should_produce_validation_issue` — a deeply nested (5+ levels) `request_extra` object must fail `validate_request_extra()`
- [ ] `request_extra_entry_count_beyond_limit_should_produce_validation_issue` — more than 32 flattened entries in `request_extra` must fail
- [ ] `resolve_agent_provider_should_merge_workspace_profile_agent_in_order` — workspace provider config sets binary path; profile sets model and a Codex behavior flag; agent overrides `max_tokens` — verify the resulting `ResolvedAgentProviderConfig` has all three values correctly merged
- [ ] `profile_defaults_should_override_workspace_and_agent_overrides_profile` — verify the three-tier merge order is strictly workspace < profile < agent

### Integration Tests (Required)

- [ ] A TOML workspace config file with `[profiles.fast-research]` containing `driver = "codex"`, `model = "gpt-4o"`, and a `[config.codex]` block must parse and validate to a `ProviderProfileConfig` with the correct fields
- [ ] A TOML workspace config with a `[profiles.safe-doc-writer]` block and an agent that references `profile = "safe-doc-writer"` must resolve to a `ResolvedAgentProviderConfig` carrying the profile's behavior layer merged with the agent's own config
- [ ] A workspace config missing the profile named in an agent's `provider.profile` must fail validation with a clear error identifying the missing profile name
- [ ] An agent config with `provider.request_extra = { api_key = "..." }` must fail `validate_config()` before compilation
- [ ] Existing `ArkyConfig::from_path()` tests in `loader.rs` must continue passing without modification after the `profiles` field is added

### Regression and Anti-Pattern Guards

- [ ] Do not let `request_extra` carry `binary`, `env`, `api_key`, `auth_token`, `client_name`, `client_version`, `startup_timeout`, `shared_app_server_key`, or any other key in `FORBIDDEN_REQUEST_EXTRA_KEYS`
- [ ] Do not flatten typed provider blocks (`CodexBehaviorLayer`, `ClaudeCodeBehaviorLayer`) into generic top-level agent fields — they must remain namespaced under `provider.config`
- [ ] Do not allow install-level secrets (`binary`, `env` maps, credential fields) to appear in `ProviderProfileConfig` or in agent-level provider config
- [ ] Do not silently drop profile config when merging — a missing profile reference must be a validation error, not a silent no-op
- [ ] Do not permit `provider.request_extra` to carry JSON that has more than `MAX_REQUEST_EXTRA_DEPTH` (4) levels of nesting or more than `MAX_REQUEST_EXTRA_ENTRIES` (32) total entries

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `arky-config` compiles with the new `profiles` support in `ArkyConfig` and `WorkspaceConfig`
- The three-tier merge order (workspace < profile < agent) is enforced and tested
- `validate_config()` rejects layer boundary violations with typed `ValidationIssue` values
- `validate_request_extra()` enforces the forbidden-key list, depth limit, and entry count limit
- `resolve_agent_provider()` returns a fully merged `ResolvedAgentProviderConfig<ProviderConfig>` in one call
- Task 11 (`ProviderBinding` compile layer) can consume `ResolvedAgentProviderConfig` without additional layering logic
- All 10 existing `arky-config` loader tests still pass

---

## Prior Implementation Reference

The old TypeScript codebase has provider layering patterns:

- `~/Dev/compozy/compozy-code/providers/core/src/` — Provider hooks, MCP bridges, tool bridges, token consumption
- `~/Dev/compozy/compozy-code/providers/runtime/src/` — OpenResponses protocol, AI SDK bridge, session/config management

The old model uses a Vercel AI SDK bridge with workspace-level and per-provider config. The new
Arky-based model is richer (three explicit tiers), but the old code shows how workspace vs per-agent
config was separated in practice.

## Notes

- This task should land before dispatch/HITL because provider/session semantics matter there.
- The `layered.rs` file already in the openfang copy of `arky-config` is the correct foundation. Do not replace it — extend it.
- Profile names like `"default"`, `"fast-research"`, `"safe-doc-writer"` from the DESIGN.md are useful as test fixtures.
