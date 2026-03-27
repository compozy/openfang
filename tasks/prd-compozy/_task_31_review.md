# Task 31 Review: HITL Post-Restart Reconstruction

## Status: PASS

## Checklist

- [x] 31.1 `HitlAnswerDisposition` enum with `Live { record }` and `Reconstruct(Box<HitlResumeContext>)` variants
- [x] 31.2 `answer_hitl_request_with_disposition` checks registry for live sender first; falls back to reconstruction path
- [x] 31.3 Recovery scan (`recover_durable_runs`) detects `WaitingHitl` dispatches and skips re-execution with informational log
- [x] 31.4 `build_hitl_resume_context` reconstructs session state from DB for post-restart path
- [x] 31.5 Multi-turn conversation continues correctly after restart (step executor is reconstructed)
- [x] 31.6 Unit tests cover two-branch dispatch, post-restart resume, step completion after restart, and recovery scan behavior

## Findings

**Two-branch dispatch** (`workflow.rs` ~line 2798): `answer_hitl_request_with_disposition` correctly checks the `HitlRegistry` for a live `oneshot::Sender`. If found, it sends `HitlAnswer` directly (`Live` branch). If not found (post-restart), it calls `build_hitl_resume_context` and returns `Reconstruct(Box<HitlResumeContext>)`. The branching is clean and correct.

**Recovery scan** (`workflow.rs` ~lines 2380-2438): `recover_durable_runs` correctly identifies `WaitingHitl` dispatch status, logs "Run has pending HITL request awaiting human answer" at INFO level, and explicitly does not re-enqueue it. This prevents phantom double-execution of HITL-paused steps on restart.

**Test coverage** is strong: five dedicated tests found in `workflow.rs`:
- `recovery_scan_should_skip_waiting_hitl_dispatches` (line 8296)
- `recovery_scan_should_project_inconsistent_waiting_hitl_runs_into_cache` (line 8340)
- `two_branch_dispatch_should_use_reconstruction_when_no_sender` (line 5457)
- `post_restart_resume_should_reconstruct_step_executor` (line 5575)
- `hitl_post_restart_resume_should_complete_step` (line 5738)

**`HitlRegistry` struct** (`workflow.rs` ~line 826): `Mutex<HashMap<String, oneshot::Sender<HitlAnswer>>>` with `register` / `remove` methods. The Mutex is not held across `.await` points (correct async usage).

No significant gaps found. All deliverables from the task spec are present and verified.

## Files Reviewed

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow.rs` (lines 820-840, 2380-2438, 2798-2860, 5457-5800, 8296-8400)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/hitl.rs` (full)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/dispatch.rs` (status transitions, `WaitingHitl`)
- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/_task_31.md`
