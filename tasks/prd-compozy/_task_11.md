## markdown

## status: pending

<task_context>
<domain>providers/arky/binding</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task10</dependencies>
</task_context>

# Task 11.0: ProviderBinding Compile Layer For Compozy Agents

## Overview

Create the new compile layer that turns Compozy `agent_definition.provider`
data into a concrete `ProviderBinding` suitable for runtime use.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Compile provider identity, settings, and typed config into a stable binding object.
- Keep the compile output separate from raw file-backed agent definitions.
</requirements>

## Subtasks

- [ ] 11.1 Define the `ProviderBinding` internal shape.
- [ ] 11.2 Map layered provider config into `ProviderBinding`.
- [ ] 11.3 Add compile tests for common and invalid provider-binding cases.

## Implementation Details

Arky already has the typed pieces, but not the exact binding layer Compozy
needs for agent-definition compilation.

**Note:** The Arky crates referenced below (e.g. `arky-protocol`, `arky-provider`) are copied from the Arky workspace (`~/Dev/compozy/arky/crates/`) into `openfang/crates/` as part of task 4. The file paths below reference these local copies within the OpenFang workspace.

### Relevant Files

- `crates/arky-protocol/src/request.rs`
- `crates/arky-provider/src/descriptor.rs`
- `crates/arky-provider/src/registry.rs`
- `tasks/prd-compozy/reset-2026-03-21/DESIGN.md`

### Dependent Files

- agent-definition compiler
- provider-specific adapters

## Deliverables

- Internal `ProviderBinding` model
- Compiler from layered provider config into binding
- Tests for binding shape and invalid combinations

## Tests

### Unit Tests (Required)

- [ ] `ProviderBinding` captures driver, model, profile, request defaults, and typed config correctly.
- [ ] Invalid provider identity or config combinations fail clearly.
- [ ] `request_extra` remains bounded and request-level only.

### Integration Tests (Required)

- [ ] Example agent-definition provider blocks compile to expected bindings.
- [ ] Bindings can be resolved through the provider registry.
- [ ] Unsupported provider-driver combinations fail before runtime execution.

### Regression and Anti-Pattern Guards

- [ ] Do not store raw untyped JSON as the main runtime provider contract.
- [ ] Do not mix installation secrets or env maps into the binding.
- [ ] Do not bypass provider capability validation where Arky already has it.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Compozy has a real provider compile target, not only docs.
- Agent-definition compilation can proceed without leaking Arky internals directly into the public schema.

---

## Notes

- This task is the seam between Compozy config surfaces and Arky provider runtime.
