## markdown

## status: pending

<task_context>
<domain>engine/workflows/compile</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task5,task8</dependencies>
</task_context>

# Task 13.0: Workflow v2 Definition Schema And Compile Pipeline

## Overview

Implement the Workflow v2 definition schema and the validate-normalize-compile
pipeline. This includes the step model with step kinds (`agent`, `primitive`,
`workflow`, `wait_signal`, `start_looper`, `emit_event`, `collect`, `noop`),
flow modes (`sequential`, `fan_out`, `conditional`, `loop`), input/output
contracts using the shared contract types from task 5, and the
`save_as`/`outputs` symbol resolution system. The pipeline takes a file-backed
workflow definition, validates it, normalizes defaults, and compiles it to a
workflow IR suitable for the runtime.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Workflow definitions can be validated and compiled before any run is created.
- The compiled IR is the single input to the workflow runtime.
- Step kinds and flow modes must be extensible without breaking existing definitions.
</requirements>

## Subtasks

- [ ] 13.1 Define Workflow v2 definition types: step model, step kinds, flow modes, input/output contracts.
- [ ] 13.2 Implement the validate-normalize-compile pipeline for workflow definitions.
- [ ] 13.3 Add tests for all step kinds, flow modes, validation errors, and compiled IR shape.

## Implementation Details

The Workflow v2 schema captures the full step model including all step kinds
and flow modes. The compile pipeline runs in three phases: validate (reject
malformed or inconsistent definitions with actionable errors), normalize
(apply defaults and canonical forms), and compile (produce a workflow IR that
is the sole input to the runtime). Symbol resolution for `save_as`/`outputs`
must be checked at compile time to catch dangling references early.

### Relevant Files

- `crates/openfang-types/src/workflow.rs` (new or extended)
- `crates/openfang-kernel/src/workflow.rs`
- `tasks/prd-compozy/reset-2026-03-21/DESIGN.md`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- `crates/openfang-types/src/contract.rs` (from task 5)
- `crates/openfang-kernel/src/workflow.rs`

## Deliverables

- Workflow v2 definition types with all step kinds and flow modes
- Validate-normalize-compile pipeline
- Tests covering all step kinds, flow modes, and error paths

## Tests

### Unit Tests (Required)

- [ ] Each step kind validates and compiles correctly.
- [ ] Invalid combinations rejected with actionable errors.
- [ ] Symbol resolution for `save_as`/`outputs` works.

### Integration Tests (Required)

- [ ] Example workflow files validate and compile end-to-end.
- [ ] Round-trip: definition -> compile -> IR preserves semantics.

### Regression and Anti-Pattern Guards

- [ ] No step kind or flow mode is silently ignored.
- [ ] Pipeline does not accept partially valid definitions.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All step kinds and flow modes are representable in the Workflow v2 schema.
- The validate-normalize-compile pipeline produces a stable IR from valid definitions.
- Invalid definitions are rejected with clear, actionable errors before any run is created.

---

## Prior Implementation Reference

The old TypeScript codebase has workflow/planning schemas and a prompt builder system:

- `~/Dev/compozy/compozy-code/packages/backend/src/db/schema/planning.ts` — Old planning schema (PRDs, techspecs) showing how "work items" were structured before the domain redesign
- `~/Dev/compozy/compozy-code/packages/prompts/` — Prompt builder (`builder.ts`, 52k lines) and formatter (`formatter.ts`, 15k lines) with built-in prompt categories for task execution, review, and subagents

The old model had no durable workflow runtime — the new Workflow v2 is greenfield. But the old
planning schema shows domain naming conventions, and the prompt system shows how step-like
constructs were expressed in the old product.

## Notes

- This task bridges the definition layer (task 5 contracts) and the runtime layer (task 14 durable runs).
- Keep the IR shape stable so that runtime tasks do not need to re-parse definitions.
