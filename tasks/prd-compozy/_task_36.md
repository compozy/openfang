## markdown

## status: pending

<task_context>
<domain>engine/triggers/runtime</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task35,task19</dependencies>
</task_context>

# Task 36.0: Event Ingress Pipeline And Match Engine

## Overview

Implement the event ingress pipeline and the trigger-to-action matching system. The event ingress
endpoint (`POST /api/v1/events`) receives events, matches them against all enabled triggers, and
dispatches the resolved target actions. The dry-run endpoint (`POST /api/v1/events/dry-run`)
returns what would happen without dispatching. The `TriggerMatchEngine` holds the set of enabled
trigger definitions (from Task 35) and evaluates them in order against incoming events.

This task implements the event ingress surfaces defined in API-SPEC.md section 8 (event ingress)
and the match engine that connects triggers to their target actions. It depends on Task 35 for
the trigger v2 type system and definition CRUD, and on Task 19 for restart recovery awareness
(so the match engine survives restarts with correct trigger state).

<critical>
- **ALWAYS READ** `CLAUDE.md` before start -- **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement the `TriggerMatchEngine` that holds the set of enabled trigger definitions and
  evaluates them in order against an incoming event. Match evaluation: `event` field must match
  exactly (or be absent for wildcard), `source` field is matched if present, `contains` checks
  for substring presence in the serialized payload, and `filters` applies key/value predicates.
- Implement the event ingress pipeline: `POST /api/v1/events` and `POST /api/v1/events/dry-run`.
  The live endpoint accepts an event with optional `idempotency_key` and `occurred_at`, matches it
  against all enabled triggers in sequence, dispatches the resolved target actions, and returns
  `{ accepted, resource_id, status, event_id, matched_triggers, effects }`. The dry-run returns
  `{ would_execute, resolved, effects, explanation }` without dispatching. No event must be
  silently dropped without matching against all enabled triggers.
- Action dispatch for each target kind must be implemented:
  - `agent_message` -> send message to agent
  - `workflow_start` -> create workflow run
  - `workflow_signal` -> signal existing run using the selector
- The match engine must respect enable/disable state from Task 35: a disabled trigger is never
  evaluated, even if its match expression would otherwise match.
- The match engine must respect `max_fires` and `cooldown_secs` from the trigger definition:
  a trigger that has reached its `max_fires` count must not fire; a trigger within its cooldown
  window since `last_fired_at` must not fire.
- The match engine must update `fire_count` and `last_fired_at` in `runtime.db` after each
  successful trigger fire.
- No event must be silently dropped: `POST /api/v1/events` must match against all enabled
  triggers and report all matched trigger IDs in the response; a trigger that errors internally
  must not cause the remaining triggers to be skipped.
</requirements>

## Subtasks

- [ ] 36.1 Implement the `TriggerMatchEngine` struct. It holds a snapshot of enabled trigger
      definitions (loaded from the file-backed store via Task 35) and provides a `match_event`
      method that evaluates all enabled triggers against an incoming event. Match evaluation
      follows this order for each trigger: (a) check `enabled` flag, (b) check `max_fires` and
      `cooldown_secs` constraints, (c) match `event` field (exact match or wildcard if absent),
      (d) match `source` field if present, (e) check `contains` substring in serialized payload,
      (f) apply `filters` key/value predicates. A trigger matches only if all applicable conditions
      pass.

- [ ] 36.2 Implement action dispatch for each target kind. After matching, each resolved target
      action must be dispatched:
      - `agent_message`: send message to the target agent via the agent runtime
      - `workflow_start`: create a new workflow run via the workflow engine
      - `workflow_signal`: signal an existing run using the `selector` (which includes
        `workflow_id` and optional run filters)
      Each dispatch must be logged and its result captured for the response.

- [ ] 36.3 Register `POST /api/v1/events` in the router. The handler accepts an event payload
      with `event`, `source`, `payload`, optional `idempotency_key`, `occurred_at`, and `metadata`.
      It calls the match engine, dispatches resolved actions, updates trigger runtime state
      (`fire_count`, `last_fired_at`), and returns the response with `accepted`, `resource_id`,
      `status`, `event_id`, `matched_triggers`, and `effects` (counts of `workflow_starts`,
      `workflow_signals`, `agent_messages`).

- [ ] 36.4 Register `POST /api/v1/events/dry-run` in the router. The handler runs the match
      engine without dispatching any action. It returns `{ would_execute, resolved, effects,
      explanation }` showing which triggers would match and what actions would be dispatched.

- [ ] 36.5 Implement `max_fires` and `cooldown_secs` enforcement in the match engine. Before
      evaluating match conditions, check the trigger's `fire_count` against `max_fires` (if set)
      and `last_fired_at` against `cooldown_secs` (if set). Triggers that have exhausted their
      fire count or are within the cooldown window are skipped with an appropriate explanation.

- [ ] 36.6 Ensure the match engine reloads its trigger set when triggers are created, updated,
      enabled, or disabled via Task 35's CRUD endpoints. The engine must not require a daemon
      restart to pick up trigger definition changes.

- [ ] 36.7 Add engine-level and integration tests. See the Tests section below.

- [ ] 36.8 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

Event ingress request per API-SPEC.md section 8:

```
{
  "event": "issue.created", "source": "api",
  "payload": { "issue_id": "ISSUE-123" },
  "idempotency_key": "...", "occurred_at": "2026-03-21T14:10:00Z",
  "metadata": { "actor": "system" }
}
```

Event ingress response per API-SPEC.md section 8:

```
{
  "accepted": true, "resource_id": "evt_123", "status": "accepted",
  "event_id": "evt_123",
  "matched_triggers": ["issue-created-start-sdlc"],
  "effects": { "workflow_starts": 1, "workflow_signals": 0, "agent_messages": 0 }
}
```

Event ingress dry-run response per API-SPEC.md section 8:

```
{
  "would_execute": true,
  "resolved": { "event": "issue.created", "source": "api" },
  "effects": { "matched_triggers": [...], "workflow_starts": 1, "workflow_signals": 0, "agent_messages": 0 },
  "explanation": { "matching_mode": "trigger_engine" }
}
```

The match engine must handle errors gracefully: if dispatching one trigger's action fails, the
remaining matched triggers must still be evaluated and dispatched. The response must report both
successful and failed dispatches.

### Relevant Files

- `crates/openfang-api/src/routes.rs` -- add event ingress handlers here
- `crates/openfang-api/src/server.rs` -- router registration; add `/api/v1/events`
- `crates/openfang-kernel/src/trigger_v2.rs` -- v2 trigger types from Task 35
- `crates/openfang-kernel/src/workflow.rs` -- workflow runtime for `workflow_start` dispatch
- `tasks/prd-compozy/docs/API-SPEC.md` -- canonical payload contracts (section 8 for events)
- `tasks/prd-compozy/docs/adrs/019-trigger-v2-with-explicit-targets.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`

### Dependent Files

- `crates/openfang-types/src/workflow.rs` -- workflow types referenced by `workflow_start` and `workflow_signal` targets
- `crates/openfang-kernel/src/workflow.rs` -- workflow runtime, used for action dispatch
- Task 35 -- trigger v2 types and definition CRUD (direct dependency)

## Deliverables

- `TriggerMatchEngine` with match evaluation and action dispatch
- `POST /api/v1/events` endpoint with full match/dispatch/response cycle
- `POST /api/v1/events/dry-run` endpoint with match-only evaluation
- `max_fires` and `cooldown_secs` enforcement
- `fire_count` and `last_fired_at` runtime state updates after each fire
- Trigger set reload on definition changes
- Tests for all event matching scenarios and action dispatch

## Tests

### Unit Tests (Required)

- [ ] Match pattern with `event = "issue.created"` matches an event whose `event` field is exactly
      `"issue.created"` and does not match an event with `event = "issue.updated"`.
- [ ] Match pattern with `contains = "ISSUE-123"` matches an event whose serialized payload
      contains the substring and does not match a payload without it.
- [ ] Match pattern with no `event` field (wildcard) matches any event regardless of its event
      type.
- [ ] `max_fires` enforcement: a trigger with `max_fires = 3` and `fire_count = 3` does not match
      even when the event matches all other conditions.
- [ ] `cooldown_secs` enforcement: a trigger with `cooldown_secs = 60` and `last_fired_at` within
      the last 30 seconds does not match.
- [ ] Target kind resolution correctly maps the three target kinds to their dispatch actions:
      `agent_message` -> agent message send, `workflow_start` -> run creation, `workflow_signal` ->
      signal dispatch.

### Integration Tests (Required)

- [ ] E2E event -> trigger -> action flow: create an enabled `workflow_start` trigger matching
      `event = "issue.created"`, send `POST /api/v1/events` with `event = "issue.created"`, assert
      the response `matched_triggers` includes the trigger ID and `effects.workflow_starts = 1`.
- [ ] A disabled trigger does not fire: create a trigger, disable it, send a matching event via
      `POST /api/v1/events`, assert `matched_triggers` is empty and no workflow run is created.
- [ ] `POST /api/v1/events/dry-run` with a matching event returns `would_execute: true` and the
      correct `effects.matched_triggers` list without dispatching any action.
- [ ] Multiple triggers match the same event: create two enabled triggers matching the same event
      type with different target kinds; send the event and verify both triggers fire and both
      effects are counted.
- [ ] Trigger error isolation: create two triggers where the first has an invalid target (e.g.,
      references a nonexistent workflow); send a matching event and verify the second trigger still
      fires successfully.

### Regression and Anti-Pattern Guards

- [ ] No event is silently dropped: `POST /api/v1/events` must match against all enabled triggers
      and report all matched trigger IDs in the response; a trigger that errors internally must not
      cause the remaining triggers to be skipped.
- [ ] No trigger fires when disabled: verified through both the matching engine unit test and the
      E2E integration test above.
- [ ] Trigger matching must not be conflated with HITL or in-step interaction semantics; triggers
      react to system-level events, not to human interaction prompts.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- `POST /api/v1/events` and `POST /api/v1/events/dry-run` are registered and the match engine
  evaluates all enabled triggers in order for every incoming event.
- Action dispatch correctly executes `agent_message`, `workflow_start`, and `workflow_signal`
  target kinds.
- `max_fires` and `cooldown_secs` are enforced; `fire_count` and `last_fired_at` are updated
  after each fire.
- Event ingress correctly counts `workflow_starts`, `workflow_signals`, and `agent_messages` in
  the effects summary; no event is silently dropped.
- Dry-run returns accurate match results without dispatching any action.
- The match engine reloads trigger definitions without requiring a daemon restart.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- CLI commands for event management are deferred to future work (do not touch openfang-cli).
- This task implements API-SPEC.md section 8 (event ingress).
- The trigger type system and definition CRUD are handled by Task 35. This task focuses on the
  runtime event processing pipeline.
