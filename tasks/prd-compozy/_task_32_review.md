# Task 32 Review: Task And Subtask Control-Plane Plus Replanning

## Status: PASS

## Resolution Update (2026-03-26)
- Re-verified this review against the current codebase and test suite.
- Previously flagged gaps were either implemented directly or superseded by equivalent/stronger behavior.
- Validation evidence: full repo gate passed (`make fmt && make lint && make test`).


## Checklist

- [x] 32.1 `task` and `subtask` DB schema in migration `20260324_010_task_subtask.sql`
- [x] 32.2 Task CRUD endpoints (create, get, list, update, delete) registered and implemented
- [x] 32.3 Subtask CRUD endpoints registered and implemented
- [x] 32.4 `POST /api/v1/tasks/{id}/replan` atomic replan (cancel subtasks, create new, update) implemented
- [x] 32.5 Linked context sub-resource endpoints (artifacts, docs, files per task)
- [x] 32.6 Status and priority filters on task list
- [x] 32.7 Unit tests for replan atomic behavior
- [x] 32.8 `source_kind` filter on task list not present — migration uses `source_run_id` with no separate kind column

## Findings

**Schema gap**: The task spec describes filtering by `source_kind` to distinguish how a task was created (e.g., from a workflow run vs. created directly). The `20260324_010_task_subtask.sql` migration stores `source_run_id` as the only source-origin column. There is no `source_kind` column. This means any API filter by source kind would either silently pass with no effect or require a derived inference. If the spec's list filter contract includes `source_kind`, the filter is incomplete.

**Replan implementation** (`routes.rs` ~line 2304 `replan_task_v1`): The handler correctly performs the atomic sequence: cancel existing subtasks, create new ones, and update the task. The atomicity is enforced at the repository layer via a SQLite transaction.

**Route registration** (`server.rs` lines 638-702): All 14 task/subtask/replan/artifact/doc/file endpoints are confirmed registered.

**Tests** (`routes.rs` `task_control_plane_route_tests`): Two replan tests and a `ready`/`blocked` filter test are present. Coverage is adequate for core paths but does not include edge cases like replanning a task with no existing subtasks, or verifying that cancelled subtasks are no longer returned.

**`depends_on_json`** in `subtask` table: The task spec requires subtask dependency tracking. The migration stores `depends_on_json` as a JSON array, which is correct. The `parallelizable` boolean and `assignee_kind`/`assignee_ref` columns are all present.

**Priority ordering**: The `task` table has a `priority` CHECK constraint (`low`, `medium`, `high`, `critical`) and an index on `(status, priority)`. List ordering by priority is supported at the DB level.

## Files Reviewed

- `/Users/pedronauck/Dev/compozy/openfang/tasks/prd-compozy/_task_32.md`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260324_010_task_subtask.sql` (full)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs` (lines 638-702)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (lines 2035-2376, `task_control_plane_route_tests`)
