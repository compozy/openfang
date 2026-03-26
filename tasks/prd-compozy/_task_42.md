## markdown

## status: completed

<task_context>
<domain>engine/retention</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task39,task33</dependencies>
</task_context>

# Task 42.0: Retention Policies And Remaining SSE Endpoints

## Overview

Implement indexing and retention policies for append-only tables that can grow without bound under
sustained workload, and implement the remaining SSE watch endpoints not yet covered by earlier
tasks. This task covers:

1. **Indexes and retention** for `workflow_checkpoint`, `artifact_version`, and `doc_version`
   tables, with a background retention job for pruning.
2. **SSE endpoints**: `GET /api/v1/runs/{id}/events`, `GET /api/v1/dispatches/{id}/events`, and
   `GET /api/v1/hitl-requests/stream` per ADR-036 and ADR-039.

These are infrastructure hardening concerns that ensure the system operates sustainably under
sustained workload and provides operators with live observability.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start -- **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add indexes and retention policies for the three append-only tables that can grow without bound
  under sustained workload: `workflow_checkpoint`, `artifact_version`, and `doc_version`. Required
  indexes: `workflow_checkpoint (run_id, created_at)`, `artifact_version (artifact_id, version_no)`,
  `artifact_version (content_hash)`, `doc_version (doc_id, version_no)`,
  `doc_version (content_hash)`. Retention policy: a configurable maximum row count or age threshold
  per parent ID (e.g., retain at most N checkpoints per run, retain all artifact/doc versions but
  prune `workflow_checkpoint` records for completed runs older than T days). The retention policy
  must be configurable and must default to a reasonable limit (e.g., 1000 checkpoints per run,
  30-day age cutoff for completed runs). The pruning job must run on a background timer, not
  inline with reads or writes.
- Implement the three remaining SSE watch endpoints from API-SPEC.md section 14 that are not yet
  covered by earlier tasks: `GET /api/v1/runs/{id}/events`, `GET /api/v1/dispatches/{id}/events`,
  and `GET /api/v1/hitl-requests/stream`. All three must follow the bounded replay and
  `stream.reset` + `stream.snapshot` semantics from ADR-039, must support `Last-Event-ID`, and
  must emit `keepalive` events on idle connections. Event names for each endpoint must match the
  SSE event name list in API-SPEC.md section 2.
- All SSE endpoints introduced in this task must stream without memory leaks over connections held
  open for at least 60 seconds. This must be verified by a test that holds a connection open
  across multiple state transitions.
</requirements>

## Subtasks

- [x] 42.1 Add indexes to `workflow_checkpoint`, `artifact_version`, and `doc_version` in new
      migrations in `migrations/compozy/`. Indexes:
      - `workflow_checkpoint (run_id, created_at)`
      - `artifact_version (artifact_id, version_no)`
      - `artifact_version (content_hash)`
      - `doc_version (doc_id, version_no)`
      - `doc_version (content_hash)`

- [x] 42.2 Implement the retention policy background job: a `tokio` task that runs on a
      configurable interval (default: every hour) and deletes `workflow_checkpoint` records for
      completed runs older than the configured age threshold, using a batched DELETE to avoid
      locking the database for large prunes. Retention rules:
      - For each `run_id`, retain all checkpoints for runs in `running` or `paused` state.
      - For `completed` or `failed` or `cancelled` runs, retain checkpoints only if the run
        completed within the last T days (default T = 30).
      - Delete in batches of at most 500 rows per pruning cycle to avoid write amplification.
      - `artifact_version` and `doc_version`: retain all versions by default (content-addressable
        immutability is a product guarantee). No pruning for these tables in this task, but indexes
        must be in place.

- [x] 42.3 Implement `GET /api/v1/runs/{id}/events` SSE endpoint. Emit at minimum:
      `stream.snapshot` (current run state), `run.updated` (on any run status or step change),
      `dispatch.updated` (on dispatch status changes within this run), `hitl.requested` (when a
      new HITL request is created for this run), `stream.reset` + `stream.snapshot` fallback on
      expired `Last-Event-ID`, and `keepalive` heartbeat every 15 seconds. The run event stream
      must emit composite events that aggregate progress across the run's dispatches and HITL
      requests.

- [x] 42.4 Implement `GET /api/v1/dispatches/{id}/events` SSE endpoint. Emit at minimum:
      `stream.snapshot` (current dispatch state), `dispatch.updated` (on status changes),
      `hitl.requested` (when an in-step HITL request is created for this dispatch), and
      `keepalive` heartbeat every 15 seconds.

- [x] 42.5 Implement `GET /api/v1/hitl-requests/stream` SSE endpoint (global stream, not per-ID).
      Emit `hitl.requested` for every new HITL request created across all runs, and `hitl.answered`
      when any request is answered. This is the surface operators use to monitor pending human
      interaction needs without polling individual run endpoints. The global stream must be bounded
      by a global ring buffer of 200 events. It must support optional `run_id` and `status` query
      filters on the query string.

- [x] 42.6 Ensure all three SSE endpoints follow the bounded replay pattern: ring buffer of 50
      events per resource (per run for `/runs`, per dispatch for `/dispatches`, global 200 for
      `/hitl-requests/stream`), `Last-Event-ID` for best-effort resume, `stream.reset` +
      `stream.snapshot` fallback, and `keepalive` every 15 seconds.

- [x] 42.7 Add tests for retention policies and SSE endpoints. See the Tests section below.

- [x] 42.8 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

### Retention Policies

Append-only tables that require retention management:

- `workflow_checkpoint`: can accumulate thousands of rows per long-running workflow. Retention
  rule: for each `run_id`, retain all checkpoints for runs in `running` or `paused` state; for
  `completed` or `failed` or `cancelled` runs, retain checkpoints only if the run completed within
  the last T days (default T = 30). Delete in batches of at most 500 rows per pruning cycle to
  avoid write amplification.
- `artifact_version` and `doc_version`: retain all versions by default (content-addressable
  immutability is a product guarantee). Pruning of these tables is not required in this task but
  the indexes must be in place for efficient querying.

The retention job must be a background `tokio::task` spawned from the kernel boot sequence. It
must not block the main event loop. Pruning must be logged via `tracing::info!` with counts of
deleted rows per cycle.

### SSE Remaining Endpoints

All three endpoints follow the same pattern as `GET /api/v1/looper-runs/{id}/events` from
Task 39:

- Ring buffer of 50 events per resource (per run for `/runs`, per dispatch for `/dispatches`,
  global 200 for `/hitl-requests/stream`)
- `Last-Event-ID` for best-effort resume
- `stream.reset` + `stream.snapshot` fallback
- `keepalive` every 15 seconds

The run event stream (`/runs/{id}/events`) must emit composite events that aggregate progress
across the run's dispatches and HITL requests. Clients watching this endpoint get the full picture
of a run without subscribing to individual dispatch or HITL streams.

The `hitl-requests/stream` global stream must be bounded by a global ring buffer of 200 events
(larger than per-resource buffers to support operators monitoring the full system). It must support
optional `run_id` and `status` query filters on the query string, but the SSE protocol itself is
still per-stream.

### Relevant Files

- `crates/openfang-api/src/routes.rs` -- handler implementations
- `crates/openfang-api/src/server.rs` -- route registration
- `crates/openfang-kernel/src/kernel.rs` -- boot sequence, background task spawning
- `crates/openfang-kernel/src/event_bus.rs` -- event bus for SSE fan-out
- `crates/openfang-memory/src/migration.rs` -- migration runner pattern
- `migrations/compozy/` -- migration sequence to extend
- `tasks/prd-compozy/docs/API-SPEC.md` sections 2, 14
- `tasks/prd-compozy/docs/adrs/036-explicit-watch-surfaces-for-live-operations.md`
- `tasks/prd-compozy/docs/adrs/039-bounded-sse-replay-and-reset-semantics.md`
- `tasks/prd-compozy/docs/DESIGN.md` section 24 -- watch policy

### Dependent Files

- Task 39 -- looper SSE surfaces (pattern to follow)
- Task 33 -- dispatch and HITL control-plane surfaces (data sources for SSE events)
- Task 43 -- E2E integration test will verify SSE endpoints

## Deliverables

- Indexes on `workflow_checkpoint`, `artifact_version`, `doc_version`
- Retention policy background job for `workflow_checkpoint`
- `GET /api/v1/runs/{id}/events` SSE endpoint
- `GET /api/v1/dispatches/{id}/events` SSE endpoint
- `GET /api/v1/hitl-requests/stream` SSE endpoint
- All SSE endpoints with bounded replay, `stream.reset` fallback, and `keepalive`

## Tests

### Unit Tests (Required)

- [x] Retention policy enforcement: after writing 1200 `workflow_checkpoint` rows for a single
      completed run older than the configured age threshold, one pruning cycle deletes rows in
      batches until the count is below the configured maximum, without touching checkpoints for
      runs below the threshold.
- [x] `GET /api/v1/runs/{id}/events` SSE serialization: a `run.updated` event for a running
      workflow run produces correctly formatted `text/event-stream` output with `id`, `event`, and
      `data` fields; the `data` field deserializes to a valid run summary shape matching API-SPEC.md
      section 9.
- [x] `GET /api/v1/hitl-requests/stream` emits a `hitl.requested` event when a new `hitl_request`
      record is created in any run; the event includes the HITL request ID, run ID, and question
      fields.
- [x] `GET /api/v1/dispatches/{id}/events` with `Last-Event-ID` beyond the ring buffer emits
      `stream.reset` then `stream.snapshot` before continuing with live events.

### Integration Tests (Required)

- [x] Retention pruning integration: create a workflow run, write 600 checkpoints, mark the run
      completed with a timestamp 60 days in the past, run one pruning cycle, and verify the
      checkpoint count for that run is below the configured maximum.
- [x] SSE memory stability: hold a connection to `GET /api/v1/runs/{id}/events` open for 60
      seconds while the run advances through 5 state changes; verify all 5 `run.updated` events
      are received and the connection remains alive via `keepalive` events.
- [x] Global HITL stream with filters: subscribe to `GET /api/v1/hitl-requests/stream?status=pending`,
      create both a pending and an answered HITL request, verify only the pending request event is
      delivered.

### Regression and Anti-Pattern Guards

- [x] No append-only table (`workflow_checkpoint`, `artifact_version`, `doc_version`) grows
      unboundedly under sustained workload; the retention job must run and reduce checkpoint counts
      for eligible completed runs.
- [x] All SSE endpoints stream without memory leaks over long connections; the ring buffer must
      have a fixed maximum size and must not grow with connection duration.
- [x] The retention job must not block the main event loop or hold database locks for extended
      periods; batched deletes enforce this.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- Retention policies prevent unbounded growth of `workflow_checkpoint` under sustained workload;
  the pruning job runs on a background timer without blocking the main event loop.
- `GET /api/v1/runs/{id}/events`, `GET /api/v1/dispatches/{id}/events`, and
  `GET /api/v1/hitl-requests/stream` all stream correctly with bounded replay, `stream.reset`
  fallback, and `keepalive` heartbeats.
- All indexes are in place for efficient querying of append-only tables.
- SSE endpoints do not leak memory over connections held open for at least 60 seconds.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Notes

- CLI commands are deferred to future work (do not touch openfang-cli).
- This task is part of the hardening phase and focuses on operational sustainability.
