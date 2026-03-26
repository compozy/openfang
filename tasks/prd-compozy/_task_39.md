## markdown

## status: completed

<task_context>
<domain>domain/looper/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task33,task32,task34</dependencies>
</task_context>

# Task 39.0: Looper Control-Plane And SSE Surfaces

## Overview

Expose looper control-plane API surfaces and SSE/watch endpoints for looper
runs. This task covers the public API layer on top of the durable looper
runtime implemented in task 34. Every endpoint must match the payload shapes
in API-SPEC.md section 13 exactly. The SSE watch endpoint for looper runs
(`GET /api/v1/looper-runs/{id}/events`) follows the bounded replay and
reset-and-snapshot semantics mandated by ADR-036 and ADR-039: it supports
`Last-Event-ID` for best-effort resume within a bounded in-memory ring buffer;
when the requested event is no longer available the server emits `stream.reset`
followed by `stream.snapshot` before continuing with live events.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets ---D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement all looper control-plane endpoints from API-SPEC.md section 13:
  `POST /api/v1/looper-runs` (create and start a looper run against an explicit
  `task_id` with a required `execution_policy`), `GET /api/v1/looper-runs`
  (list with filters: `task_id`, `source_run_id`, `status`, `execution_mode`),
  `GET /api/v1/looper-runs/{id}` (detail with full progress and policy),
  `GET /api/v1/looper-runs/{id}/subtasks` (list looper subtask execution view),
  `POST /api/v1/looper-runs/{id}/pause`, `POST /api/v1/looper-runs/{id}/resume`,
  `POST /api/v1/looper-runs/{id}/cancel`. All endpoints must be registered in
  `crates/openfang-api/src/server.rs` and implemented in the routes module.
- The `POST /api/v1/looper-runs` creation endpoint must validate that the
  request body includes a `task_id` and a well-formed `execution_policy` object
  (with at least `mode` and `max_parallelism`). A missing or invalid policy
  must return HTTP 422 with a structured error body matching the error shape
  in API-SPEC.md section 2. The endpoint must not silently default the policy.
- Implement `GET /api/v1/looper-runs/{id}/events` as an SSE endpoint per
  ADR-036. The endpoint must: (a) immediately stream a `stream.snapshot` event
  containing the current looper run state; (b) stream live events as the
  looper run advances — at minimum `run.updated`, `subtask.started`,
  `subtask.completed`, `subtask.failed`; (c) support `Last-Event-ID` for
  best-effort resume within a bounded ring buffer of recent events (minimum
  ring buffer capacity: 50 events per run); (d) emit `stream.reset` followed
  by `stream.snapshot` when `Last-Event-ID` refers to an event no longer in
  the buffer, then continue with live events per ADR-039.
- SSE events must be serialized as standard `text/event-stream` format with
  `id:`, `event:`, and `data:` fields. The `id` field must be a monotonically
  increasing integer per run so `Last-Event-ID` can be compared numerically.
  The `data` field must be valid JSON matching the event type schemas from
  API-SPEC.md section 2.
- List and detail responses must exactly match the looper run resource shape
  from API-SPEC.md section 13, including the `execution_policy`,
  `current_subtask_id`, `progress` object (`total`, `completed`, `failed`),
  and all timestamp fields. No field may be omitted or renamed.
- Pause, resume, and cancel endpoints must return the standard operational
  action response shape from API-SPEC.md section 2 (`accepted`, `resource_id`,
  `status`) and must delegate to the `LooperRuntime` from task 34. If the
  looper run is already in a terminal state (`completed`, `failed`,
  `cancelled`), these endpoints must return HTTP 409 with a structured error.
</requirements>

## Subtasks

- [x] 39.1 Implement `POST /api/v1/looper-runs`: validate request body
      (`task_id` required, `execution_policy` required and well-formed), call
      `LooperRunRepository::create` from task 34, start the `LooperRuntime`, and
      return the accepted response with `looper_run_id`. Register the route in
      `crates/openfang-api/src/server.rs`.
- [x] 39.2 Implement `GET /api/v1/looper-runs` with list filters (`task_id`,
      `source_run_id`, `status`, `execution_mode`), cursor pagination, and the
      looper run summary shape. Implement `GET /api/v1/looper-runs/{id}` with the
      full detail shape including progress and policy.
- [x] 39.3 Implement `GET /api/v1/looper-runs/{id}/subtasks` returning the
      `looper_subtask` execution view (from `LooperSubtaskRepository` in task 34),
      not the canonical `subtask` records from `SubtaskRepository` in task 28.
- [x] 39.4 Implement `POST /api/v1/looper-runs/{id}/pause`,
      `POST /api/v1/looper-runs/{id}/resume`, and
      `POST /api/v1/looper-runs/{id}/cancel`. Each must call the corresponding
      method on `LooperRuntime` and return the action response. Return HTTP 409
      when the run is in a terminal state.
- [x] 39.5 Implement `GET /api/v1/looper-runs/{id}/events` as an SSE endpoint.
      Wire an in-process event channel from `LooperRuntime` (a `tokio::sync::broadcast`
      channel is recommended) into the SSE response stream. Implement the ring
      buffer for bounded replay and the `stream.reset` + `stream.snapshot`
      fallback. Register the route in `crates/openfang-api/src/server.rs`.
- [x] 39.6 Write unit and integration tests as detailed in the Tests section.
- [x] 39.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass with zero warnings before marking done.

## Implementation Details

The looper control-plane follows API-SPEC.md section 13 exactly. The creation
endpoint is the canonical way to start looper execution for a task (ADR-048,
DESIGN.md section 23):

```json
{
  "task_id": "task_001",
  "subtask_ids": null,
  "execution_policy": {
    "mode": "parallel",
    "max_parallelism": 4,
    "selection": "priority"
  },
  "metadata": { "source": "api" }
}
```

The `subtask_ids` field, when provided, limits the looper to a specific subset
of the task's subtasks. When `null`, the looper operates over all subtasks of
the task that are in an eligible status (`planned`, `ready`).

The looper run detail shape from API-SPEC.md section 13:

```json
{
  "id": "loop_321",
  "task_id": "task_001",
  "source_run_id": "run_123",
  "status": "running",
  "execution_policy": {
    "mode": "parallel",
    "max_parallelism": 4,
    "selection": "priority"
  },
  "current_subtask_id": "subtask_001",
  "progress": { "total": 12, "completed": 3, "failed": 1 },
  "error": null,
  "started_at": "2026-03-21T14:08:00Z",
  "updated_at": "2026-03-21T14:10:00Z",
  "completed_at": null
}
```

The SSE endpoint is one of the five baseline watch surfaces mandated by
ADR-036. It must not be a generic `watch=true` query parameter but a distinct
sub-resource path (`/events`). The endpoint must set
`Content-Type: text/event-stream`, `Cache-Control: no-cache`, and
`Connection: keep-alive` headers. Each SSE message must use the standard
format:

```
id: 42
event: run.updated
data: {"id":"loop_321","status":"running","progress":{"total":12,"completed":4,"failed":0}}

```

Event names to implement for looper runs:

- `stream.snapshot` — sent immediately on connect with current run state
- `stream.reset` — sent before snapshot when `Last-Event-ID` is expired
- `run.updated` — sent on any looper run status or progress change
- `subtask.started` — sent when a looper subtask transitions to `running`
- `subtask.completed` — sent when a looper subtask transitions to `completed`
- `subtask.failed` — sent when a looper subtask transitions to `failed`
- `keepalive` — sent every 15 seconds on idle connections to prevent proxies
  from closing the connection

The bounded ring buffer for replay must be per-run (keyed by `looper_run_id`)
and must be stored in `AppState` (or a dedicated event store on `AppState`).
The ring buffer capacity of 50 events per run means that up to 50 recent events
can be replayed via `Last-Event-ID`. Events older than the buffer window trigger
the `stream.reset` fallback.

The `LooperRuntime` from task 34 must expose a broadcast channel that callers
can subscribe to for live events. A `tokio::sync::broadcast::Sender<LooperEvent>`
is the recommended pattern — the API SSE handler subscribes a new receiver and
forward events to the HTTP response body. The sender lag policy on the broadcast
channel must be set so slow SSE consumers do not block looper execution
(use `broadcast::channel(128)` and drop lagged receivers gracefully).

The `AppState` in `crates/openfang-api/src/routes.rs` must be extended with
the looper runtime registry (a `DashMap<LooperRunId, Arc<LooperRuntime>>` or
equivalent) so that API handlers can locate the in-memory runtime for a given
looper run ID. When a looper run is recovered on restart (task 34 recovery),
the recovered runtime must also be inserted into this registry.

Follow the `AppState` extension pattern established in
`crates/openfang-api/src/routes.rs` and `crates/openfang-api/src/server.rs`:
new fields are added to `AppState` struct, initialized in `build_router`, and
accessed via `State(state): State<Arc<AppState>>` in handlers.

### Relevant Files

- `crates/openfang-api/src/routes.rs` — AppState, handler pattern, route functions
- `crates/openfang-api/src/server.rs` — `build_router`, route registration
- `crates/openfang-api/src/types.rs` — request/response type definitions
- `tasks/prd-compozy/docs/API-SPEC.md` sections 2, 13, 14 — looper shapes and watch policy
- `tasks/prd-compozy/docs/adrs/036-explicit-watch-surfaces-for-live-operations.md`
- `tasks/prd-compozy/docs/adrs/039-bounded-sse-replay-and-reset-semantics.md`
- `tasks/prd-compozy/docs/adrs/048-task-and-subtask-control-plane-surfaces.md`
- `tasks/prd-compozy/docs/DESIGN.md` sections 23, 24 — watch policy rationale
- looper runtime modules from task 34

### Dependent Files

- `crates/openfang-kernel/src/kernel.rs` — looper registry and recovery integration
- task 43 E2E integration test — exercises the full looper API surface

## Deliverables

- Looper control-plane API surfaces: create, list, detail, subtasks,
  pause, resume, cancel
- SSE watch endpoint with bounded replay, `stream.reset` fallback, and
  `keepalive` heartbeat
- Looper runtime registry in `AppState`
- Full test suite as described below

## Tests

### Unit Tests (Required)

- [ ] `POST /api/v1/looper-runs` with a valid body and a known `task_id` returns
      HTTP 202 with `accepted: true` and a non-empty `looper_run_id`.
- [ ] `POST /api/v1/looper-runs` with a missing `execution_policy` field returns
      HTTP 422 with a structured error body that includes `code: "validation_error"`
      and a `details` array identifying the missing field.
- [ ] `POST /api/v1/looper-runs` with `execution_policy.max_parallelism = 0`
      returns HTTP 422 with a domain validation error.
- [ ] SSE event serialization: a `run.updated` event for a running looper run
      produces a correctly formatted `text/event-stream` message with `id`,
      `event`, and `data` fields; the `data` field deserializes to a valid looper
      run summary shape.
- [ ] `POST /api/v1/looper-runs/{id}/pause` on a looper run in `completed` state
      returns HTTP 409 with a structured error indicating the run is terminal.
- [ ] `GET /api/v1/looper-runs/{id}` for a non-existent ID returns HTTP 404 with
      a structured error body.
- [ ] `GET /api/v1/looper-runs` with `status = "running"` filter returns only
      looper runs in `running` status; runs in other statuses are excluded.

### Integration Tests (Required)

- [ ] Full API round-trip: create a looper run via `POST /api/v1/looper-runs`,
      poll `GET /api/v1/looper-runs/{id}` until status is `completed`, verify
      `progress.completed` equals the total number of subtasks.
- [ ] SSE endpoint streams at least one `subtask.started` and one
      `subtask.completed` event during a looper run that executes one subtask;
      the `stream.snapshot` event is the first event received by a fresh subscriber.
- [ ] `Last-Event-ID` resume within the ring buffer: connect to the SSE endpoint,
      receive 10 events, disconnect, reconnect with `Last-Event-ID` set to event 5,
      and verify events 6–10 are replayed before live events arrive.
- [ ] `Last-Event-ID` beyond the buffer: connect with a `Last-Event-ID` that is
      older than the ring buffer window, and verify the server emits `stream.reset`
      then `stream.snapshot` before continuing with live events.
- [ ] `keepalive` heartbeat: open an SSE connection to a paused looper run and
      verify a `keepalive` event is received within 20 seconds without any looper
      state change.
- [ ] Pause then resume round-trip via API: `POST /pause` returns `accepted`,
      `GET /{id}` shows `status = "paused"`, `POST /resume` returns `accepted`,
      subsequent `GET /{id}` shows `status = "running"`.

### Regression and Anti-Pattern Guards

- [ ] Do not bolt on SSE as a `watch=true` query parameter on the detail
      endpoint; the watch surface must be the dedicated `/events` sub-resource per
      ADR-036.
- [ ] Do not treat the SSE endpoint as a full event history API; events older
      than the ring buffer must trigger `stream.reset` and `stream.snapshot`, not
      unbounded backfill per ADR-039.
- [ ] Do not re-architect the looper runtime from task 34 in this task; only
      add the API surface and the event channel integration.
- [ ] Do not create internal-only routes for looper control; all surfaces must
      be public paths under `/api/v1/looper-runs`.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All seven looper control-plane endpoints (create, list, detail, subtasks,
  pause, resume, cancel) are registered in `server.rs` and return payload
  shapes that exactly match API-SPEC.md section 13.
- The SSE endpoint streams `stream.snapshot`, `run.updated`, `subtask.started`,
  `subtask.completed`, `subtask.failed`, and `keepalive` events correctly.
- `Last-Event-ID` resume works within the ring buffer; requests outside the
  buffer trigger `stream.reset` + `stream.snapshot` fallback per ADR-039.
- The looper runtime registry in `AppState` correctly maps live and recovered
  runtimes to their `looper_run_id` so API handlers can locate them.
- HTTP 409 is returned for pause/resume/cancel on terminal looper runs.
- `cargo fmt --all`, `cargo clippy`, and `cargo test --workspace` all pass at
  zero warnings and zero failures.

---

## Notes

- This task was slimmed from the original task 24. Artifact/doc versioning is now in task 37. Pack operationalization and final hardening are now in task 32.
- CLI commands for looper management are deferred to future work (do not touch openfang-cli).
