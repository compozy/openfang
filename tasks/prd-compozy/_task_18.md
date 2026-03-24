## markdown

## status: completed

<task_context>
<domain>agents/compile</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task5,task10,task11,task12</dependencies>
</task_context>

# Task 18.0: Agent Definition Validation And Compile Pipeline

## Overview

Implement the real `validate -> normalize -> compile` pipeline for
`agent_definition`, including `AgentManifest`, `ProviderBinding`, and
`AgentProductMetadata` as the three distinct output layers of compilation.

The public `agent_definition` shape is frozen in ADR-029. The validation
pipeline must follow the four-stage bounded layered model from ADR-041: schema
validation, reference validation, semantic validation, and normalization.
Compilation produces a `CompiledAgentDefinition` that separates
`AgentManifest` (OpenFang platform base), `ProviderBinding` (typed provider
identity and config, from ADR-027), and `AgentProductMetadata` (version,
enabled, group, tags, input/output contracts) cleanly without collapsing
them into a single blob.

The input and output fields use the shared `ContractNode` type from task 5
(ADR-042). The provider block must be structured with `driver`, `model`,
`profile`, `defaults`, `config`, and optional `request_extra` — not a flat
untyped map. Validation must not boot providers, call the network, or execute
templates (ADR-041).

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- The public `AgentDefinition` struct must match the top-level schema from ADR-029: `id`, `name`, `version`, `description`, `enabled`, `group`, `tags`, `provider`, `prompt`, `capabilities`, `runtime`, `input`, `output`. Triggers and schedules must not be embedded.
- The provider block must expose `driver`, `model`, `profile`, `defaults`, `config`, and optional `request_extra` as distinct typed fields, not a flat or untyped map, per ADR-027.
- The validation pipeline must implement exactly four stages in order: schema validation, reference validation, semantic validation, and normalization (ADR-041). Each stage must be a distinct function or method, not collapsed into one pass.
- Validation must be fully synchronous and must not boot any provider, make any network call, or execute any template. Provider binary resolution and credential checks are explicitly out of scope (ADR-041).
- `provider.config` must be validated as a typed block whose permitted fields depend on the chosen `provider.driver`. Unknown driver values must produce a schema-validation issue. Known drivers (`claude_code`, `codex`, and Claude-compatible wrappers) must validate their config fields specifically.
- `provider.request_extra`, if present, must be validated against the forbidden-key list from `crates/arky-config/src/layered.rs` (`FORBIDDEN_REQUEST_EXTRA_KEYS`) and must not contain credentials, environment variables, or transport infrastructure keys (ADR-027).
- `input` and `output` fields on the `AgentDefinition` must be validated using the `validate` and `normalize` functions from `openfang_types::contract` (task 5, ADR-042).
- The compile step must produce a `CompiledAgentDefinition` with three distinct fields: `agent_manifest: AgentManifest`, `provider_binding: ProviderBinding`, `product_metadata: AgentProductMetadata`. Product metadata (`version`, `enabled`, `group`, `tags`, `input`, `output`) must not be embedded inside `AgentManifest` (ADR-029).
</requirements>

## Subtasks

- [x] 18.1 Define the `AgentDefinition` struct in a new module (e.g., `crates/openfang-types/src/agent_definition.rs` or a new `openfang-definitions` crate) with all top-level fields from ADR-029, the typed `ProviderBlock` struct, `ProviderDefaults`, `ProviderConfig` (as a driver-tagged enum or typed sub-struct), `PromptBlock`, `CapabilitiesBlock`, and `RuntimeBlock`.
- [x] 18.2 Define `ProviderBinding`, and `AgentProductMetadata` structs in the same module or an adjacent `compiled.rs`, ensuring `AgentManifest` from `openfang-types::agent` is reused as-is without modification.
- [x] 18.3 Implement `stage1_schema_validate(def: &AgentDefinition) -> Vec<ValidationIssue>` — checks required fields, known driver values, required provider fields, legal enum values for `delegation`, `workspace`, `memory_policy`, and `hitl`.
- [x] 18.4 Implement `stage2_reference_validate(def: &AgentDefinition, ctx: &ValidationContext) -> Vec<ValidationIssue>` — checks that named profiles exist in the context, that skill names are known if a skill registry is provided, and that any referenced primitive names are known. The `ValidationContext` must be constructible with empty registries so callers can skip reference checks they do not have data for.
- [x] 18.5 Implement `stage3_semantic_validate(def: &AgentDefinition) -> Vec<ValidationIssue>` — checks cross-field compatibility: `provider.profile` compatibility with the selected driver, `output.kind` compatibility with output-specific semantic metadata, bounded validation of `provider.request_extra` against the forbidden-key list, and validation of `input`/`output` using `openfang_types::contract::validate`.
- [x] 18.6 Implement `stage4_normalize(def: AgentDefinition) -> AgentDefinition` — fills defaults (`enabled = true` if absent, `nullable = false` on contract nodes, `open = false` on object contracts), canonicalizes contract aliases via `openfang_types::contract::normalize`, and produces a stable logical representation.
- [x] 18.7 Implement `compile(def: AgentDefinition) -> Result<CompiledAgentDefinition, CompileError>` — assumes the definition is already validated and normalized, maps it into `AgentManifest`, `ProviderBinding`, and `AgentProductMetadata`, and returns `Err` only for structural mapping failures (not for validation issues, which belong in the earlier stages).

## Implementation Details

### Current Codebase State

`crates/openfang-types/src/agent.rs` defines `AgentManifest` with a flat
`ModelConfig` for provider identity (`provider`, `model`, `max_tokens`,
`temperature`, `system_prompt`, `api_key_env`, `base_url`). This is the
existing OpenFang platform base type and must not be changed by this task.
The compile step must map the new structured `AgentDefinition` provider block
into this existing `AgentManifest` shape.

`crates/arky-config/src/layered.rs` defines `ProviderRequestDefaults`,
`FORBIDDEN_REQUEST_EXTRA_KEYS`, and `validate_request_extra`. The
`ProviderBinding` struct and `request_extra` validation in this task should
reuse these constants and patterns rather than duplicating them.

`crates/arky-config/src/validate.rs` shows the existing pattern for multi-issue
collection via `Vec<ValidationIssue>` with a final `if issues.is_empty()`
guard. All four validation stages must follow this pattern.

`crates/arky-provider/src/traits.rs` defines the `Provider` trait and
`ProviderDescriptor`. The `driver` field on `ProviderBinding` should use the
same driver identity strings that the Arky provider registry uses (`claude_code`,
`codex`, `claude_compat`, etc.).

`crates/openfang-runtime/src/drivers/` contains `claude_code.rs`, `codex.rs`,
`anthropic.rs`, `openai.rs`, `gemini.rs`, `copilot.rs`, `qwen_code.rs`,
`fallback.rs`. The set of known driver names for schema validation must be kept
in sync with this directory.

`crates/openfang-kernel/src/registry.rs` defines `AgentRegistry` and provides
`register`, `find_by_name`, `get`, etc. The `ValidationContext` for reference
validation should accept an optional reference to the registry or a snapshot
of known agent names, so reference validation does not need a live kernel
reference.

### What Needs to Be Created

A new module — either `crates/openfang-types/src/agent_definition.rs` or a new
`openfang-definitions` crate — must provide:

- `AgentDefinition`: the public input struct
- `ProviderBlock`, `ProviderDefaults`, `ProviderConfig` (driver-tagged enum with typed variant structs for `claude_code`, `codex`, and a generic `ClaudeCompat` variant)
- `PromptBlock`: `system: Option<String>`, `instructions: Option<String>`, `skills: Vec<String>`
- `CapabilitiesBlock`: `tools: Vec<String>`, `primitives: Vec<String>`, `delegation: Vec<DelegationKind>`, `workspace: WorkspaceKind`, `network: bool`
- `RuntimeBlock`: `autonomous: bool`, `memory_policy: MemoryPolicy`, `hitl: HitlPolicy`
- `ProviderBinding`: `driver: String`, `model: String`, `profile: Option<String>`, `defaults: ProviderDefaults`, `config: ProviderConfig`
- `AgentProductMetadata`: `version: String`, `enabled: bool`, `group: Option<String>`, `tags: Vec<String>`, `input: Option<ContractNode>`, `output: Option<ContractNode>`
- `CompiledAgentDefinition`: `agent_manifest: AgentManifest`, `provider_binding: ProviderBinding`, `product_metadata: AgentProductMetadata`
- `ValidationIssue`: `severity: Severity`, `code: String`, `path: String`, `message: String` (matching the `API-SPEC.md` issue object shape)
- `ValidationContext`: holds optional profile names, skill names, primitive names for reference validation
- `CompileError`: `thiserror`-derived enum

### Integration Points

- `openfang_types::contract` (task 5): `validate` and `normalize` are called in stages 3 and 4 for `input` and `output` fields.
- `arky_config::layered::FORBIDDEN_REQUEST_EXTRA_KEYS` and `validate_request_extra`: reused in stage 3 for `request_extra` validation.
- `crates/openfang-api/src/routes.rs`: the `POST /api/v1/agents/validate` and `POST /api/v1/agents/compile` handlers (task 18) will call the pipeline functions implemented here.
- `crates/openfang-api/src/types.rs`: the API response types for validation and compilation responses must match the `API-SPEC.md` shapes (`valid`, `issues`, `normalized` for validate; `definition_id`, `normalized`, `compiled` for compile).
- `AgentManifest` in `crates/openfang-types/src/agent.rs`: the compile step maps the new `AgentDefinition.provider` and `AgentDefinition.prompt` into the existing `AgentManifest` fields without modifying `AgentManifest` itself.

### Relevant Files

- `crates/openfang-types/src/agent_definition.rs` (new, or new crate)
- `crates/openfang-types/src/agent.rs` (existing `AgentManifest` — read only, do not modify)
- `crates/arky-config/src/layered.rs` (forbidden key list, validation patterns)
- `crates/arky-config/src/validate.rs` (multi-issue collection pattern)
- `crates/arky-provider/src/traits.rs` (provider descriptor pattern)
- `crates/openfang-runtime/src/drivers/` (known driver names)
- `tasks/prd-compozy/docs/DESIGN.md` (sections on Agent Definition Shape, Internal Compilation Model, Definition Validation And Normalization)
- `tasks/prd-compozy/docs/API-SPEC.md` (section 3, Agents — resource shape, compiled response, validation request/response)
- `tasks/prd-compozy/docs/adrs/029-agent-definition-public-schema.md`
- `tasks/prd-compozy/docs/adrs/041-bounded-layered-definition-validation.md`
- `tasks/prd-compozy/docs/adrs/027-provider-specific-agent-configuration.md`
- `tasks/prd-compozy/docs/adrs/042-lightweight-definition-contract-schema.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`
- `tasks/prd-compozy/docs/adrs/040-toml-authoring-json-transport-ir-execution.md`

### Dependent Files

- `crates/openfang-api/src/routes.rs` (task 18 will add validate/compile route handlers that call this pipeline)
- `crates/openfang-api/src/types.rs` (API request/response shapes for agent validate and compile)

## Deliverables

- `AgentDefinition` and all supporting structs
- `CompiledAgentDefinition` with `AgentManifest`, `ProviderBinding`, and `AgentProductMetadata`
- Four-stage validation pipeline functions
- `compile` function
- Tests covering valid definitions, all validation failure modes, and compile output correctness

## Tests

### Unit Tests (Required)

- [x] `stage1_schema_validate` returns zero issues for a minimal valid `AgentDefinition` with required fields populated (`id`, `name`, `provider.driver`, `provider.model`).
- [x] `stage1_schema_validate` returns an issue with `code = "missing_field"` and `path = "provider.driver"` when `driver` is absent.
- [x] `stage1_schema_validate` returns an issue with `code = "unknown_driver"` when `provider.driver` is set to an unrecognized string such as `"unknown_provider"`.
- [x] `stage1_schema_validate` returns an issue when `runtime.hitl` is set to an invalid enum value.
- [x] `stage2_reference_validate` returns an issue when `provider.profile` names a profile not present in `ValidationContext.known_profiles`.
- [x] `stage2_reference_validate` returns zero issues when `ValidationContext` has empty registries (no known profiles, no known skills), even if the definition references profiles and skills — unknown references are only flagged when the context has data to check against.
- [x] `stage3_semantic_validate` returns an issue when `provider.request_extra` contains a forbidden key such as `"api_key"` or `"env"`.
- [x] `stage3_semantic_validate` returns an issue when `output.kind = "artifact_ref"` is used without any `artifact_type` metadata (if the semantic rule requires it).
- [x] `stage3_semantic_validate` calls `openfang_types::contract::validate` on `input` and `output` and propagates any contract validation issues with path prefix `"input"` or `"output"`.
- [x] `stage4_normalize` fills `enabled = true` when the field is absent from the input definition.
- [x] `stage4_normalize` resolves `"text"` alias in `input.kind` to `ContractKind::String` via `openfang_types::contract::normalize`.
- [x] `compile` returns a `CompiledAgentDefinition` where `product_metadata.group` and `product_metadata.tags` match the source definition and are not present on `agent_manifest`.
- [x] `compile` maps `provider.driver` and `provider.model` onto `provider_binding.driver` and `provider_binding.model` without loss.
- [x] A full pipeline call (`stage1` -> `stage2` -> `stage3` -> `stage4` -> `compile`) on the PRD writer definition from `API-SPEC.md` section 3 produces zero issues and a non-empty `CompiledAgentDefinition`.

### Integration Tests (Required)

- [x] A well-formed `AgentDefinition` loaded from a TOML file (using `toml::from_str`) validates and compiles through the full pipeline without issues.
- [x] A definition with a malformed provider block (`driver` set to empty string) fails at stage 1 before reaching stage 3.
- [x] A definition whose `input` contract has a structural violation (e.g., `required` references a field not in `fields`) fails at stage 3 with a descriptive issue path.
- [x] A definition with multiple independent violations (missing `id`, unknown driver, forbidden `request_extra` key) produces all three issues in a single combined validation run across the stages.
- [x] `compile` called on an un-normalized definition (before `stage4_normalize`) produces a `CompileError` rather than silently producing a malformed output — or alternatively, `compile` internally normalizes first; either behavior must be documented and tested explicitly.

### Regression and Anti-Pattern Guards

- [x] No provider binary is executed, no tokio runtime is required, and no network socket is opened during any validation stage — verified by running the unit tests without any environment variables or network access.
- [x] `compile` must not accept a raw deserialized `AgentDefinition` that has not been through normalization without either normalizing internally or returning an error — no silent half-compiled manifests.
- [x] Product metadata fields (`version`, `enabled`, `group`, `tags`, `input`, `output`) must not appear on the compiled `AgentManifest` — verified by asserting the `AgentManifest` fields in compile output tests.
- [x] `ValidationIssue` must include both `code` and `path` fields — no free-form string-only error messages.
- [x] No `unwrap()` in production code paths across all four stages and `compile`.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- `AgentDefinition` parses from both TOML and JSON representations of the full PRD writer example from `API-SPEC.md` section 3 without errors.
- All four validation stages are distinct named functions; no stage is merged into another.
- `compile` produces a `CompiledAgentDefinition` with all three layers populated for any valid, normalized definition.
- Provider metadata and product metadata are cleanly separated in the compile output — `AgentManifest` carries only the OpenFang platform fields, `ProviderBinding` carries the driver identity and typed config, `AgentProductMetadata` carries version/enabled/group/tags/contracts.
- All validation issues include `severity`, `code`, `path`, and `message`, matching the `API-SPEC.md` issue object shape.
- Zero clippy warnings, zero test failures.

---

## Prior Implementation Reference

The old TypeScript codebase has agent definition and prompt construction patterns:

- `~/Dev/compozy/compozy-code/packages/prompts/` — Prompt builder system with structured prompt categories (task execution, review, oracle, debug, subagents)
- `~/Dev/compozy/compozy-code/packages/prompts/builder.ts` — 52k-line prompt builder showing how agent capabilities, skills, and instructions were composed

The old model composed agent definitions at prompt-build time. The new model separates definition
(validate/normalize/compile) from execution. The old prompt builder shows what fields and capabilities
agents needed in practice.

## Notes

- This task finishes the model we already froze in the docs.
- The four-stage pipeline boundary is a hard requirement from ADR-041 — do not collapse stages even if they seem small individually.
- The `ValidationContext` struct must be designed so that task 18 can construct it from live kernel state (registry snapshots) and pass it into the pipeline without the pipeline itself holding kernel references.
