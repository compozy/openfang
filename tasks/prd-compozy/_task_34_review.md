# Task 34 Review: Looper Durable Schema And Runtime

## Status: PASS

## Checklist

- [x] 34.1 `looper_run` and `looper_subtask` tables created in migration `20260325_011_looper_runtime.sql`
- [x] 34.2 `LooperRuntime` struct with `run` async loop and `LooperDispatchExecutor` trait implemented
- [x] 34.3 Sequential mode dispatches one subtask at a time; parallel mode respects `max_parallelism`
- [x] 34.4 `depends_on` enforcement delays subtask selection until dependency completes
- [x] 34.5 `non_parallelizable` subtask waits for all in-flight dispatches before executing
- [x] 34.6 `Semaphore` used for bounded concurrency in parallel mode
- [x] 34.7 `ensure_execution_view` resets interrupted dispatches on restart recovery
- [x] 34.8 Pause/resume and cancel operations implemented
- [x] 34.9 Looper control-plane routes registered and implemented (`GET`, `POST`, pause, resume, cancel, SSE)
- [x] 34.10 `looper_control_plane_route_tests` module with 7 tests: create, validation, parallelism reject, SSE serialization, pause conflict, get-not-found, list-with-status-filter
- [x] 34.11 `looper.rs` kernel tests: 9 tests covering sequential, parallel, non-parallelizable, depends_on, 5-subtask, restart recovery, pause/resume, cancel, 10-subtask bounded concurrency

## Findings

**Schema**: `looper_run` table has all required columns including `execution_policy_json`, `current_subtask_id`, `progress_json`, `error_json`, `completed_at`, and a CHECK constraint on `status`. `looper_subtask` has a `UNIQUE(looper_run_id, subtask_id)` constraint and FKs to both `looper_run` and `subtask`. All required indexes are present.

**Runtime**: `LooperRuntime::run` loop correctly uses `Semaphore::acquire_owned` before spawning a subtask dispatch, stores the permit in the task's closure (`_permit`) for RAII release, preventing over-parallelism. The `select_next_subtask` function correctly checks `depends_on_json` parsed from the subtask record and skips subtasks whose dependencies are not yet in a terminal state.

**Recovery**: `ensure_execution_view` resets any dispatch in `running` state back to `pending` at startup, ensuring that subtasks interrupted by daemon crash are correctly re-queued rather than left orphaned.

**Route tests** (`looper_control_plane_route_tests`, lines 94-505): All 7 tests are well-structured, use real kernel instances, and assert correct status codes and JSON fields. The SSE serialization test verifies the event wire format (`id`, `event`, `data` lines).

**One minor note**: The `pause_looper_run_v1_should_return_conflict_for_completed_run` test seeds a completed run and verifies 409 Conflict. There is no test for pausing a `running` looper (happy path). This is a gap in route-level test coverage but does not affect correctness of the implementation, which is exercised by the kernel-level tests.

No blocking gaps found. All deliverables are present and implementation logic is correct.

## Files Reviewed

- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/_task_34.md`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260325_011_looper_runtime.sql` (full)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/looper.rs` (full)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (lines 94-505, `looper_control_plane_route_tests`)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs` (lines 638-702)
