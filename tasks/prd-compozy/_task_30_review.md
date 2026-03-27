# Task 30 Review: HITL Single-Turn Live Pause And Resume

## Status: PASS

## Checklist
- [x] `HitlRegistry` struct defined with `HashMap<HitlRequestId, oneshot::Sender<HitlAnswer>>`
- [x] Oneshot channel created before agent execution begins for each dispatch
- [x] `Sender` registered in `HitlRegistry` via `register()` method
- [x] `HitlAnswer` type defined with `continuation_message()` method
- [x] Pause side: `HitlRepository::create` assigns `sequence_no`
- [x] Pause side: `DispatchRepository::update_status` transitions to `waiting_hitl`
- [x] Pause side: `workflow_run.active_hitl_request_id` updated to pending request ID
- [x] Pause side: checkpoint written with `kind = hitl_requested`
- [x] Pause side: oneshot `Sender` registered in `HitlRegistry`
- [x] All pause-side DB writes succeed before step executor suspends on `Receiver`
- [x] Resume side: `hitl_request` transitioned to `answered`
- [x] Resume side: `agent_dispatch` transitioned back to `running`
- [x] Resume side: `workflow_run.active_hitl_request_id` cleared (set to null)
- [x] Resume side: `Sender` looked up in `HitlRegistry` and answer sent through channel
- [x] `Receiver` resolves with `HitlAnswer`; answer injected as continuation input to agent loop
- [x] Multi-turn: second HITL question in same step uses same `dispatch_id`, new `hitl_request_id`, `sequence_no = 2`
- [x] Multi-turn: new oneshot channel created for each question
- [x] `workflow_run.current_step_id` does NOT advance during HITL pause
- [x] Mutual exclusion guard: `waiting_kind = "signal"` + `active_hitl_request_id` non-null simultaneously raises error
- [x] `active_hitl_request_id` cleared on step completion (all transition paths)
- [x] Distinct code paths for `wait_signal` (workflow-level) and in-step HITL (`waiting_hitl` dispatch status)
- [x] Resume writes are transactional (all DB writes or none)
- [x] Unit test: `hitl_pause_should_create_request_and_transition_dispatch_atomically`
- [x] Unit test: `hitl_pause_should_set_run_active_hitl_request_id`
- [x] Unit test: `hitl_pause_should_register_oneshot_sender_in_registry`
- [x] Unit test: `hitl_resume_should_transition_dispatch_back_to_running`
- [x] Unit test: `hitl_resume_should_send_answer_through_oneshot_channel`
- [x] Unit test: `hitl_second_question_in_same_step_should_reuse_dispatch_id`
- [x] Unit test: `hitl_step_id_should_not_advance_during_pause`
- [x] Unit test: `hitl_should_not_be_conflatable_with_wait_signal`
- [x] Unit test: `hitl_resume_should_inject_answer_into_continuation_context`
- [x] Integration test: `hitl_end_to_end_pause_and_resume_in_single_step`
- [x] Integration test: `hitl_multiple_turns_end_to_end`
- [x] Integration test: `hitl_cancelled_request_should_fail_dispatch`

## Findings

**Implemented correctly:**
- The full oneshot channel-based HITL mechanism is implemented in `workflow.rs`. `HitlRegistry` holds a `Mutex<HashMap<String, oneshot::Sender<HitlAnswer>>>` on the `WorkflowEngine`.
- The pause path (`request_hitl_pause`) performs all 5 required DB writes atomically before suspending: creates the `hitl_request` row, transitions the dispatch to `waiting_hitl`, updates `workflow_run.active_hitl_request_id`, writes a checkpoint with `kind = hitl_requested`, and registers the oneshot `Sender`. The `wait_for_hitl_answer` method then awaits the `Receiver`.
- The resume path (`answer_live_hitl` in tests / API handler path) transitions `hitl_request` to `answered`, transitions `agent_dispatch` back to `running`, clears `active_hitl_request_id`, and delivers the answer through the oneshot channel.
- The answer is injected as a continuation input via `HitlAnswer::continuation_message()`, which produces the user message fed to the agent loop.
- Multi-turn HITL within a single step is supported: the second question reuses the same `dispatch_id` and gets a new `hitl_request_id` with `sequence_no = 2`. This is verified by `hitl_second_question_in_same_step_should_reuse_dispatch_id`.
- The guard against `wait_signal` conflation is enforced in `ensure_run_can_enter_waiting_mode` (which checks both `waiting_kind` and `active_hitl_request_id` simultaneously).
- `active_hitl_request_id` is explicitly cleared in all terminal transition paths for workflow runs (cancel, complete, fail, pause/resume boundaries).
- The `hitl_cancelled_request_should_fail_dispatch` integration test verifies that a cancelled HITL request transitions the dispatch to `failed` and updates the workflow run.

**Minor observations:**
- All 9 unit tests and 3 integration tests required by the spec are present, with exact names matching the spec.
- The `hitl_resume_should_inject_answer_into_continuation_context` test is present (line 6160) and verifies the answer text appears in the `captured_continuations` vector after step completion.
- Timeout enforcement (`timeout_at` column) is explicitly deferred per the spec's Notes section — this is correctly unimplemented and not a gap.
- The implementation uses a `Mutex<HashMap<...>>` rather than a `DashMap` for the registry, which is acceptable since the registry operations are short-lived and never held across `.await` points.

**Code quality:**
- The `HitlAnswer` type carries structured data rather than raw strings, making the continuation context injection type-safe.
- All DB-write sequences in the pause/resume paths are wrapped in the appropriate transaction semantics.
- No `unwrap()` in production paths; all errors propagate through `TransitionError`.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow.rs` (lines ~778–860, ~1507–1560, ~1600–1760, ~5234–6600, ~6160–6270, ~6269–6600)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/hitl.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/dispatch.rs`
