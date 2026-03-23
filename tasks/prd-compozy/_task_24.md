## markdown

## status: pending

<task_context>
<domain>engine/dispatch/runtime</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task12,task18,task19</dependencies>
</task_context>

# Task 24.0: Dispatch Runtime Integration With Provider-Native Sessions

## Overview

Integrate `agent_dispatch` with the real provider/session semantics from Arky,
especially session-native resume behavior.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Route dispatch execution through the new provider-binding model.
- Preserve enough session identity to support later resume and HITL continuation.
</requirements>

## Subtasks

- [ ] 24.1 Connect dispatch creation to provider-bound agent execution.
- [ ] 24.2 Persist provider/session identifiers needed for resume.
- [ ] 24.3 Add dispatch execution tests across Codex and Claude Code paths.

## Implementation Details

The Arky inspection showed that session identity matters early. This task is
where that becomes part of durable execution.

### Relevant Files

- dispatch runtime modules
- provider-binding integration modules
- `crates/arky-claude-code/src/session.rs`
- `crates/arky-codex/src/provider.rs`

### Dependent Files

- HITL runtime integration
- dispatch API handlers

## Deliverables

- Dispatch runtime flow on top of provider bindings
- Provider-native session linkage captured durably
- Integration tests for dispatch execution

## Tests

### Unit Tests (Required)

- [ ] Dispatch records retain the provider/session data needed for continuation.
- [ ] Dispatch status transitions align with runtime execution events.
- [ ] Invalid provider binding or runtime setup fails cleanly.

### Integration Tests (Required)

- [ ] Dispatch execution works end-to-end for Codex-backed agents.
- [ ] Dispatch execution works end-to-end for Claude Code-backed agents.
- [ ] Restart preserves enough dispatch identity for later resume behavior.

### Regression and Anti-Pattern Guards

- [ ] Do not route durable dispatch through raw provider calls that bypass bindings.
- [ ] Do not fake session resume with stringly runtime hacks.
- [ ] Do not keep dispatch runtime state only in provider-local memory.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Durable dispatch is wired to the real provider layer.
- Session-native semantics are preserved for later recovery and HITL behavior.

---

## Prior Implementation Reference

The old TypeScript codebase shows how provider sessions were managed during dispatch:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/` — Provider tool adapters showing session patterns
- `~/Dev/compozy/compozy-code/providers/runtime/src/session/` — Session management in the old OpenResponses runtime
- `~/Dev/compozy/compozy-code/providers/runtime/src/protocol/` — OpenResponses protocol handling

The old runtime kept session identity in the provider layer. The new model must persist session/provider
identifiers durably in `agent_dispatch` so that resume and HITL continuation work across restarts.

## Notes

- This task is why provider work must land before full dispatch/HITL integration.
