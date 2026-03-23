## markdown

## status: pending

<task_context>
<domain>engine/types/contracts</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task1</dependencies>
</task_context>

# Task 5.0: Shared Definition Contract Types

## Overview

Implement the shared lightweight contract language for `input`/`output` on both
agent and workflow definitions. This includes structural kinds (`string`,
`integer`, `number`, `boolean`, `object`, `array`, `any`) and semantic kinds
(`artifact_ref`, `doc_ref`, `issue_ref`, `task_ref`, `task_list`, `run_ref`).
These types are used by the agent compile pipeline (task 16) and the workflow
compile pipeline (task 13).

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- A single shared module defines contract types reusable by agents and workflows.
- The types must support validation of input/output contracts at compile time.
</requirements>

## Subtasks

- [ ] 5.1 Define contract schema types (FieldKind, SemanticKind, ContractField, ContractSchema) in openfang-types.
- [ ] 5.2 Implement validation logic for contract schemas.
- [ ] 5.3 Add tests for contract type parsing, validation, and edge cases.

## Implementation Details

This task defines the contract type system used across the platform for both
agent and workflow definitions. The contract schema must be expressive enough to
describe structured inputs and outputs while remaining lightweight and
serializable. Validation logic should catch malformed contracts early at
definition-compile time rather than at runtime.

### Relevant Files

- `crates/openfang-types/src/contract.rs` (new)
- `tasks/prd-compozy/reset-2026-03-21/DESIGN.md`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- `crates/openfang-kernel/src/workflow.rs`
- `crates/openfang-types/src/lib.rs`

## Deliverables

- Shared contract types module in `openfang-types`
- Validation logic for input/output contracts
- Tests covering all structural and semantic kinds

## Tests

### Unit Tests (Required)

- [ ] All structural kinds (`string`, `integer`, `number`, `boolean`, `object`, `array`, `any`) parse correctly.
- [ ] All semantic kinds (`artifact_ref`, `doc_ref`, `issue_ref`, `task_ref`, `task_list`, `run_ref`) parse correctly.
- [ ] Contract schema validation rejects invalid field combinations.

### Integration Tests (Required)

- [ ] Contract types serialize and deserialize round-trip cleanly.
- [ ] Contract schemas can be embedded in workflow and agent definitions.
- [ ] Validation errors produce actionable messages for definition authors.

### Regression and Anti-Pattern Guards

- [ ] Do not duplicate contract types in agent and workflow modules separately.
- [ ] Do not skip validation for any structural or semantic kind.
- [ ] Do not introduce runtime-only contract checks that bypass compile-time validation.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- A single shared contract module defines all structural and semantic kinds.
- Both agent and workflow compile pipelines can use these types directly.
- Validation catches malformed contracts before runtime.

---

## Prior Implementation Reference

The old TypeScript codebase has contract/schema types that inform the domain vocabulary for this task:

- `~/Dev/compozy/compozy-code/packages/types/` — Shared TypeScript types and generated API contract
- `~/Dev/compozy/compozy-code/packages/sdk/src/schemas/` — SDK schema definitions used by the old client

These show what contract types were used before. The new Rust implementation is more expressive
(structural + semantic kinds), but the old types clarify naming conventions and field expectations.

## Notes

- This task is a prerequisite for both the agent compile pipeline (task 16) and workflow compile pipeline (task 13).
- Keep the contract type system minimal and extensible for future semantic kinds.
