## markdown

## status: pending

<task_context>
<domain>engine/triggers/runtime</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task13,task21,task27</dependencies>
</task_context>

# Task 29.0: Trigger And Event Ingress

## Overview

Implement the trigger v2 definition model, the event ingress pipeline, and the
trigger-to-action matching system. Triggers connect external events to actions
(`agent_message`, `workflow_start`, `workflow_signal`). The event ingress
endpoint (`POST /api/events`) receives events, matches them against enabled
triggers, and dispatches the target actions. Trigger definitions are file-backed
and support validate/compile/enable/disable/test operations.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Triggers can match events by type, source, and payload patterns.
- Enabled triggers fire their target actions on match.
- Disabled triggers are inert.
- The test endpoint validates matching without executing the action.
</requirements>

## Subtasks

- [ ] 29.1 Define trigger v2 types (match patterns, target kinds, enable/disable state) and implement CRUD endpoints at `/api/triggers`.
- [ ] 29.2 Implement event ingress at `POST /api/events` and the trigger-matching pipeline.
- [ ] 29.3 Add tests for trigger CRUD, event matching, target dispatch, enable/disable, and the test endpoint.

## Implementation Details

Trigger definitions use the match/target model described in DESIGN.md section 22
and API-SPEC.md section 5. Match fields include `event`, `source`, `contains`,
and `filters`. Supported target kinds are `agent_message`, `workflow_start`, and
`workflow_signal`.

Event ingress follows API-SPEC.md section 8. The `POST /api/events` endpoint
accepts an event payload with optional `idempotency_key` and `occurred_at`,
matches it against all enabled triggers, and dispatches the resolved target
actions. The response includes `matched_triggers` and an `effects` summary.

The `POST /api/triggers/{id}/test` endpoint accepts a synthetic event and
returns the match result and resolved target without executing the action.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `crates/openfang-kernel/src/trigger.rs` (new)
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`
- `tasks/prd-compozy/reset-2026-03-21/DESIGN.md`

### Dependent Files

- `crates/openfang-types/src/workflow.rs`
- `crates/openfang-kernel/src/workflow.rs`

## Deliverables

- Trigger v2 definition types and CRUD endpoints
- Event ingress endpoint with matching pipeline
- Tests for all trigger operations and event matching scenarios

## Tests

### Unit Tests (Required)

- [ ] Match pattern validation accepts valid event/source/contains/filters combinations.
- [ ] Target kind resolution correctly maps to `agent_message`, `workflow_start`, and `workflow_signal`.
- [ ] Enable/disable state transitions update trigger runtime state correctly.

### Integration Tests (Required)

- [ ] E2E event -> trigger -> action flow dispatches the correct target action.
- [ ] Disabled trigger does not fire when a matching event is received.
- [ ] Test endpoint returns match result without executing the action.

### Regression and Anti-Pattern Guards

- [ ] No event is silently dropped without matching against all enabled triggers.
- [ ] No trigger fires when disabled.
- [ ] Do not conflate trigger matching with HITL or in-step interaction semantics.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Trigger v2 definitions are fully operational with CRUD, enable/disable, and test.
- Event ingress pipeline matches events and dispatches target actions reliably.
- The test endpoint provides safe validation of trigger matching behavior.

---

## Notes

- This task implements DESIGN.md sections 19 (Trigger v2) and 22 (Trigger v2 Public Schema) and API-SPEC.md sections 5 (Triggers) and 8 (Event Ingress).
- CLI commands for trigger and event management are deferred to future work (do not touch openfang-cli).
