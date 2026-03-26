## markdown

## status: completed

<task_context>
<domain>engine/schedules/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task6,task19</dependencies>
</task_context>

# Task 26.0: Schedule Control-Plane Surfaces

## Overview

Implement the full schedule definition CRUD and operational API surfaces under `/api/v1/schedules`.
Schedules are persisted in `runtime.db` (schema from task 6) and connect typed cron expressions
to workflow or agent targets. The API provides CRUD, validate, fork, enable/disable, run-now, and
run-now/dry-run endpoints. The surface stays close to the typed OpenFang cron model per ADR-035
and DESIGN.md section 15, while aligning action payloads with the rest of the Compozy control
plane per ADR-034 and API-SPEC.md section 6.

The current codebase already registers basic `/api/schedules` and `/api/schedules/{id}/run` routes
in `crates/openfang-api/src/server.rs` (lines 297-308). Those routes lack the `/api/v1` prefix,
are missing validate, fork, enable, disable, and run-now/dry-run endpoints, and expose the older
blob-style contract that ADR-023 and ADR-035 explicitly exclude from the product surface. This task
replaces that surface with the typed, Compozy-owned schedule contract.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement the complete schedule CRUD surface at `/api/v1/schedules`: `GET /api/v1/schedules`,
  `POST /api/v1/schedules`, `GET /api/v1/schedules/{id}`, `PUT /api/v1/schedules/{id}`, and
  `DELETE /api/v1/schedules/{id}`. All responses follow the typed schedule resource shape from
  API-SPEC.md section 6 with `schedule`, `action`, and `delivery` blocks.
- Implement `POST /api/v1/schedules/validate` per ADR-038 and ADR-035: validation must accept
  `{ definition, strict, context }` and return `{ valid, issues, normalized }`. Cron expression
  validity must be checked at validation time using the typed cron model from
  `crates/openfang-types/src/scheduler.rs`. Invalid cron expressions are rejected immediately, not
  deferred to fire time.
- Implement `POST /api/v1/schedules/{id}/fork` per API-SPEC.md section 7 with correct `origin` and
  `forked_from` provenance metadata.
- Implement `GET /api/v1/schedules/{id}/runtime` returning the typed runtime status shape
  (`schedule_id`, `enabled`, `last_run`, `next_run`, `last_status`, `consecutive_errors`,
  `one_shot`) per API-SPEC.md section 6.
- Implement `POST /api/v1/schedules/{id}/enable` and `POST /api/v1/schedules/{id}/disable` as
  operational actions. These return the accepted-style envelope (`{ accepted, resource_id, status }`)
  and update the persisted enabled flag in `runtime.db` without deleting the schedule record.
  A disabled schedule must never fire even if its cron expression would otherwise match.
- Implement `POST /api/v1/schedules/{id}/run-now` to bypass the cron schedule and trigger immediate
  execution of the schedule action. The response is the accepted envelope optionally extended with
  `run_id` or `session_id` depending on the action kind.
- Implement `POST /api/v1/schedules/{id}/run-now/dry-run` per ADR-038: returns
  `{ would_execute, resolved, effects, explanation }` without executing the action.
- All list endpoints return `{ items, next_cursor }` with `limit` (default 50, max 200), `cursor`,
  and filters `agent`, `enabled`, `schedule_kind`, `action_kind`, and `q` per ADR-034. Schedules
  do not expose a separate compile endpoint because they map closely to the typed scheduler model
  and do not require a public compilation artifact (ADR-035).
</requirements>

## Subtasks

- [x] 26.1 Register the `/api/v1/schedules` router group in `crates/openfang-api/src/server.rs`,
      replacing the existing `/api/schedules` registration. Implement `GET /api/v1/schedules`
      (paginated list with `agent`, `enabled`, `schedule_kind`, `action_kind`, `q` filters),
      `POST /api/v1/schedules` (create, persisted to `runtime.db`), `GET /api/v1/schedules/{id}`,
      `PUT /api/v1/schedules/{id}`, and `DELETE /api/v1/schedules/{id}`. Create and update must
      validate the cron expression and action before writing.
- [x] 26.2 Implement `POST /api/v1/schedules/validate` per ADR-038. Validation must parse the
      typed `schedule` block (including `kind`, `expr`, and `tz`), validate the cron expression using
      the existing cron types in `crates/openfang-types/src/scheduler.rs`, check action kind is one of
      `system_event`, `agent_turn`, `workflow_run`, `workflow_signal`, check delivery kind is one of
      `none`, `channel`, `last_channel`, `webhook`, and validate target references (workflow ID,
      agent ID). Returns `{ valid, issues, normalized }` with structured issue objects.
- [x] 26.3 Implement `POST /api/v1/schedules/{id}/fork` to produce a user-owned fork with
      `origin.kind = "user"` and a populated `forked_from` block, persisted to `runtime.db`.
- [x] 26.4 Implement `GET /api/v1/schedules/{id}/runtime` backed by the `runtime.db` schedule
      runtime status row for that schedule ID. The response carries `schedule_id`, `enabled`,
      `last_run`, `next_run`, `last_status`, `consecutive_errors`, and `one_shot`.
- [x] 26.5 Implement `POST /api/v1/schedules/{id}/enable` and `POST /api/v1/schedules/{id}/disable`.
      Both update the `enabled` flag in `runtime.db` and notify the scheduler engine so the change
      takes effect immediately without a daemon restart. A newly disabled schedule must be removed from
      the active cron queue before the endpoint returns.
- [x] 26.6 Implement `POST /api/v1/schedules/{id}/run-now` and
      `POST /api/v1/schedules/{id}/run-now/dry-run`. The run-now endpoint dispatches the schedule
      action immediately (bypassing the cron timer) and returns the accepted envelope. The dry-run
      endpoint returns `{ would_execute, resolved: { schedule_id, action }, effects: { schedule_fire },
explanation: { delivery } }` per API-SPEC.md section 6.
- [x] 26.7 Add route-level and handler-level tests. See the Tests section below.

## Implementation Details

Schedules are persisted in `runtime.db`. The typed cron model already present in
`crates/openfang-types/src/scheduler.rs` (`CronJobId`, `CronSchedule`, `CronAction`,
`CronDelivery`, and the constant limits `MAX_JOBS_PER_AGENT`, `MIN_EVERY_SECS`, etc.) provides the
validation foundation. The existing scheduler in `crates/openfang-kernel/src/scheduler.rs` manages
the active cron queue and must be notified when enable/disable state changes.

The full schedule resource shape (API-SPEC.md section 6) is:

```
id, agent, name, enabled, schedule: { kind, expr, tz },
action: { kind, ... }, delivery: { kind },
created_at, runtime_status: { last_run, next_run, last_status, consecutive_errors, one_shot }
```

Action kinds and their payloads (API-SPEC.md section 6):

- `system_event`: `{ kind, event, payload }`
- `agent_turn`: `{ kind, input: { items }, model_override, timeout_secs }` — reuses the agent
  message input item model
- `workflow_run`: `{ kind, workflow_id, input, timeout_secs }`
- `workflow_signal`: `{ kind, signal, selector: { workflow_id }, payload }`

Delivery kinds: `none`, `channel`, `last_channel`, `webhook`.

Validation payload conventions follow ADR-034 and API-SPEC.md section 2:

- Request: `{ "definition": {}, "strict": true, "context": {} }`
- Response: `{ "valid": true, "issues": [], "normalized": {} }`
- Issue object: `{ "severity": "error"|"warning", "code": "...", "path": "schedule.expr", "message": "..." }`

Operational action responses (enable, disable, run-now) use the accepted envelope:
`{ "accepted": true, "resource_id": "sched_123", "status": "accepted" }`

Run-now/dry-run response per API-SPEC.md section 6:
`{ "would_execute": true, "resolved": { "schedule_id", "action": { ... } }, "effects": { "schedule_fire": true }, "explanation": { "delivery": { "kind": "none" } } }`

Schedules intentionally do not expose a compile endpoint because their typed model does not require
a separate public compilation artifact (ADR-035). The validate endpoint is sufficient.

The existing `/api/schedules` and `/api/schedules/{id}/run` routes in `server.rs` (lines 297-308)
must be migrated. The old `/run` sub-resource becomes `/run-now` under the new path.

### Relevant Files

- `crates/openfang-api/src/routes.rs` — existing handler implementations; add new v1 schedule handlers here
- `crates/openfang-api/src/server.rs` — router registration; replace `/api/schedules` block with `/api/v1/schedules`
- `crates/openfang-kernel/src/scheduler.rs` — active scheduler; must be notified on enable/disable changes
- `crates/openfang-types/src/scheduler.rs` — typed cron types (`CronJobId`, `CronSchedule`, `CronAction`, `CronDelivery`)
- `tasks/prd-compozy/docs/API-SPEC.md` — canonical payload contracts (section 6 for schedules, section 2 for common conventions)
- `tasks/prd-compozy/docs/DESIGN.md` — section 15 (canonical vs extended OpenFang systems, schedule notes)
- `tasks/prd-compozy/docs/adrs/035-schedule-api-surface-on-typed-cron-model.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`
- `tasks/prd-compozy/docs/adrs/023-public-api-exposure-rules.md`

### Dependent Files

- `crates/openfang-kernel/src/kernel.rs` — kernel holding the scheduler reference
- `crates/openfang-types/src/config.rs` — config types referenced in AppState

## Deliverables

- All `/api/v1/schedules` CRUD endpoints registered and implemented
- `POST /api/v1/schedules/validate` endpoint with typed cron and action validation
- `POST /api/v1/schedules/{id}/fork` endpoint with provenance metadata
- `GET /api/v1/schedules/{id}/runtime` endpoint
- `POST /api/v1/schedules/{id}/enable` and `POST /api/v1/schedules/{id}/disable` endpoints
- `POST /api/v1/schedules/{id}/run-now` and `POST /api/v1/schedules/{id}/run-now/dry-run` endpoints
- Tests for all operations

## Tests

### Unit Tests (Required)

- [x] Cron expression validation: a valid standard 5-field expression (`"0 2 * * *"`) passes
      validation; a malformed expression (`"99 99 99 99 99"`) returns `valid: false` with a structured
      issue whose `path` is `"schedule.expr"`.
- [x] Timezone validation: an unknown timezone string in `schedule.tz` returns `valid: false` with
      a structured issue.
- [x] Action payload validation: a `workflow_run` action with a missing `workflow_id` field returns
      `valid: false` with `path: "action.workflow_id"` in the issues list.
- [x] An unsupported action kind returns a structured validation error, not a 500 or a silent
      success.
- [x] Validation of a `workflow_signal` action correctly requires a `selector.workflow_id` field.
- [x] Enable/disable state transitions: after `POST /api/v1/schedules/{id}/disable` the persisted
      `enabled` flag becomes false and the schedule is removed from the active cron queue synchronously.

### Integration Tests (Required)

- [x] Full schedule lifecycle: create → validate its definition → enable → run-now → disable →
      delete. Each step must return the correct status code and payload shape.
- [x] `POST /api/v1/schedules` with a valid `workflow_run` action persists the record in `runtime.db`
      and returns the full schedule resource including `runtime_status`.
- [x] List endpoint returns `{ items, next_cursor }` with pagination: create four schedules, fetch
      with `limit=2`, assert `next_cursor` non-null, fetch second page, assert all four are returned
      across both pages.
- [x] A disabled schedule (`enabled: false`) never fires even when its cron expression matches the
      current time; verified by checking scheduler state after disable.
- [x] `POST /api/v1/schedules/{id}/run-now` on a disabled schedule must still succeed (run-now
      bypasses the enabled gate) or return a clear documented error if the product decides to block it.
      Either behavior must be explicitly tested and consistent with API-SPEC.md.
- [x] `POST /api/v1/schedules/{id}/run-now/dry-run` returns `would_execute: true` and a `resolved`
      block containing the schedule ID and action kind without performing any side effect.
- [x] `DELETE /api/v1/schedules/{id}` on a non-existent ID returns 404 with the stable error
      envelope.

### Regression and Anti-Pattern Guards

- [x] Disabled schedules never fire: verified through scheduler state inspection after disable, not
      only through the flag in the database row.
- [x] Invalid cron expressions are rejected at write time (`POST` and `PUT`) and at validate time;
      they must never reach the cron queue.
- [x] The old blob-style `/api/schedules` routes are removed and not duplicated alongside the new
      v1 surface.
- [x] Run-now is the only bypass for the cron timer; no hidden side-effecting path triggers
      schedule actions outside of the scheduler and run-now endpoints.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- All ten endpoints (`GET`, `POST`, `GET {id}`, `PUT {id}`, `DELETE {id}`, `POST validate`,
  `POST {id}/fork`, `GET {id}/runtime`, `POST {id}/enable`, `POST {id}/disable`,
  `POST {id}/run-now`, `POST {id}/run-now/dry-run`) are registered in the axum router and return
  correct status codes and payload shapes for both happy-path and error cases.
- Cron expressions and action payloads are validated at write time using the typed cron model from
  `crates/openfang-types/src/scheduler.rs`; no invalid definition reaches the scheduler queue.
- Enable/disable toggle the active scheduler state synchronously and persist the change to
  `runtime.db`.
- Run-now bypasses the cron timer and dispatches the action immediately; run-now/dry-run simulates
  this without side effects.
- All list endpoints return `{ items, next_cursor }` with the correct pagination and filter
  parameters.
- Disabled schedules provably never fire: verified by inspecting scheduler state in tests.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- ADR-035 explicitly states schedules do not require a separate compile endpoint. Do not add one.
- The existing `/api/schedules/{id}/run` route name changes to `/api/v1/schedules/{id}/run-now`
  per API-SPEC.md section 6 and the CLI mirror in section 14.
