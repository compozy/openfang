## markdown

## status: pending

<task_context>
<domain>agents/compile</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task5,task10,task11,task12</dependencies>
</task_context>

# Task 16.0: Agent Definition Validation And Compile Pipeline

## Overview

Implement the real `validate -> normalize -> compile` pipeline for
`agent_definition`, including `AgentManifest`, `ProviderBinding`, and product
metadata output.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Compile agent definitions into stable internal objects.
- Validate provider config, profiles, groups, tags, input, and output contracts.
</requirements>

## Subtasks

- [ ] 16.1 Implement layered validation for agent definitions.
- [ ] 16.2 Implement normalization and compile output objects.
- [ ] 16.3 Add compile and validation tests across normal and invalid inputs.

## Implementation Details

The output should line up with the accepted `CompiledAgentDefinition` model,
not only a deserialized manifest blob.

### Relevant Files

- agent-definition compiler and validator modules
- `tasks/prd-compozy/reset-2026-03-21/DESIGN.md`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`
- `tasks/prd-compozy/reset-2026-03-21/adrs/041-bounded-layered-definition-validation.md`

### Dependent Files

- `/api/agents` handlers
- CLI agent commands

## Deliverables

- Agent-definition validation pipeline
- Normalized and compiled agent-definition output
- Tests for compile and validation behavior

## Tests

### Unit Tests (Required)

- [ ] Schema/reference/semantic/normalization validation works for agent definitions.
- [ ] Compile output includes `AgentManifest`, `ProviderBinding`, and product metadata.
- [ ] Invalid provider/profile/input/output combinations fail with actionable issues.

### Integration Tests (Required)

- [ ] Example agent definitions validate and compile through the public pipeline.
- [ ] `GET /compiled`-style outputs can be produced from file-backed definitions.
- [ ] Broken agent definitions fail before runtime execution starts.

### Regression and Anti-Pattern Guards

- [ ] Do not boot providers during validation.
- [ ] Do not compile directly from raw deserialized state without normalization.
- [ ] Do not bury product metadata inside `AgentManifest`.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Agent definitions have a real, bounded compile pipeline.
- Provider and product metadata are separated cleanly.

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
