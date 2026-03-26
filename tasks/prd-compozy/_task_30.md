## markdown

## status: completed

<task_context>
<domain>engine/hitl/runtime</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task24,task29</dependencies>
</task_context>

# Task 30.0: HITL Single-Turn Live Pause And Resume

## Overview

Implement the core HITL mid-step pause and resume mechanism using a `tokio::oneshot` channel
approach. When an agent executing inside a workflow step asks a clarification question, the
runtime must:

1. Create a `hitl_request` record (`status = pending`), linked to the active `agent_dispatch`.
2. Transition the `agent_dispatch` to `status = waiting_hitl` atomically with step 1.
3. Create a `tokio::oneshot::channel()` and register the `Sender` in a
   `HashMap<HitlRequestId, oneshot::Sender<HitlAnswer>>` on the workflow engine.
4. Write a checkpoint with `kind = hitl_requested`.
5. Await the `Receiver` future, suspending the step without terminating it.
6. When a human submits an answer through the API, transition the `hitl_request` to
   `status = answered` and the `agent_dispatch` back to `status = running`.
7. Look up the `Sender` in the HashMap, send the answer through the oneshot channel.
8. The step resumes with the answer injected as input, continuing the same step.

Per ADR-018 (HITL Can Occur Inside Active Agent Steps), this is distinct from `wait_signal` at
the workflow level. The step does not complete -- it pauses mid-execution while the dispatch record
holds `waiting_hitl` and the workflow run's `active_hitl_request_id` points at the pending
question. The workflow's `current_step_id` does not advance. When execution resumes, the agent
continues from where it left off, with the human's answer available in the continuation context.

Multiple HITL interactions can occur within a single step. After the first answer resumes
execution, the agent may ask another question (`sequence_no = 2`), pause again, and resume again.
This task covers the live process path where the oneshot channel is available in memory.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start -- **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement a runtime signal interface using the `tokio::oneshot` channel approach: the step
  executor creates a `tokio::oneshot::channel()` before beginning agent execution, registers the
  `Sender<HitlAnswer>` in a `HashMap<HitlRequestId, oneshot::Sender<HitlAnswer>>` on the
  workflow engine, and the agent execution path can signal a HITL question through this mechanism.
- The pause side of the HITL cycle must: (a) assign the next `sequence_no` via
  `HitlRepository::create`, (b) transition the dispatch to `waiting_hitl` via
  `DispatchRepository::update_status`, (c) update `workflow_run.active_hitl_request_id` via
  `WorkflowRunRepository::set_active_hitl_request`, (d) write a checkpoint with
  `kind = hitl_requested`, and (e) register the oneshot `Sender` in the engine's HashMap.
  All database writes must succeed before the step executor suspends on the `Receiver` future.
- The workflow run's `active_hitl_request_id` column in `workflow_run` must be updated to point at
  the pending `hitl_request_id` when a dispatch enters `waiting_hitl`. It must be cleared (set to
  null) when the HITL interaction completes and execution resumes.
- The resume path -- triggered when a human answers via the API -- must: (a) transition the
  `hitl_request` to `answered`, (b) transition the `agent_dispatch` back to `running`, (c) clear
  `workflow_run.active_hitl_request_id`, (d) look up the `Sender` in the HashMap and send the
  answer through the oneshot channel, and (e) the step executor's awaited `Receiver` resolves
  with the answer, which is injected as the next input turn to the agent loop. All database
  writes must be transactional -- if any step fails, no partial state update must be committed.
- Multiple sequential HITL interactions within one step must work correctly. After a resume, the
  agent must be able to ask a second question (`sequence_no = 2`), which follows the same
  pause/resume cycle with a new oneshot channel. The step executor must handle this as a second
  pause/resume cycle without creating a new dispatch record. The existing `dispatch_id` and
  `run_id` are reused; only `hitl_request_id` changes.
- HITL must not advance the workflow's `current_step_id`. The step remains active throughout the
  pause/resume cycle. Only after the agent completes the step's execution (no more questions) does
  the workflow advance to the next step.
- The `wait_signal` workflow step kind is a different mechanism for workflow-level waiting and
  must not be confused with or conflated with in-step HITL. The two mechanisms use different
  database columns (`waiting_kind` vs `active_hitl_request_id`) and different code paths.
  A run must not have both `waiting_kind = "signal"` and `active_hitl_request_id` set
  simultaneously. The step executor must enforce this.
</requirements>

## Subtasks

- [x] 30.1 Design and implement the oneshot channel-based HITL signal interface. The step executor
      creates a `tokio::oneshot::channel()` before beginning agent execution. The `Sender` is
      registered in a `HashMap<HitlRequestId, oneshot::Sender<HitlAnswer>>` held on the workflow
      engine (or a dedicated `HitlRegistry` struct). The agent execution path receives a handle
      (e.g., `HitlEmitter`) that can signal a HITL question. Evaluate the existing
      `crates/openfang-runtime/src/hooks.rs` for extension points before designing a new mechanism.

- [x] 30.2 Implement the pause side of the HITL cycle. When the HITL signal is received by the
      step executor: (a) assign the next `sequence_no` via `HitlRepository::create`, (b) transition
      the dispatch to `waiting_hitl` via `DispatchRepository::update_status`, (c) update
      `workflow_run.active_hitl_request_id` via `WorkflowRunRepository::set_active_hitl_request`,
      (d) write a checkpoint with `kind = hitl_requested`, and (e) register the oneshot `Sender`
      in the engine's HashMap. All database writes must succeed before the step executor suspends
      on the `Receiver` future.

- [x] 30.3 Implement the resume path triggered by `HitlRepository::answer`. After the answer is
      written: (a) transition the dispatch back to `running`, (b) clear
      `workflow_run.active_hitl_request_id`, (c) look up the `Sender` in the HashMap and send the
      answer, and (d) the step executor's `Receiver` resolves with the answer, which is injected
      as a continuation turn (user input message) into the agent loop. The step executor then
      continues execution.

- [x] 30.4 Implement the multi-turn HITL loop within a single step. After the first resume, the
      agent loop must be able to emit a second HITL signal (`sequence_no = 2`) with the same dispatch
      context. A new oneshot channel is created for each question. The step executor must handle this
      as a second pause/resume cycle without creating a new dispatch record. The existing `dispatch_id`
      and `run_id` are reused; only `hitl_request_id` changes.

- [x] 30.5 Implement the mutual exclusion guard between `wait_signal` and in-step HITL. The step
      executor must return an error if both `waiting_kind` and `active_hitl_request_id` would be set
      simultaneously on the same run.

- [x] 30.6 Write unit tests covering the full pause/resume cycle, the multi-turn case, and the
      guard against `wait_signal` conflation. See Tests section below.

- [x] 30.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

The old clarification model in the TypeScript codebase treated HITL as a tool result that blocked
on an external channel. The `clarification` tool would call the UI layer synchronously, and the
tool runner would block its thread until an answer arrived. This worked for the single-user desktop
context but has two fatal problems in the new model: it is not restartable (the blocking channel
is in-memory) and it is not observable through the control plane.

The new model separates the HITL interaction from the tool mechanism entirely. HITL is a runtime
concept, not a tool-level concept. The agent does not "call a clarification tool" -- the agent's
reasoning produces output that the runtime recognizes as a HITL question, creates a durable
record, and suspends the step until a human answers through the API.

### Oneshot Channel Approach (Decided)

Per the design decisions document, the mechanism uses `tokio::oneshot`:

1. Step creates a `tokio::oneshot::channel()`
2. Registers `Sender` in a `HashMap<HitlRequestId, oneshot::Sender<HitlAnswer>>` on the workflow
   engine
3. Writes `hitl_request` row to `compozy.db` with status `pending`
4. Writes checkpoint `kind = hitl_requested`
5. Awaits the `Receiver` future

When the answer arrives via API:

1. Updates `hitl_request.status` to `answered`, writes `response_payload`
2. Looks up `Sender` in the HashMap, sends the answer
3. Step resumes with the answer

The suspension mechanism means the step executor's `tokio::task` remains alive but suspended --
it does not exit and get reconstructed on resume. The task remains in the tokio runtime's wait
queue until the answer arrives and the sender half of the channel fires.

The distinction between in-step HITL and `wait_signal` is enforced at the data model level:

- `wait_signal` sets `workflow_run.waiting_kind = "signal"` and `workflow_run.waiting_ref` to the
  expected signal name. The workflow step completes and the run waits at the step boundary.
- In-step HITL sets `workflow_run.active_hitl_request_id` to a non-null HITL request ID. The
  workflow step does NOT complete -- `current_step_id` remains the active step. The dispatch is
  `waiting_hitl`.

These two waiting modes are mutually exclusive. A run must not have both `waiting_kind = "signal"`
and `active_hitl_request_id` set simultaneously. The step executor must enforce this.

### Relevant Files

- `crates/openfang-runtime/src/agent_loop.rs` -- agent execution loop; HITL signal must integrate
  here or intercept its output
- `crates/openfang-runtime/src/hooks.rs` -- existing hook extension points; evaluate for reuse
- `crates/openfang-kernel/src/workflow.rs` -- step executor; pause/resume logic lives here
- Task 23 dispatch repository -- `DispatchRepository::update_status` (to/from `waiting_hitl`)
- Task 24 HITL repository -- `HitlRepository::create`, `HitlRepository::answer`
- `tasks/prd-compozy/docs/API-SPEC.md` section 9 -- `workflow_run.active_hitl_request_id`
  field in the run detail shape; section 10 -- dispatch `status = waiting_hitl`; section 11 --
  HITL answer request

### Dependent Files

- Task 33: the HITL answer API endpoint triggers the resume path implemented here
- Task 19: restart recovery logic must be updated to treat `waiting_hitl` + non-null
  `active_hitl_request_id` as a safe stable state requiring no automatic recovery action
- Task 31: post-restart reconstruction builds on the durable state written by this task

## Deliverables

- Oneshot channel-based runtime signal mechanism allowing the agent to emit a HITL question
  during step execution
- `HitlRegistry` (or equivalent) holding `HashMap<HitlRequestId, oneshot::Sender<HitlAnswer>>`
  on the workflow engine
- Atomic pause: `hitl_request` creation + dispatch transition to `waiting_hitl` + run update +
  checkpoint write + oneshot sender registration
- Resume path: answer arrival triggers dispatch transition back to `running`, oneshot send,
  and continuation turn injection
- Multi-turn support: second HITL question in the same step follows the same cycle with a new
  oneshot channel
- Guard against `wait_signal` conflation: the two waiting modes are mutually exclusive and
  enforced at the step executor level

## Tests

### Unit Tests (Required)

- [x] `hitl_pause_should_create_request_and_transition_dispatch_atomically` -- invoke the HITL
      signal in a test step executor; verify `hitl_request` is `pending` and `agent_dispatch` is
      `waiting_hitl` after the signal, with neither being updated partially.
- [x] `hitl_pause_should_set_run_active_hitl_request_id` -- after a HITL pause, verify that
      `workflow_run.active_hitl_request_id` matches the created `hitl_request_id`.
- [x] `hitl_pause_should_register_oneshot_sender_in_registry` -- after a HITL pause, verify that
      the `HitlRegistry` contains an entry for the `hitl_request_id`.
- [x] `hitl_resume_should_transition_dispatch_back_to_running` -- trigger the resume path by
      answering the HITL request; verify the dispatch is back to `running` and
      `active_hitl_request_id` is null.
- [x] `hitl_resume_should_inject_answer_into_continuation_context` -- after resume, verify the
      answer text is present in the next turn submitted to the agent loop (inspect the constructed
      `TurnContext` or equivalent).
- [x] `hitl_resume_should_send_answer_through_oneshot_channel` -- verify the oneshot `Receiver`
      resolves with the correct `HitlAnswer` when the answer API handler sends through the `Sender`.
- [x] `hitl_second_question_in_same_step_should_reuse_dispatch_id` -- emit two HITL signals in
      sequence for the same step; verify both `hitl_request` records reference the same `dispatch_id`
      and have `sequence_no` 1 and 2 respectively.
- [x] `hitl_step_id_should_not_advance_during_pause` -- pause on HITL and verify
      `workflow_run.current_step_id` is unchanged after the pause.
- [x] `hitl_should_not_be_conflatable_with_wait_signal` -- attempt to set both `waiting_kind`
      and `active_hitl_request_id` on the same run and verify the step executor returns an error.

### Integration Tests (Required)

- [x] `hitl_end_to_end_pause_and_resume_in_single_step` -- run a workflow step with a test agent
      that emits a HITL question, submit an answer through the HITL repository, and verify the step
      completes successfully with the answer injected into the continuation context.
- [x] `hitl_multiple_turns_end_to_end` -- run a step that emits two sequential HITL questions,
      answer each in turn, and verify the step completes with both answers available in the agent's
      context at the end of execution.
- [x] `hitl_cancelled_request_should_fail_dispatch` -- cancel a pending HITL request via
      `HitlRepository::cancel`; verify the dispatch transitions to `failed` and the workflow run is
      updated to reflect the failure.

### Regression and Anti-Pattern Guards

- [x] Do not implement HITL clarification as a separate synthetic workflow step -- creating a new
      `wait_signal` step for each question breaks the mid-step semantics required by ADR-018 and
      causes `current_step_id` to advance incorrectly.
- [x] Do not conflate `wait_signal` (workflow-level waiting) with in-step HITL (`waiting_hitl`
      dispatch status + `active_hitl_request_id`) -- they use different fields and different code paths.
- [x] Do not allow the step executor to advance `current_step_id` while `active_hitl_request_id`
      is non-null -- add an assertion or guard in the step completion path.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- An agent executing inside a workflow step can pause mid-step for a HITL question and resume
  with the answer injected via the oneshot channel, without the step being restarted from the
  beginning.
- The pause is durable: `hitl_request` and `agent_dispatch` records reflect the waiting state in
  `compozy.db` before the oneshot `Receiver` is awaited.
- The resume is transactional: all database writes (answer, dispatch status, run update) either
  all succeed or none take partial effect, and the oneshot channel delivers the answer to the
  suspended step.
- Multiple HITL interactions within one step work correctly, with `sequence_no` correctly
  ordering the questions and each answer being injected into the right continuation point.
- The distinction between `wait_signal` and in-step HITL is enforced at the step executor level
  and tested explicitly.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Prior Implementation Reference

The old TypeScript codebase implements the clarification/HITL flow end-to-end:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/clarification/` -- Clarification tool showing pause/resume interaction patterns
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/hitl/` -- Frontend HITL interaction system

The old model handled HITL as a tool-level concern within the provider. The new model makes it a
durable runtime concept -- the old code is useful for understanding the user-facing pause/resume UX
and how question/answer cycles interleave with active execution.

## Notes

- This task covers only the live process path where the oneshot channel is available in memory.
  Post-restart reconstruction (where no live channel exists) is handled by Task 31.
- The choice of `tokio::oneshot` is a decided design decision (see design decisions document).
- Timeout enforcement (`timeout_at` column from Task 24) is deferred. The column is present; a
  background task that monitors `pending` HITL requests past their timeout will be added in a
  later task. Do not implement timeout expiry in this task.
