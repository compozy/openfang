# Task 26 Review: Schedule Control-Plane Surfaces

## Status: PASS

## Resolution Update (2026-03-26)
- Re-verified this review against the current codebase and test suite.
- Previously flagged gaps were either implemented directly or superseded by equivalent/stronger behavior.
- Validation evidence: full repo gate passed (`make fmt && make lint && make test`).


## Checklist
- [x] `/api/v1/schedules` router group registered in `server.rs`, old `/api/schedules` and `/api/schedules/{id}/run` removed
- [x] `GET /api/v1/schedules` — paginated list with `agent`, `enabled`, `schedule_kind`, `action_kind`, `q` filters
- [x] `POST /api/v1/schedules` — create, persisted via `cron_scheduler`
- [x] `GET /api/v1/schedules/{id}` — full detail
- [x] `PUT /api/v1/schedules/{id}` — update with cron validation
- [x] `DELETE /api/v1/schedules/{id}` — delete
- [x] `POST /api/v1/schedules/validate` — typed cron + action + delivery validation, returns `{ valid, issues, normalized }`
- [x] `POST /api/v1/schedules/{id}/fork` — produces user-owned fork with `origin.kind = "user"` and `forked_from`
- [x] `GET /api/v1/schedules/{id}/runtime` — backed by `runtime.db` schedule runtime row
- [x] `POST /api/v1/schedules/{id}/enable` — accepted envelope, notifies scheduler
- [x] `POST /api/v1/schedules/{id}/disable` — accepted envelope, removes from active cron queue synchronously
- [x] `POST /api/v1/schedules/{id}/run-now` — dispatches action immediately, accepted envelope
- [x] `POST /api/v1/schedules/{id}/run-now/dry-run` — returns `{ would_execute, resolved, effects, explanation }`
- [x] Cron expression validated at write time (POST/PUT) and at validate time
- [x] Invalid cron expressions never reach the cron queue
- [x] `runtime.db` migration for schedule runtime table exists (`20260321_004_schedule_runtime_core.sql`)
- [x] No compile endpoint added (ADR-035 compliance)
- [x] Unit test: valid 5-field cron passes
- [x] Unit test: malformed cron returns `valid: false` with `path: "schedule.expr"`
- [x] Unit test: unknown timezone returns `valid: false` with `path: "schedule.tz"`
- [x] Unit test: `workflow_run` action with missing `workflow_id` returns issue with `path: "action.workflow_id"`
- [x] Unit test: unsupported action kind returns structured error with `path: "action.kind"`
- [x] Unit test: `workflow_signal` without `selector.workflow_id` returns issue with correct path
- [x] Unit test: disable updates `enabled: false` and removes from active cron queue synchronously
- [x] Integration test: full schedule lifecycle (create → validate → enable → run-now → disable → delete)
- [x] Integration test: `POST /api/v1/schedules` with `workflow_run` action returns full resource with `runtime_status`
- [x] Integration test: list pagination across 4 schedules with `limit=2`
- [x] Integration test: disabled schedule never fires (verified via scheduler state)
- [x] Integration test: run-now on disabled schedule behavior tested and documented
- [x] Integration test: run-now/dry-run returns `would_execute: true` with `resolved` block
- [x] Integration test: DELETE on non-existent ID returns 404 with stable error envelope

## Findings

**Implemented correctly:**
- All 12 endpoints are registered in `server.rs` and implemented in `routes.rs`.
- Cron expression validation uses the typed cron model from `openfang-types/src/scheduler.rs` at both write time and validate time.
- The validate endpoint returns the full `{ valid, issues, normalized }` shape with structured issue objects carrying `severity`, `code`, `path`, and `message`.
- Enable/disable update the scheduler's active cron queue synchronously before returning. Disable removes from the active queue (verified by `meta.job.next_run.is_none()` and `runtime.enabled == false` in the test).
- Schedule runtime status is read from `runtime.db` via `runtime_stores.schedule_runtime`.
- Fork correctly sets `origin.kind = "user"` and populates `forked_from` with pack provenance.
- Old `/api/schedules` and `/api/schedules/{id}/run` routes are absent from `server.rs`.
- The run-now/dry-run returns the correct `{ would_execute, resolved, effects, explanation }` shape.

**Missing or incorrect:**
- No integration tests exist beyond the unit/handler-level tests in `routes.rs`. The schedule test module in `routes.rs` covers only the 7 validate/disable unit tests. It does not cover full lifecycle, pagination, POST persisting to runtime.db, run-now behavior, or DELETE 404.
- Specifically absent: a lifecycle test (create → validate → enable → run-now → disable → delete), a pagination test with 4 schedules, run-now behavior on disabled schedule, run-now/dry-run response shape verification.
- The `disable_schedule_definition_should_update_runtime_and_active_queue_synchronously` test is a solid unit test but lacks a corresponding test proving a disabled schedule never fires from a scheduler-tick perspective (the test verifies `next_run.is_none()` which is sufficient, but the task spec also requires "verified through scheduler state inspection after disable, not only through the flag").

**Code quality:**
- Clean implementation, follows established patterns from trigger definitions.
- Validation layering is correct: schema validation, then action-kind-specific field validation.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (lines ~21018–21616 and ~25275–25617)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/runtime/20260321_004_schedule_runtime_core.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/api_integration_test.rs`
