## markdown

## status: completed

<task_context>
<domain>engine/hitl/recovery</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task30</dependencies>
</task_context>

# Task 31.0: HITL Post-Restart Reconstruction

## Overview

Implement the post-restart recovery path for HITL interactions. When the process restarts while
a HITL request is pending, the in-memory oneshot channel from Task 30 is gone. This task handles
the reconstruction: the recovery scan detects `hitl_request` rows with `status = pending` and
`agent_dispatch` rows with `status = waiting_hitl`, leaves them intact (no automatic re-execution),
and when a human eventually answers through the API, the workflow engine re-executes the step from
the checkpoint rather than relying on a live suspended task.

This is the complement to Task 30's live channel path. Task 30 handles the happy path where the
oneshot channel is in memory. This task handles the case where the process has restarted and no
live channel exists.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start -- **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- A process restart during a pending HITL interaction must be safe. On restart, the run's
  `active_hitl_request_id` is non-null and the dispatch is `waiting_hitl`. The restart recovery
  scan (from Task 19) must recognize this state and leave it intact -- it must not attempt to
  re-execute the step or overwrite the pending request. The system waits for a human answer before
  resuming.
- Implement the post-restart resume path in the answer API handler. When an answer arrives for a
  HITL request that has no live oneshot channel (the HashMap lookup fails because the process
  restarted), the handler must: (a) answer the `hitl_request` (Task 24's `HitlRepository::answer`),
  (b) transition the dispatch to `running` (Task 23's `DispatchRepository::update_status`),
  (c) clear `workflow_run.active_hitl_request_id`, (d) load the provider session using the
  `session_id` from the dispatch record via `arky_session::SessionStore::load` or the OpenFang
  memory session store, (e) construct a continuation turn that includes the answer as a user input
  message, and (f) re-execute the step from the checkpoint. The step must be idempotent up to the
  HITL request point.
- The recovery scan extensions must: (a) detect runs with non-null `active_hitl_request_id` and
  dispatch status `waiting_hitl`, (b) log these as stable waiting states (not errors), and
  (c) skip them in the recovery queue -- they require no automatic recovery action.
- Multi-turn HITL conversation after restart must work: if the agent asks a second question after
  the post-restart resume, Task 30's live oneshot channel path takes over for subsequent
  questions (since the process is now running and the step executor is alive).
- The two-branch resume path (live oneshot channel vs post-restart reconstruction) must be clearly
  separated in code. The answer handler checks whether a live sender exists in the HashMap; if
  yes, it uses Task 30's live path; if no, it uses this task's reconstruction path.
</requirements>

## Subtasks

- [x] 31.1 Implement the restart recovery guard. On process startup, if a workflow run has a
      non-null `active_hitl_request_id` and its dispatch is `waiting_hitl`, the recovery logic (from
      Task 19) must leave these records intact and not attempt re-execution. Add detection for
      `waiting_hitl` dispatch status as a recognized stable state in the recovery scan. Log these
      as informational ("Run {id} has pending HITL request {hitl_id}, awaiting human answer").

- [x] 31.2 Implement the post-restart resume path in the answer API handler. When an answer
      arrives and no live oneshot `Sender` is found in the `HitlRegistry` HashMap: (a) answer
      the `hitl_request` in the database, (b) transition the dispatch back to `running`,
      (c) clear `workflow_run.active_hitl_request_id`, (d) load the provider session from the
      durable session store using the `session_id` from the dispatch record, (e) construct a
      continuation turn with the answer as user input, and (f) reconstruct and re-execute the
      step executor from the last checkpoint. The step execution resumes from the HITL request
      point, not from the beginning of the step.

- [x] 31.3 Ensure provider session context is preserved across restart. For Arky-backed providers,
      the `SessionId` written in Task 29 must be sufficient to resume the session via
      `arky_session::SessionStore::load`. For the OpenFang LLM driver, the canonical session in
      `openfang_memory::session` must be checkpointed so it is reloadable after suspension. Verify
      that the session loaded post-restart contains the full conversation context up to the HITL
      pause point.

- [x] 31.4 Implement the two-branch dispatch in the answer handler. The handler must:
      (a) check whether a live oneshot `Sender` exists in the `HitlRegistry` for this
      `hitl_request_id`, (b) if yes, use Task 30's live channel path (send answer through oneshot),
      (c) if no, use this task's reconstruction path (load session, reconstruct step executor).
      This branching must be explicit and well-documented in code.

- [x] 31.5 Verify multi-turn HITL works after restart: after the post-restart resume, if the
      agent asks another question, the live oneshot channel path from Task 30 handles it correctly
      (since the step executor is now alive in the current process).

- [x] 31.6 Write unit and integration tests covering the restart recovery guard, the post-restart
      resume path, the session reconstruction, and the two-branch dispatch. See Tests section below.

- [x] 31.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

When the process restarts while the step executor task is suspended on a oneshot channel, the
task is gone. The restart recovery path (Task 19) detects the `waiting_hitl` dispatch and the
pending `hitl_request`, and leaves them intact for a human to answer. The answer API (Task 33)
then drives the transition logic.

The resume path in the answer API handler must handle two cases:

1. **Live task exists** (Task 30 path): send the answer via the existing oneshot channel to wake
   the suspended step executor.
2. **No live task** (this task's path): reconstruct the step executor with the session loaded
   from `arky_session::SessionStore` using the `session_id` from the dispatch record.

The key difference is that the post-restart path must reconstruct the execution context:

1. Load the provider session from the durable session store.
2. Load the last checkpoint for this run/step.
3. Construct a continuation turn that includes the answer as a user input message.
4. Re-execute the step from the checkpoint point, with the full conversation context available.

The step must be idempotent up to the HITL request point. This means that any side effects
produced before the HITL question must either be safely re-executable or guarded by the
checkpoint state.

### Relevant Files

- `crates/openfang-runtime/src/agent_loop.rs` -- agent execution loop; post-restart continuation
- `crates/openfang-kernel/src/workflow.rs` -- step executor; reconstruction logic lives here
- `crates/arky-session/src/store.rs` -- `SessionStore::load` used in post-restart resume
- `crates/arky-session/src/sqlite.rs` -- durable session backend for Arky providers
- `crates/openfang-memory/src/session.rs` -- OpenFang LLM driver session for non-Arky paths
- Task 23 dispatch repository -- `DispatchRepository::update_status` (to/from `waiting_hitl`)
- Task 24 HITL repository -- `HitlRepository::create`, `HitlRepository::answer`
- Task 19 -- restart recovery logic that must be extended
- Task 30 -- live oneshot channel path that this task complements

### Dependent Files

- Task 33: the HITL answer API endpoint dispatches to either live path (Task 30) or
  reconstruction path (this task)

## Deliverables

- Restart recovery guard: `waiting_hitl` + non-null `active_hitl_request_id` recognized as a
  stable state requiring no automatic recovery action
- Post-restart resume path: session reconstruction from durable store, continuation turn with
  answer, step re-execution from checkpoint
- Two-branch dispatch in answer handler: live oneshot channel vs post-restart reconstruction
- Provider session preservation verified across restart boundary
- Multi-turn HITL after restart: subsequent questions use live path

## Tests

### Unit Tests (Required)

- [x] `recovery_scan_should_skip_waiting_hitl_dispatches` -- simulate a restart by creating a
      run with non-null `active_hitl_request_id` and a dispatch in `waiting_hitl` status; run the
      recovery scan and verify these records are left intact (not transitioned or re-executed).
- [x] `post_restart_resume_should_load_session_from_store` -- after simulating a restart (no
      live oneshot channel), submit an answer and verify the session is loaded via
      `SessionStore::load` using the `session_id` from the dispatch record, not from in-memory state.
- [x] `post_restart_resume_should_reconstruct_step_executor` -- after the post-restart resume,
      verify a new step executor is created and executes the continuation turn with the answer.
- [x] `two_branch_dispatch_should_use_live_path_when_sender_exists` -- with a live oneshot
      `Sender` in the registry, verify the answer handler uses the live channel path.
- [x] `two_branch_dispatch_should_use_reconstruction_when_no_sender` -- without a live oneshot
      `Sender` in the registry, verify the answer handler uses the reconstruction path.

### Integration Tests (Required)

- [x] `hitl_restart_during_pending_request_should_preserve_state` -- pause on HITL, simulate a
      process restart (drop and recreate the runtime context), run the restart recovery scan, and
      verify: the run's `active_hitl_request_id` is still set, the dispatch is still `waiting_hitl`,
      and the HITL request is still `pending`.
- [x] `hitl_post_restart_resume_should_reconstruct_session_from_store` -- after the restart
      simulation above, submit an answer and verify the session is loaded via `SessionStore::load`
      using the `session_id` from the dispatch record, not from in-memory state.
- [x] `hitl_post_restart_resume_should_complete_step` -- after post-restart resume, verify the
      step completes successfully and the workflow advances to the next step.
- [x] `hitl_multi_turn_after_restart_should_use_live_path` -- after post-restart resume, trigger
      a second HITL question; verify the second question uses the live oneshot channel path (since
      the step executor is now alive) and the second answer completes the step.

### Regression and Anti-Pattern Guards

- [x] Do not lose provider/session context across the HITL pause/resume boundary -- the session
      must be loadable by `SessionStore::load` after a restart; an in-memory-only session is not safe.
- [x] Do not resume with a freshly created session instead of the stored session -- this would
      lose the agent's reasoning context and produce a broken continuation.
- [x] Do not consolidate the live and post-restart resume paths into one code path that loses the
      explicit post-restart case -- they must be separately testable.
- [x] Do not have the recovery scan attempt to re-execute a step that is in `waiting_hitl` state --
      this would corrupt the pending HITL interaction.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- A process restart during a pending HITL interaction leaves the state intact and awaiting a
  human answer -- no automatic re-execution occurs.
- After restart, when a human answers the pending HITL request, the session is reconstructed
  from the durable store and the step resumes with the answer injected.
- The two-branch resume path (live oneshot vs reconstruction) is cleanly separated and both
  branches are independently testable.
- Multi-turn HITL after restart works: the first resume uses reconstruction, subsequent questions
  use the live oneshot path.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Prior Implementation Reference

The old TypeScript codebase implements the clarification/HITL flow end-to-end:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/clarification/` -- Clarification tool showing pause/resume interaction patterns
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/hitl/` -- Frontend HITL interaction system

## Notes

- This task depends on Task 30 which implements the live oneshot channel path. The post-restart
  reconstruction path cannot be tested without the durable state written by Task 30's pause side.
- The step must be idempotent up to the HITL request point for post-restart reconstruction to
  work correctly. This is an architectural constraint that must be documented and enforced.
