## markdown

## status: completed

<task_context>
<domain>engine/dispatch/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task29,task30</dependencies>
</task_context>

# Task 33.0: Dispatch And HITL Control-Plane Surfaces

## Overview

Expose the durable dispatch and HITL runtime objects through the Compozy public control plane.
Per ADR-026 (Runtime Execution Resource Surfaces), `dispatches` and `hitl-requests` are
first-class product resources under `/api/v1`, designed for both direct human administration and
internal agentic use. This task wires the repositories from tasks 23 and 24 — and the runtime
semantics from tasks 24 and 25 — into the API layer defined in `API-SPEC.md` sections 10 and 11.

The control-plane surfaces in this task are the completion of the full durable dispatch/HITL
runtime slice. After this task, a human operator or an internal agent can:

- List and inspect all dispatches for a workflow run.
- List all children of a given dispatch (for multi-level delegation trees).
- Cancel or retry a dispatch.
- List and inspect HITL requests, including pending ones awaiting an answer.
- Submit an answer to a pending HITL request, triggering the resume path from task 30.
- Cancel a pending HITL request, failing the associated dispatch.
- Watch live dispatch and HITL state through SSE endpoints.

All endpoints must be consistent with the payload shapes in `API-SPEC.md` and must back their
responses from durable storage, never from in-memory runtime state alone.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement all dispatch endpoints defined in `API-SPEC.md` section 10:
  `GET /api/v1/dispatches`, `GET /api/v1/dispatches/{id}`,
  `GET /api/v1/dispatches/{id}/children`, `POST /api/v1/dispatches/{id}/retry`,
  `POST /api/v1/dispatches/{id}/cancel`. All read endpoints must return data from the
  `DispatchRepository` (durable storage), not from runtime in-memory projections.
- Implement all HITL endpoints defined in `API-SPEC.md` section 11:
  `GET /api/v1/hitl-requests`, `GET /api/v1/hitl-requests/{id}`,
  `POST /api/v1/hitl-requests/{id}/answer`, `POST /api/v1/hitl-requests/{id}/cancel`.
  The `answer` endpoint must trigger the resume path from task 30, not only update the database.
- Implement the run-scoped sub-resource endpoints from `API-SPEC.md` section 9:
  `GET /api/v1/runs/{id}/dispatches` and `GET /api/v1/runs/{id}/hitl-requests`. These must
  delegate to the same repositories as the top-level endpoints.
- Dispatch list responses must support the query filters defined in API-SPEC.md: `run_id`,
  `status`, `target_agent`, `step_id`. HITL list responses must support: `run_id`, `dispatch_id`,
  `status`, `kind`. All list responses must use the standard `{"items": [], "next_cursor": null}`
  envelope with `limit` and `cursor` pagination.
- The `POST /api/v1/dispatches/{id}/cancel` endpoint must: (a) verify the dispatch is in a
  cancellable state (`pending` or `running`), (b) cancel any live execution (via the kernel or
  workflow engine), (c) transition the dispatch to `cancelled`, (d) cascade to cancel any pending
  HITL requests linked to this dispatch.
- The `POST /api/v1/dispatches/{id}/retry` endpoint must: (a) verify the dispatch is in a
  retryable state (`failed` or `cancelled`), (b) increment the `attempt` counter, (c) create a
  new dispatch record as a child of the original (or reset the original, per the accepted retry
  semantics), and (d) re-enqueue execution through the same dispatch mode as the original.
- The SSE watch endpoints from API-SPEC.md section 14 — `GET /api/v1/dispatches/{id}/events`
  and `GET /api/v1/hitl-requests/stream` — must be registered in the router. At minimum, they
  must emit a `stream.snapshot` event with the current state and a `keepalive` heartbeat. Full
  live-event streaming is not required in this task but the endpoint must exist and not return 404.
- All response payloads must match the detail shapes in API-SPEC.md exactly. Fields must not be
  renamed, omitted, or given alternative types. The `status`, `kind`, and timestamp fields must
  serialize to the strings and RFC 3339 formats specified.
</requirements>

## Subtasks

- [x] 33.1 Register the new route handlers in `crates/openfang-api/src/server.rs`. Add all
      dispatch and HITL routes to the Axum router under `/api/v1/`. Confirm all paths match the
      exact strings in API-SPEC.md sections 10 and 11. Routes registered in `server.rs` but not
      implemented in `routes.rs` must return 501 Not Implemented during development, not 404.

- [x] 33.2 Implement the dispatch read surfaces: `GET /api/v1/dispatches`,
      `GET /api/v1/dispatches/{id}`, `GET /api/v1/dispatches/{id}/children`. These are read-only
      handlers backed by `DispatchRepository`. Apply the `run_id`, `status`, `target_agent`, and
      `step_id` query filters. Return the summary shape for list items and the detail shape for
      single-record endpoints, as defined in API-SPEC.md section 10.

- [x] 33.3 Implement the dispatch action surfaces: `POST /api/v1/dispatches/{id}/cancel` and
      `POST /api/v1/dispatches/{id}/retry`. Both must validate the current dispatch status before
      acting. `cancel` must cascade to linked HITL requests. `retry` must increment `attempt` and
      re-enqueue execution through the workflow engine or dispatch runtime.

- [x] 33.4 Implement the HITL read surfaces: `GET /api/v1/hitl-requests`,
      `GET /api/v1/hitl-requests/{id}`. These are backed by `HitlRepository`. Apply the `run_id`,
      `dispatch_id`, `status`, and `kind` filters. Return the exact detail shape from API-SPEC.md
      section 11, including `sequence_no`, `context`, `response`, and `timeout_at`.

- [x] 33.5 Implement `POST /api/v1/hitl-requests/{id}/answer`. This is the primary human
      interaction endpoint. It must: validate the request is `pending`, call `HitlRepository::answer`
      to write the response, and then trigger the task 30 resume path that transitions the dispatch
      and wakes (or reconstructs) the suspended step executor. Return the accepted action response
      shape from API-SPEC.md common conventions.

- [x] 33.6 Implement `POST /api/v1/hitl-requests/{id}/cancel`. Cancel the HITL request and
      cascade to fail the linked dispatch. Update `workflow_run.active_hitl_request_id` to null.

- [x] 33.7 Implement the run-scoped sub-resources `GET /api/v1/runs/{id}/dispatches` and
      `GET /api/v1/runs/{id}/hitl-requests` as thin delegating handlers that call the same
      repository query with `run_id` pre-filtered.

- [x] 33.8 Register stub SSE handlers for `GET /api/v1/dispatches/{id}/events` and
      `GET /api/v1/hitl-requests/stream` that return a `stream.snapshot` and `keepalive`. Full
      live streaming is deferred but the endpoints must be reachable.

- [x] 33.9 Write end-to-end tests covering the full lifecycle: create a run and dispatch via the
      workflow engine, read the dispatch through the API, submit a HITL answer through the API,
      verify the dispatch and run transition correctly, and read the final state through the API.

## Implementation Details

The existing API layer in `crates/openfang-api/src/routes.rs` follows a pattern where handlers
receive `State<Arc<AppState>>` and call into `self.kernel`. The `AppState` struct currently holds
`Arc<OpenFangKernel>` and various auxiliary state. For dispatch and HITL, the handlers need access
to the `DispatchRepository` and `HitlRepository` from tasks 19 and 20. These repositories should
be added to `AppState` as `Arc<dyn DispatchRepository>` and `Arc<dyn HitlRepository>` respectively,
preserving the dynamic dispatch pattern consistent with the rest of the API layer.

The HITL answer handler is the most complex endpoint. After writing the answer to the database, it
must trigger the runtime resume path from task 30. This means calling into the workflow engine or
kernel to signal that a HITL request has been answered. The signal path depends on how task 30
implements the resume: if a live suspended task exists (the step executor's channel receiver), the
kernel maintains a registry of pending HITL senders keyed by `hitl_request_id`. The answer handler
looks up the sender, fires it, and returns. If no live task exists (post-restart), the handler
triggers a step-executor reconstruction path.

This two-path design means the answer handler must not assume a live task exists. The flow is:

1. Validate request is `pending` via `HitlRepository::find_by_id`.
2. Call `HitlRepository::answer` with the response payload.
3. Transition dispatch to `running` via `DispatchRepository::update_status`.
4. Clear `workflow_run.active_hitl_request_id`.
5. Check kernel's HITL resume registry for a live sender keyed by `hitl_request_id`.
   6a. If found: send the answer value and remove the entry from the registry.
   6b. If not found: enqueue a step-executor reconstruction via the workflow engine.

The retry endpoint requires careful semantics. The current dispatch record is `failed` or
`cancelled`. The retry must increment `attempt` on the existing record and re-submit the dispatch
to the workflow engine for execution. Alternatively, if the design calls for creating a new
dispatch record for the retry, the original record's ID must be referenced in the new record's
`parent_dispatch_id` for lineage tracing. The chosen approach must be consistent with what
`GET /api/v1/dispatches/{id}/children` returns — if retry creates a sibling under the same parent,
children traversal works correctly; if retry creates a child of the failed record, the tree depth
increases.

The dispatch list endpoint must support the `step_id` filter to allow workflow step views to show
only dispatches belonging to a specific step. This is important for the `write-prd` step example
in API-SPEC.md — a workflow may have many dispatches, and the UI needs to scope them by step.

For the run-scoped sub-resources (`/api/v1/runs/{id}/dispatches` and
`/api/v1/runs/{id}/hitl-requests`), validate that the run exists before querying. Return 404 if
the run ID is not found in `WorkflowRunRepository`. This prevents leaking dispatch information
for runs the caller did not explicitly reference.

The HITL list endpoint (`GET /api/v1/hitl-requests`) is the primary surface for a human operator
monitoring pending questions. The `status=pending` filter is the most common use case — an
operator polls this endpoint to see what needs answering. The default sort order must be
`created_at ASC` so the oldest pending questions appear first.

The SSE endpoints registered in this task are intentionally minimal — they emit a snapshot and
heartbeat but do not yet deliver live event deltas. Full live streaming is a separate later task.
The endpoints must be registered now so that downstream SSE clients (future UI, CLI, agent
control) can be written against stable URL paths.

### Relevant Files

- `crates/openfang-api/src/routes.rs` — where new handlers are implemented
- `crates/openfang-api/src/server.rs` — where routes are registered in the Axum router
- `crates/openfang-api/src/types.rs` — where request/response types are defined for API payloads
- task 23 dispatch repository — `DispatchRepository` trait and `DispatchRecord`
- task 24 HITL repository — `HitlRepository` trait and `HitlRecord`
- task 30 resume path — the function/channel to trigger after `HitlRepository::answer`
- `tasks/prd-compozy/docs/API-SPEC.md` sections 9, 10, 11, 14 — canonical payload
  shapes, endpoint lists, filter parameters, SSE event names
- `tasks/prd-compozy/docs/adrs/026-runtime-execution-resource-surfaces.md` — mandates
  that dispatches and HITL requests are first-class public resources

### Dependent Files

- SSE/watch surface improvements (later task)
- final hardening task (task 29 or equivalent)
- future CLI surfaces (`compozy dispatches ...`, `compozy hitl ...`) — these will use the same
  public API paths; do not add internal-only endpoints

## Deliverables

- All dispatch endpoints from API-SPEC.md section 10 implemented and registered
- All HITL endpoints from API-SPEC.md section 11 implemented and registered
- Run-scoped dispatch and HITL sub-resources from API-SPEC.md section 9 implemented
- HITL answer endpoint triggering the task 30 runtime resume path
- Stub SSE handlers for dispatch and HITL event streams
- Response payloads matching API-SPEC.md shapes exactly (field names, types, envelope)
- End-to-end tests covering the full dispatch/HITL lifecycle through the API

## Tests

### Unit Tests (Required)

- [ ] `dispatch_list_response_should_match_api_spec_summary_shape` — call `GET /api/v1/dispatches`
      with a seeded repository and verify the response envelope and each item's fields match the
      summary shape from API-SPEC.md section 10 exactly.
- [ ] `dispatch_detail_response_should_include_all_required_fields` — call
      `GET /api/v1/dispatches/{id}` and verify every field in the detail shape is present, including
      `kind`, `attempt`, `parent_dispatch_id` (null when absent), `spawned_agent_id` (null when absent),
      and all timestamps.
- [ ] `hitl_answer_endpoint_should_trigger_dispatch_transition` — submit a valid answer via
      `POST /api/v1/hitl-requests/{id}/answer` and verify the `agent_dispatch` is `running` (or
      `completed` if execution finishes synchronously in the test) after the call returns.
- [ ] `hitl_list_with_status_filter_should_return_only_matching_records` — seed pending and
      answered HITL records; query with `status=pending` and verify only pending records are returned.
- [ ] `dispatch_cancel_should_cascade_to_linked_hitl_request` — cancel a dispatch that has a
      linked `pending` HITL request and verify the HITL request is `cancelled` after the dispatch
      cancel returns.
- [ ] `dispatch_retry_should_increment_attempt_counter` — retry a `failed` dispatch and verify
      the `attempt` field is incremented in the stored record or in the new retry record.
- [ ] `hitl_answer_on_non_pending_request_should_return_error` — attempt to answer an already-
      `answered` HITL request via the API and verify a 4xx error response is returned.
- [ ] `run_scoped_dispatch_list_should_return_404_for_missing_run` — call
      `GET /api/v1/runs/nonexistent/dispatches` and verify a 404 response.

### Integration Tests (Required)

- [ ] `dispatch_lifecycle_visible_through_api_after_runtime_execution` — start a real workflow
      run with a seeded agent, let the dispatch execute, and query `GET /api/v1/dispatches` to verify
      the completed dispatch record is present with correct status and result fields.
- [ ] `hitl_answer_end_to_end_through_api` — run a workflow step that emits a HITL question,
      submit the answer via `POST /api/v1/hitl-requests/{id}/answer`, and verify the step eventually
      completes and the workflow run advances to the next step.
- [ ] `dispatch_children_endpoint_should_return_child_dispatches` — create a dispatch with two
      child dispatches (via a `spawn` or multi-agent workflow), call
      `GET /api/v1/dispatches/{parent_id}/children`, and verify both children are returned.
- [ ] `internal_agent_can_use_hitl_api_to_answer_own_question` — demonstrate that an internal
      agent (not a human) can call `POST /api/v1/hitl-requests/{id}/answer` programmatically, which
      is a valid use case per the control-plane primacy principle in DESIGN.md section 2.
- [ ] `sse_dispatch_events_endpoint_should_return_snapshot_and_keepalive` — connect to
      `GET /api/v1/dispatches/{id}/events` and verify a `stream.snapshot` event is emitted followed
      by a `keepalive` heartbeat within the expected interval.

### Regression and Anti-Pattern Guards

- [ ] Do not create internal-only endpoints for dispatch or HITL control-plane actions — all
      actions (cancel, retry, answer) must go through the public `/api/v1` surface, which internal
      agents use through the same contracts as humans.
- [ ] Do not expose stale in-memory state in read endpoints — `GET /api/v1/dispatches/{id}` must
      always read from `DispatchRepository`, not from a kernel-held in-memory projection of the run.
- [ ] Do not hide response semantics behind ad hoc JSON blobs — the `status`, `kind`, and all
      structured fields must be explicit named fields in the response, not packed into a generic
      `metadata` blob.
- [ ] Do not return 200 with an empty body when a dispatch or HITL request is not found — return
      404 with a structured error response matching the API-SPEC.md error envelope.
- [ ] Do not trigger the HITL resume path from the answer endpoint without first verifying the
      HITL request is `pending` — a double-answer attempt must return an error, not silently fire the
      resume logic twice.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All endpoints listed in API-SPEC.md sections 10 and 11 are registered and return non-404
  responses.
- `GET /api/v1/dispatches` and `GET /api/v1/dispatches/{id}` return payloads that exactly match
  the summary and detail shapes in API-SPEC.md section 10, including all nullable fields.
- `GET /api/v1/hitl-requests` and `GET /api/v1/hitl-requests/{id}` return payloads that exactly
  match the detail shape in API-SPEC.md section 11, including `sequence_no` and `context`.
- `POST /api/v1/hitl-requests/{id}/answer` triggers the task 30 resume path — verified by the
  dispatch transitioning to `running` (or `completed` for fast agents) after the call.
- `POST /api/v1/dispatches/{id}/cancel` cascades to cancel linked pending HITL requests.
- `POST /api/v1/dispatches/{id}/retry` increments `attempt` and re-enqueues execution.
- The run-scoped sub-resources (`/runs/{id}/dispatches`, `/runs/{id}/hitl-requests`) return 404
  for unknown run IDs and correctly scoped results for known ones.
- Internal agents can use the same API endpoints as human operators without special routing.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Notes

- These surfaces complete the durable dispatch/HITL runtime slice. Together with tasks 19, 20,
  24, and 25, this task delivers the full observability and control-plane story for agent
  delegation and human interaction.
- CLI commands for dispatch and HITL management (`compozy dispatches ...`, `compozy hitl ...`)
  are deferred to future work and must not touch `openfang-cli` in this task. The CLI will be
  implemented as a client of these API endpoints.
- The SSE stub endpoints registered in subtask 33.8 must use the exact URL paths from API-SPEC.md
  section 14 (`GET /api/v1/dispatches/{id}/events`, `GET /api/v1/hitl-requests/stream`) so that
  future SSE implementation does not require route changes.
- The control-plane primacy principle (DESIGN.md section 2) means the answer and cancel endpoints
  are not "human-only" — internal agents submitting answers to their own HITL questions is a
  valid and expected use case. The handler must not reject programmatic callers.
