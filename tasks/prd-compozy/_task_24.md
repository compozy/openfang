## markdown

## status: pending

<task_context>
<domain>engine/hitl/schema</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task9,task19</dependencies>
</task_context>

# Task 24.0: hitl_request Schema And Persistence Layer

## Overview

Add the durable schema and persistence layer for `hitl_request` in `compozy.db`. This table is
the mechanism by which human-in-the-loop interactions become first-class runtime objects rather
than ephemeral tool-level side effects. Per ADR-007 (Autonomous By Default, Explicit HITL Only)
and ADR-018 (HITL Can Occur Inside Active Agent Steps), HITL is not an approval gate applied to
tools but a deliberate product-level pause that can occur inside an active `agent` step while
the step's dispatch remains in `waiting_hitl` state.

A single agent step can issue multiple sequential HITL interactions — for example, a PRD-writing
agent asking one clarification question, getting an answer, then asking a follow-up before
completing the step. The `sequence_no` column makes this ordering durable and queryable. The
`dispatch_id` foreign reference ties each HITL request to the specific `agent_dispatch` that
created it, enabling the runtime in task 30 to resume the correct execution context.

This task delivers only the schema and repository. Runtime pause/resume semantics land in task 30.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add the `hitl_request` table to `compozy.db` via a versioned migration following the same
  incremental version-gate pattern in `crates/openfang-memory/src/migration.rs`. The migration
  must be idempotent and must not disturb earlier schema versions.
- The schema must include all columns from `DATABASE-SCHEMA.md` section 3: `hitl_request_id`,
  `run_id`, `step_id`, `dispatch_id`, `kind`, `status`, `question`, `context_json`,
  `response_json`, `sequence_no`, `created_at`, `answered_at`, `timeout_at`. The `dispatch_id`
  column is nullable because a HITL request may originate outside a dispatch context in future
  extensions, but it is expected to always be populated for in-step HITL as described in ADR-018.
- Implement the HITL status lifecycle as a typed Rust enum: `pending`, `answered`, `cancelled`,
  `timed_out`. Status transitions must be enforced at the repository boundary: `pending` is the
  only legal initial state; `answered`, `cancelled`, and `timed_out` are terminal and may not
  transition further.
- Implement HITL kind as a typed enum covering at minimum `clarification`. Additional kinds
  (`approval`, `choice`, `freeform`) may be stubs for now but must be represented so the schema
  accommodates future extension without migration.
- The `sequence_no` field enables ordering within a single step and dispatch. The repository must
  expose a method to assign the next sequence number for a given `(run_id, step_id, dispatch_id)`
  tuple atomically. Gaps in sequence numbers must not occur due to concurrent inserts.
- Implement a `HitlRepository` with: `create`, `find_by_id`, `find_pending_for_run`,
  `find_by_dispatch`, `answer`, `cancel`, `mark_timed_out`. The `answer` method must write
  `response_json` and `answered_at` atomically in the same SQLite write transaction.
- Indexes must cover: `(run_id)` for run-scoped queries, `(dispatch_id)` for dispatch-linked
  lookups, `(status)` for pending-request enumeration, and `(run_id, step_id, sequence_no)` for
  ordered multi-question retrieval within a step.
</requirements>

## Subtasks

- [ ] 24.1 Add a new `compozy.db` migration function that creates the `hitl_request` table with
      all required columns and the four indexes listed in the requirements. Record the migration in
      the migration log. Verify idempotency on repeat application and clean upgrade from earlier
      schema versions.

- [ ] 24.2 Define `HitlStatus` enum (`Pending`, `Answered`, `Cancelled`, `TimedOut`) and
      `HitlKind` enum (`Clarification`, and stubs for `Approval`, `Choice`, `Freeform`) in the
      appropriate types module. Implement `Display`, `FromStr`, and `serde` derives. The enums must
      serialize to/from the snake_case strings used in the API spec (`pending`, `answered`,
      `clarification`, etc.).

- [ ] 24.3 Define `HitlRecord` struct covering all schema columns. JSON columns (`context_json`,
      `response_json`) must use `serde_json::Value`. Timestamps must use `chrono::DateTime<Utc>` with
      RFC 3339 serialization consistent with API-SPEC.md conventions.

- [ ] 24.4 Implement the `HitlRepository` async trait and its SQLite-backed implementation.
      Follow the same structural pattern as `crates/openfang-memory/src/structured.rs` and the async
      trait pattern from `crates/arky-session/src/store.rs`. The `sequence_no` assignment must be
      atomic: use a `SELECT MAX(sequence_no) ... + 1` within the same write transaction as the insert.

- [ ] 24.5 Implement the `answer` method as a transactional write: set `response_json`,
      `answered_at`, and `status = answered` in one SQLite transaction. If the request is not in
      `pending` state, the method must return a typed error rather than a silent no-op.

- [ ] 24.6 Write unit tests using in-memory SQLite. Cover all repository operations, all status
      transitions (legal and illegal), sequence number ordering for multiple questions in one step,
      and the atomicity of the answer operation.

- [ ] 24.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

The old clarification pattern in the TypeScript codebase treated HITL as a tool invocation that
blocked on a channel until a user responded. This meant the HITL state was entirely ephemeral —
a restart during a pending clarification would lose the question, the context, and any partial
answer. The new `hitl_request` table makes all of this durable.

The key difference from the old model is that `hitl_request` links upward through three levels:

- `run_id` — the durable workflow run this question belongs to
- `step_id` — the logical step (e.g., `write-prd`) within that run
- `dispatch_id` — the specific `agent_dispatch` record that issued the question

This three-level linkage is what makes task 30's mid-step pause/resume possible. When the runtime
needs to resume after an answer is provided, it looks up the `dispatch_id` from the answered HITL
request, reloads the dispatch record (which has `status = waiting_hitl`), transitions it back to
`running`, and injects the answer into the agent's execution context.

The `sequence_no` field handles the multi-question case. When a PRD-writing agent asks "Should I
prioritize B2B admins or end users?" and later asks "Which timeline is acceptable?", those two
HITL interactions within the same step get `sequence_no = 1` and `sequence_no = 2` respectively.
The sequence number is scoped to `(run_id, step_id, dispatch_id)` — different steps or dispatches
restart at 1.

The `context_json` field carries structured context the agent provides alongside the question —
for example, the artifact ID being written, the current draft section, or a list of candidate
choices. This context is surfaced to the human through the API's HITL detail shape (API-SPEC.md
section 11). It must be stored and returned faithfully; the repository must not interpret it.

The `response_json` field holds the human's structured answer, also surfaced in the API shape.
For a `clarification` kind the response might be `{"type": "text", "value": "B2B admins first"}`.
For a `choice` kind it might reference a named option. The repository stores this as an opaque
`serde_json::Value`; the runtime in task 30 is responsible for interpreting it.

The old `ApprovalManager` in `crates/openfang-kernel/src/approval.rs` is a completely different
subsystem managing tool-level approval gates. It must not be reused or referenced for HITL. Per
ADR-007, the approval system is a legacy mechanism that HITL does not inherit from.

### Relevant Files

- `tasks/prd-compozy/docs/DATABASE-SCHEMA.md` — canonical column set for
  `hitl_request`
- `tasks/prd-compozy/docs/API-SPEC.md` section 11 — HITL detail shape and endpoints
  that this schema must back, including `sequence_no` in the response payload
- `tasks/prd-compozy/docs/adrs/007-autonomous-by-default-explicit-hitl.md` — HITL
  is a first-class product primitive, not a tool-level approval gate
- `tasks/prd-compozy/docs/adrs/018-hitl-can-occur-inside-active-agent-steps.md` —
  the `dispatch_id` link and mid-step semantics
- `crates/openfang-memory/src/migration.rs` — migration versioning pattern to follow
- `crates/openfang-memory/src/structured.rs` — repository implementation pattern
- `crates/arky-session/src/store.rs` — async trait and error pattern to follow

### Dependent Files

- `crates/openfang-memory/src/migration.rs` or equivalent — gains a new version step
- new HITL module in the appropriate memory/db crate
- task 30: mid-step pause/resume reads and writes these records
- task 33: API handlers expose these records through `/api/v1/hitl-requests`

## Deliverables

- `compozy.db` migration adding the `hitl_request` table with all required columns and indexes
- `HitlStatus` and `HitlKind` typed enums with serde and display support
- `HitlRecord` struct and `HitlRepository` async trait
- SQLite-backed repository implementation with atomic answer and sequence assignment
- Legal status-transition enforcement at the repository boundary
- Full unit test coverage for schema, CRUD operations, sequence numbering, answer atomicity, and
  transition guards

## Tests

### Unit Tests (Required)

- [ ] `hitl_record_should_persist_all_required_fields` — create a HITL record and reload it;
      verify every column round-trips including `context_json`, `response_json`, `sequence_no`,
      and all nullable timestamp fields.
- [ ] `hitl_sequence_numbers_should_be_ordered_within_step` — create three HITL records for the
      same `(run_id, step_id, dispatch_id)` tuple and verify they receive `sequence_no` 1, 2, 3 in
      insertion order.
- [ ] `hitl_sequence_numbers_should_restart_across_steps` — create HITL records for two different
      step IDs and verify each starts at `sequence_no = 1` independently.
- [ ] `hitl_answer_should_write_response_and_timestamp_atomically` — create a pending request,
      call `answer`, and verify `response_json`, `answered_at`, and `status = answered` are all
      written together.
- [ ] `hitl_answer_should_fail_on_non_pending_request` — attempt to answer a request that is
      already `answered` or `cancelled` and verify a typed error is returned.
- [ ] `hitl_status_terminal_states_should_not_transition` — attempt to cancel an `answered`
      request and verify the repository rejects the transition.
- [ ] `hitl_find_pending_for_run_should_return_only_pending` — insert a mix of pending and
      answered HITL records for the same run; `find_pending_for_run` must return only pending ones.
- [ ] `hitl_find_by_dispatch_should_scope_correctly` — insert records for two dispatch IDs;
      `find_by_dispatch` must return only those belonging to the queried dispatch.

### Integration Tests (Required)

- [ ] `compozy_db_migration_should_add_hitl_table_cleanly` — open an in-memory `compozy.db`,
      run all migrations, and verify the `hitl_request` table and all required indexes exist.
- [ ] `compozy_db_migration_should_be_idempotent_with_hitl_table` — run migrations twice and
      verify no error and no duplicate tables.
- [ ] `hitl_repository_should_survive_connection_restart` — write a HITL record with a pending
      answer, drop and re-open the SQLite connection, and verify the record is still present with the
      same state.
- [ ] `hitl_repository_sequence_assignment_should_be_race_safe` — simulate two concurrent inserts
      for the same step and verify the resulting sequence numbers are distinct (1 and 2, not both 1).

### Regression and Anti-Pattern Guards

- [ ] Do not model HITL using or extending the old `ApprovalManager` in
      `crates/openfang-kernel/src/approval.rs` — they are different concepts with different semantics.
- [ ] Do not bury HITL state only inside `workflow_checkpoint.data_json` or `agent_dispatch`
      payload columns — `hitl_request` must exist as its own first-class table.
- [ ] Do not lose the `dispatch_id` linkage — without it, task 30 cannot resume the correct
      execution context after an answer is received.
- [ ] Do not treat `sequence_no` as optional or auto-generated outside the repository — the
      atomic assignment in the repository is the sole authority.
- [ ] Do not use `unwrap()` in repository code — all SQLite errors must propagate as typed errors.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- The `hitl_request` table exists in `compozy.db` with all columns and indexes defined in
  `DATABASE-SCHEMA.md` section 3.
- The `HitlStatus` and `HitlKind` enums serialize to the snake_case strings used in API-SPEC.md
  section 11 without manual string mapping.
- Terminal status states (`answered`, `cancelled`, `timed_out`) cannot be overwritten by the
  repository — the enforcement is in the write path, not in callers.
- The `sequence_no` assignment is atomic: concurrent inserts for the same step never collide.
- The `answer` method writes `response_json`, `answered_at`, and `status` in a single transaction.
- Task 30 can begin immediately without any additional schema work.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Prior Implementation Reference

The old TypeScript codebase implements HITL as a "clarification" tool:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/clarification/` — HITL/clarification tool implementation showing pause/resume patterns

In the old model, clarification was a tool-level concern. In the new model, `hitl_request` is a
first-class durable runtime concept with its own table, status lifecycle, and linkage to run/step/dispatch.
The old code is useful for understanding the user-facing interaction patterns and question/answer flow.

## Notes

- This is the storage half of HITL, not the full runtime behavior.
- The `compozy.db` migration version must be coordinated with task 23 to avoid numbering conflicts.
  Task 23 adds `agent_dispatch`; task 24 adds `hitl_request` — these should be consecutive versions.
- The `timeout_at` column is stored but not enforced by the repository. Timeout enforcement is a
  runtime concern and is deferred to task 30. The column is present so the schema is stable when
  timeout enforcement lands.
