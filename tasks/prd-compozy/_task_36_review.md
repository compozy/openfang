# Task 36 Review: Event Ingress Pipeline And Match Engine

## Status: PASS

## Checklist

- [x] 36.1 `TriggerMatchEngine` (snapshot of enabled triggers) with `match_event` method; evaluates all enabled triggers in order
- [x] 36.2 Action dispatch for all three target kinds: `agent_message`, `workflow_start`, `workflow_signal`
- [x] 36.3 `POST /api/v1/events` registered and implemented with full match/dispatch/response cycle
- [x] 36.4 `POST /api/v1/events/dry-run` registered and implemented (match only, no dispatch)
- [x] 36.5 `max_fires` and `cooldown_secs` enforcement in match engine
- [x] 36.6 `fire_count` and `last_fired_at` updated via `record_successful_fire` after each fire
- [x] 36.7 Match engine reloads trigger set on enable/disable without restart (via `active_compiled` `RwLock`)
- [x] 36.8 Error isolation: dispatch error in one trigger does not prevent evaluation of remaining triggers
- [x] 36.9 Unit tests in `trigger_v2.rs` (10 tests) cover all match-engine evaluation paths
- [x] 36.10 Integration tests in `crates/openfang-api/tests/event_ingress_v1_api_test.rs` (6 tests) cover all required scenarios

## Findings

**Handler** (`routes.rs` line 11174 `post_event_ingress_v1`): Builds a snapshot from the `TriggerV2Engine`, evaluates all active triggers via `TriggerMatchEngine::match_event`, dispatches each matched target, records successful fires, and returns the response with `accepted`, `resource_id`, `event_id`, `matched_triggers`, and `effects` counts. The handler continues processing remaining triggers even when one dispatch fails, satisfying the no-silent-drop requirement.

**Dry-run handler** (`routes.rs` line 11259 `dry_run_event_ingress_v1`): Runs the same match engine evaluation but skips dispatch. Returns `{ would_execute, resolved, effects, explanation }` as specified.

**Match evaluation order** in `TriggerMatchEngine::match_event` (kernel `trigger_v2.rs` line 247): (a) enabled check, (b) max_fires/cooldown check, (c) event exact-match or wildcard, (d) source match if present, (e) contains substring, (f) filters key/value. All conditions must pass.

**`record_successful_fire`** atomically increments `fire_count` and sets `last_fired_at` in the `trigger_runtime` SQLite table after each successful dispatch. This is called by the event ingress handler per matched trigger.

**Integration tests** (`event_ingress_v1_api_test.rs`, 6 tests):
1. `event_ingress_should_start_workflow_and_record_fire_state` — E2E: create trigger, send event, assert `matched_triggers` and `effects.workflow_starts = 1`, verify `fire_count` incremented
2. `disabled_trigger_should_not_fire_without_restart` — create trigger, disable it, send matching event, assert `matched_triggers` is empty
3. `dry_run_should_report_effects_without_dispatching` — dry-run returns `would_execute: true` with effects, no workflow run created
4. `event_ingress_should_dispatch_multiple_matching_targets` — two enabled triggers on same event, both fire, both counted in effects
5. `event_ingress_should_isolate_dispatch_errors` — trigger with invalid target doesn't prevent second trigger from firing
6. `event_ingress_should_dispatch_agent_messages` — `agent_message` target kind dispatched correctly

All 5 required integration test scenarios from the spec are covered (plus an `agent_message` test not explicitly listed but covering the dispatch requirement). The unit tests in `trigger_v2.rs` cover the 6 required unit test scenarios exactly.

**Trigger set reload**: The `active_compiled` `RwLock<HashMap>` is updated synchronously on `enable_trigger`/`disable_trigger` calls. Since the event ingress handler calls `take_match_engine_snapshot()` at request time, it always sees the current enabled set without restart.

No gaps found. All deliverables are present and tested.

## Files Reviewed

- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/_task_36.md`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (lines 11172-11350, `post_event_ingress_v1`, `dry_run_event_ingress_v1`)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs` (lines 419-426)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/trigger_v2.rs` (lines 240-300, 885-930, 1532-1760)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/event_ingress_v1_api_test.rs` (full)
