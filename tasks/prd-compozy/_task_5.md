## markdown

## status: completed

<task_context>
<domain>engine/types/contracts</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 5.0: Shared Definition Contract Types

## Overview

Implement the shared lightweight contract language for `input`/`output` on both
agent and workflow definitions. This includes structural kinds (`string`,
`integer`, `number`, `boolean`, `object`, `array`, `any`) and semantic kinds
(`artifact_ref`, `doc_ref`, `issue_ref`, `task_ref`, `task_list`, `run_ref`).
These types are used by the agent compile pipeline (task 18) and the workflow
compile pipeline (task 14).

The contract schema is deliberately narrow, as decided in ADR-042. It is
inspired by JSON Schema but must not adopt full JSON Schema as the authoring
format. The canonical form must be expressible in both TOML and JSON, per
ADR-040, and must produce stable, validated output that the IR execution layer
can depend on without further schema inference.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- A single shared module in `openfang-types` defines all contract types reusable by both agents and workflows, with no duplication across definition families (ADR-042).
- The canonical contract node supports exactly the fields defined in ADR-042: `kind`, `description`, `nullable`, plus `fields`/`required`/`open` for objects and `items` for arrays.
- Structural kinds (`string`, `integer`, `number`, `boolean`, `object`, `array`, `any`) and semantic kinds (`artifact_ref`, `doc_ref`, `issue_ref`, `task_ref`, `task_list`, `run_ref`) are defined as a single closed enum, not as open strings.
- Normalization accepts the convenience aliases `text` -> `string` and `json` -> `any` during deserialization and produces only canonical kinds in the normalized form (ADR-042).
- Object contracts default to `open = false`; any definition that needs an open-ended JSON bag must say so explicitly via `kind = "any"` or `kind = "object"` with `open = true` (ADR-042).
- Semantic kinds may carry kind-specific metadata fields (`artifact_type` for `artifact_ref`, `doc_type` for `doc_ref`) represented as optional typed fields on the contract node, not as an untyped JSON blob.
- Validation must be callable at definition-compile time without provider boot, network calls, or template execution, in conformance with the bounded validation rule from ADR-041.
- The contract type system must be serializable to and from both JSON and TOML with stable round-trips, since definitions are file-backed in TOML and transported as JSON per ADR-040.
</requirements>

## Subtasks

- [x] 5.1 Create `crates/openfang-types/src/contract.rs` with the `ContractKind` enum covering all structural and semantic kinds, the `ContractNode` struct with its kind-conditional fields, and the `ContractSchema` type alias or newtype wrapping the top-level contract node.
- [x] 5.2 Implement a `normalize` function (or method on `ContractNode`) that resolves alias kinds (`text`, `json`) into their canonical forms and sets structural defaults (`open = false` on objects, `nullable = false` when absent).
- [x] 5.3 Implement a `validate` function (or `ContractNode::validate`) that enforces structural rules: `fields` and `required` may only appear on `object` kind, `items` may only appear on `array` kind, `artifact_type` / `doc_type` appear only on the appropriate semantic kinds, and `required` field names must be keys present in `fields`.
- [x] 5.4 Add `ContractValidationError` to the crate's error type or as a standalone `thiserror`-derived enum, with variants covering each distinct validation failure mode (unknown-kind alias, misplaced field, missing required key reference, etc.).
- [x] 5.5 Register `pub mod contract;` in `crates/openfang-types/src/lib.rs` and ensure the module is re-exported at the crate root under a stable public path.
- [x] 5.6 Verify that the existing `crates/openfang-kernel/src/workflow.rs` workflow step types and `AgentManifest` in `crates/openfang-types/src/agent.rs` can reference the new `ContractNode` type for their `input`/`output` fields without circular dependency.
- [x] 5.7 Write all tests (see Tests section) in `crates/openfang-types/src/contract.rs` under a `#[cfg(test)]` block, using `pretty_assertions::assert_eq` throughout.

## Implementation Details

### Current Codebase State

`crates/openfang-types/src/lib.rs` currently exposes modules for `agent`,
`approval`, `capability`, `comms`, `config`, `error`, `event`,
`manifest_signing`, `media`, `memory`, `message`, `model_catalog`, `scheduler`,
`serde_compat`, `taint`, `tool`, `tool_compat`, and `webhook`. There is no
`contract` module yet.

`crates/openfang-types/src/agent.rs` defines `AgentManifest` with flat
`ModelConfig` for the provider. It has no `input` or `output` contract fields
today. Those fields need to be added as `Option<ContractNode>` once this task
lands, but the wiring into `AgentManifest` belongs to task 18, not here.

`crates/openfang-kernel/src/workflow.rs` defines `WorkflowStep` and `Workflow`
structs for the existing in-memory workflow engine. These have no contract types
for `input`/`output` today. The new `ContractNode` type needs to be importable
from `openfang-types` by the kernel without adding a reverse dependency.

`crates/arky-config/src/layered.rs` shows the existing pattern for layered
validation with `ValidationIssue` structs returned as a `Vec` rather than
`Result<_, Vec<_>>` so that all issues can be collected in a single pass. The
contract validator should follow the same pattern: collect all issues rather than
failing on the first error.

### What Needs to Be Created

`crates/openfang-types/src/contract.rs` is a new file. It must contain:

- `ContractKind`: a `#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]` enum with `#[serde(rename_all = "snake_case")]`. Variants cover all seven structural kinds and all six semantic kinds. Normalization of aliases (`text`, `json`) should happen via a `#[serde(deserialize_with = ...)]` helper or in the `normalize` step, not by adding alias variants to the enum itself.
- `ContractNode`: a `#[derive(Debug, Clone, Serialize, Deserialize)]` struct with `#[serde(default)]` where appropriate. Fields: `kind: ContractKind`, `description: Option<String>`, `nullable: bool` (default false), `fields: Option<IndexMap<String, ContractNode>>` or `HashMap`, `required: Vec<String>` (default empty), `open: bool` (default false), `items: Option<Box<ContractNode>>`, `artifact_type: Option<String>`, `doc_type: Option<String>`.
- `fn normalize(node: ContractNode) -> ContractNode`: resolves defaults and canonicalizes aliases recursively over `fields` and `items`.
- `fn validate(node: &ContractNode) -> Vec<ContractValidationIssue>`: collects all structural violations.
- `ContractValidationIssue`: a struct with `path: String` and `message: String`, mirroring the `ValidationIssue` pattern from `arky-config`.

### Integration Points

- `crates/openfang-types/src/lib.rs`: add `pub mod contract;`
- Task 18 (`_task_18.md`): the agent compile pipeline imports `ContractNode` from `openfang-types::contract` and uses `validate` + `normalize` during the schema validation and normalization stages.
- Task 14 (workflow compile pipeline): same import path.
- `API-SPEC.md` section 2 defines the canonical JSON wire shape for contract nodes; the `ContractNode` struct must serialize to exactly that shape.

### Relevant Files

- `crates/openfang-types/src/contract.rs` (new)
- `crates/openfang-types/src/lib.rs` (add module registration)
- `crates/arky-config/src/layered.rs` (validation issue pattern reference)
- `tasks/prd-compozy/docs/DESIGN.md` (section on Shared Definition Contract Schema)
- `tasks/prd-compozy/docs/API-SPEC.md` (section 2, Definition Contract Schema)
- `tasks/prd-compozy/docs/adrs/042-lightweight-definition-contract-schema.md`
- `tasks/prd-compozy/docs/adrs/041-bounded-layered-definition-validation.md`
- `tasks/prd-compozy/docs/adrs/040-toml-authoring-json-transport-ir-execution.md`

### Dependent Files

- `crates/openfang-kernel/src/workflow.rs` (will reference `ContractNode` for step input/output)
- `crates/openfang-types/src/lib.rs` (module registration)

## Deliverables

- New `contract` module in `crates/openfang-types/src/contract.rs` with all contract types, normalization, and validation
- `ContractValidationIssue` type for structured validation error reporting
- Module registered at `openfang_types::contract`
- Full test suite covering all structural and semantic kinds, normalization aliases, validation failures, and round-trip serialization

## Tests

### Unit Tests (Required)

- [x] Each of the seven structural kinds (`string`, `integer`, `number`, `boolean`, `object`, `array`, `any`) deserializes correctly from a JSON `{"kind": "..."}` node.
- [x] Each of the six semantic kinds (`artifact_ref`, `doc_ref`, `issue_ref`, `task_ref`, `task_list`, `run_ref`) deserializes correctly from a JSON `{"kind": "..."}` node.
- [x] Alias kind `"text"` normalizes to `ContractKind::String` after `normalize()` is called; the raw deserialized node must reflect the alias and the normalized node must reflect the canonical kind.
- [x] Alias kind `"json"` normalizes to `ContractKind::Any` after `normalize()`.
- [x] An `object` node with no explicit `open` field defaults to `open = false` after normalization.
- [x] A `ContractNode` with `kind = "string"` and a `fields` map present fails validation with a `ContractValidationIssue` indicating misplaced `fields`.
- [x] A `ContractNode` with `kind = "array"` and no `items` field passes validation (items is optional for array contracts).
- [x] A `ContractNode` with `kind = "object"`, `required: ["missing_key"]`, and a `fields` map that does not contain `"missing_key"` fails validation with a descriptive issue at the correct path.
- [x] An `artifact_ref` node with `artifact_type: Some("prd")` serializes to `{"kind":"artifact_ref","artifact_type":"prd"}` and round-trips correctly.
- [x] A nested `object` contract with two `fields` entries each containing their own `ContractNode` round-trips through JSON serialization without data loss.

### Integration Tests (Required)

- [x] A `ContractNode` representing the SDLC workflow input contract from `API-SPEC.md` (object with `issue_id: string`, `required: ["issue_id"]`, `open: false`) deserializes from JSON, validates with zero issues, and serializes back to identical JSON.
- [x] A `ContractNode` representing the PRD writer agent output contract from `API-SPEC.md` (`kind = "artifact_ref"`, `artifact_type = "prd"`) deserializes from JSON, validates with zero issues, and round-trips.
- [x] A deeply nested contract with `kind = "object"`, a field of `kind = "array"` whose `items` is `kind = "object"` validates successfully and normalizes all nested defaults.
- [x] `validate()` called on a contract node with two independent violations collects both issues in a single call (no early-exit).
- [x] A TOML-serialized contract (using `toml::to_string` and `toml::from_str`) round-trips to the same `ContractNode` value as the JSON round-trip.

### Regression and Anti-Pattern Guards

- [x] The `contract` module must not define any duplicate kind types in agent-specific or workflow-specific modules — the single `ContractKind` enum in `contract.rs` is the only definition.
- [x] `ContractKind` must not have `Text` or `Json` variants — normalization of those aliases must happen at the parse or normalize layer, not by adding them to the enum.
- [x] The `validate` function must not make any external calls, must not boot providers, and must not depend on any tokio async runtime — it must be a pure synchronous function.
- [x] No `unwrap()` calls in production code paths; use `?` or `expect()` with a message.
- [x] `fields` must use a deterministic iteration order in tests — use `IndexMap` or sort before asserting.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- `openfang_types::contract::ContractNode` is importable by any crate in the workspace without circular dependencies.
- All seven structural kinds and all six semantic kinds are represented and round-trip through both JSON and TOML serialization.
- Normalization converts aliases to canonical forms and fills structural defaults deterministically.
- Validation collects all issues in a single pass and returns an empty `Vec` for all valid contract examples from `API-SPEC.md`.
- `ContractValidationIssue` includes a structured `path` field so callers can surface actionable error messages to definition authors.
- Zero clippy warnings and zero test failures on `cargo test --workspace`.
- No contract type definitions exist outside `crates/openfang-types/src/contract.rs`.

---

## Prior Implementation Reference

The old TypeScript codebase has contract/schema types that inform the domain vocabulary for this task:

- `~/Dev/compozy/compozy-code/packages/types/` — Shared TypeScript types and generated API contract
- `~/Dev/compozy/compozy-code/packages/sdk/src/schemas/` — SDK schema definitions used by the old client

These show what contract types were used before. The new Rust implementation is more expressive
(structural + semantic kinds), but the old types clarify naming conventions and field expectations.

## Notes

- This task is a prerequisite for both the agent compile pipeline (task 18) and workflow compile pipeline (task 14).
- Keep the contract type system minimal and extensible for future semantic kinds.
- ADR-042 explicitly prohibits adopting full JSON Schema features (`$ref`, `$defs`, `oneOf`, `anyOf`, `patternProperties`). Do not add these even as optional fields.
- If a JSON Schema projection is needed for tool interop, it must be generated as a derived output during compile, never stored as the canonical type.
