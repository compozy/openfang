## markdown

## status: pending

<task_context>
<domain>agents/control-plane</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task16</dependencies>
</task_context>

# Task 18.0: Agent Control-Plane Surfaces On Top Of ProviderBinding

## Overview

Align `/api/agents` API agent surfaces with the new agent-definition
compile model and runtime projection.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Expose create/get/update/delete/validate/compile consistently through the API.
- Keep operational runtime surfaces aligned with the accepted control-plane model.
</requirements>

## Subtasks

- [ ] 18.1 Implement definition-facing agent API endpoints on top of the compiler.
- [ ] 18.2 Implement runtime projection, session, and message surfaces coherently.
- [ ] 18.3 Add end-to-end tests for the new agent control-plane paths.

## Implementation Details

This task is where the provider-binding work becomes visible through the public
product contract. Only API surfaces are in scope; CLI integration is future work.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- agent compiler/validator
- provider-binding runtime integration

## Deliverables

- Updated `/api/agents` control-plane behavior
- Tests for definition, runtime, and compile flows

## Tests

### Unit Tests (Required)

- [ ] Agent definition mutations use the new compile pipeline.
- [ ] Runtime projection fields are derived consistently.
- [ ] Compile and validate endpoints return stable shapes.

### Integration Tests (Required)

- [ ] Create/get/update/delete/validate/compile flows work end-to-end.
- [ ] Agent runtime/session/message surfaces reflect the new model.
- [ ] API responses are consistent and well-structured.

### Regression and Anti-Pattern Guards

- [ ] Do not keep the old "spawn from raw manifest blob" model as the public contract.
- [ ] Do not couple definition endpoints directly to ephemeral runtime state.
- [ ] Do not add undocumented side channels for internal agents.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Agents are controllable through the accepted public API contract.
- Provider-binding work is visible and usable through API surfaces.

---

## Notes

- This task should land before dispatch/HITL runtime integration.
- CLI agent surfaces (`openfang-cli`) are out of scope and will be addressed as future work.
