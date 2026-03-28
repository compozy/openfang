# PRD Tasks 31–43: Backend Features and UI Implications

Analysis of what was implemented in each task, whether UI changes are needed, and what kind of UI work would be required.

---

## Summary Table

| Task# | Title | Backend Feature | UI Needed? | UI Description | Status |
|-------|-------|-----------------|------------|----------------|--------|
| 31 | HITL Post-Restart Reconstruction | Two-branch HITL resume path (live channel vs post-restart reconstruction); restart recovery guard that leaves `waiting_hitl` dispatches intact; session reconstruction from durable store | Yes | HITL interaction panel needs to persist and recover state across page reloads; pending HITL requests must remain answerable in the UI after a backend restart; no new page needed, but the existing HITL answer flow must be resilient to the "no live channel" case | Completed / PASS |
| 32 | Task And Subtask Control-Plane Plus Replanning | Full task/subtask CRUD at `/api/v1/tasks` and `/api/v1/subtasks`; atomic `POST /api/v1/tasks/{id}/replan` with `cancel_subtasks`, `create_subtasks`, `update_subtasks` operations; linked context sub-resources (`/artifacts`, `/docs`, `/files`) per task | Yes | New Tasks page (or section) for listing, creating, editing, and deleting tasks; subtask list within each task detail view; replan action button/modal on task detail; linked artifacts/docs/files tabs within task detail; priority and status filters on task list | Completed / PASS |
| 33 | Dispatch And HITL Control-Plane Surfaces | All dispatch endpoints (`GET`, cancel, retry, children) at `/api/v1/dispatches`; all HITL endpoints (`GET`, answer, cancel) at `/api/v1/hitl-requests`; run-scoped dispatch and HITL sub-resources; SSE stub handlers for live event streams | Yes | Dispatches tab or section within a run detail view; HITL requests panel/inbox for operators to see and answer pending questions; cancel and retry action buttons on dispatch rows; run-scoped dispatch tree view (parent/children); SSE-driven live status updates once full streaming is wired | Completed / PASS (tests absent — 33.9 gap noted in review) |
| 34 | Looper Durable Schema And Runtime | `looper_run` and `looper_subtask` tables in `compozy.db`; `LooperRunRepository` and `LooperSubtaskRepository`; `LooperRuntime` executor with sequential and parallel modes; pause/resume/cancel transitions; restart recovery | Yes | Looper runs list page (reachable from a task detail or top-level nav); looper run detail with progress bar (`total`, `completed`, `failed`); subtask execution view within looper run detail; pause, resume, cancel action buttons; execution policy display (mode, max_parallelism) | Completed / PASS |
| 35 | Trigger v2 Types And Definition CRUD | New trigger v2 type system (`TriggerV2`, `TriggerMatch`, `TriggerTarget`); full CRUD at `/api/v1/triggers`; validate, compile, fork, enable/disable, runtime inspection, and test endpoints | Yes | Triggers management page replacing or extending the existing triggers UI; trigger creation/edit form with match fields (event, source, contains, filters) and target kind selector (agent_message, workflow_start, workflow_signal); enable/disable toggle per trigger row; trigger test panel (send synthetic event, see match result); fork action for pack-managed triggers; runtime status column (fire_count, last_fired_at) | Completed / PASS |
| 36 | Event Ingress Pipeline And Match Engine | `TriggerMatchEngine` with full match evaluation; `POST /api/v1/events` with match/dispatch/response; `POST /api/v1/events/dry-run`; `max_fires`/`cooldown_secs` enforcement; `fire_count`/`last_fired_at` state updates | Yes | Event ingress test panel (send a synthetic event and see matched triggers and effects); dry-run mode toggle to preview without dispatching; event history or activity log showing recent ingested events and their effects; runtime status columns on trigger list reflecting fire counts | Completed / PASS |
| 37 | Artifact And Doc Versioning | `artifact`, `artifact_version`, `doc`, `doc_version` tables in `compozy.db`; `ArtifactRepository` and `DocRepository` with append-only versioning; content-addressable hash lookup; provenance fields | Yes | Artifact and document version history panels within task detail and run detail views; version timeline showing version_no, content_hash, created_by, created_at; content viewer for the current version body; provenance display linking a version back to the dispatch or run that produced it | Completed / PASS |
| 38 | Artifact And Doc Standalone Read Endpoints | Read-only endpoints: `GET /api/v1/artifacts`, `GET /api/v1/artifacts/{id}`, `GET /api/v1/artifacts/{id}/versions`, and doc equivalents; pagination and `artifact_type`/`task_id`/`q` filters | Yes | Standalone Artifacts page and Documents page in the dashboard nav; list views with type and task filters; artifact detail view with current version inline and version history tab; document detail view with same pattern; these are browseable independently of a specific task or run | Completed / PASS |
| 39 | Looper Control-Plane And SSE Surfaces | All looper API endpoints: create, list, detail, subtasks, pause, resume, cancel; SSE `GET /api/v1/looper-runs/{id}/events` with bounded ring buffer replay, `stream.reset`, `keepalive`; looper runtime registry in `AppState` | Yes | Looper runs list page with status and execution_mode filters; looper run detail showing live progress via SSE (subtask.started, subtask.completed, subtask.failed events); real-time progress bar driven by `run.updated` events; pause/resume/cancel toolbar on looper run detail; looper subtask table within detail view | Completed / PASS |
| 40 | Pack List Detail And CRUD Endpoints | `PackManifest` type; `PackRegistry` scanning `~/.compozy/packs/`; `GET /api/v1/packs`, `GET /api/v1/packs/{id}`, `GET /api/v1/packs/{id}/objects`, `POST /api/v1/packs/{id}/fork` | Yes | Packs page in the dashboard; pack list with installed/managed badges and object counts by resource type; pack detail showing managed objects with forked indicators; fork button per object; object type breakdown display | Completed / PASS (task file status was `pending` but code is complete) |
| 41 | Pack System Install Upgrade And Bootstrap | `PackInstaller` with install, upgrade, upgrade dry-run, uninstall; `POST /api/v1/packs/install`, `POST /api/v1/packs/{id}/upgrade`, `POST /api/v1/packs/{id}/upgrade/dry-run`, `POST /api/v1/packs/{id}/uninstall`; SDLC built-in pack bootstrapped at startup | Yes | Install pack form/modal (source kind selector, pack ID, version); upgrade button on pack detail with dry-run preview modal showing managed_objects_added/updated/removed/forks_untouched; uninstall button with confirmation and fork-warning dialog; bootstrap status indicator on first launch | Completed / PASS (task file status was `pending` but code is complete) |
| 42 | Retention Policies And Remaining SSE Endpoints | Indexes on `workflow_checkpoint`, `artifact_version`, `doc_version`; background retention job for `workflow_checkpoint` pruning; `GET /api/v1/runs/{id}/events` SSE; `GET /api/v1/dispatches/{id}/events` SSE; `GET /api/v1/hitl-requests/stream` global SSE | Partial | No new pages needed; existing run detail, dispatch detail, and HITL inbox views should be upgraded to consume the new SSE endpoints for live updates instead of polling; HITL operator inbox can subscribe to the global `/hitl-requests/stream` to receive new questions in real time; retention settings could be surfaced in an admin/settings page but are not required for core UI | Completed / PASS |
| 43 | E2E Integration Test And Restart Recovery Regression | Comprehensive E2E test covering the full event → trigger → workflow → dispatch → HITL → completion → artifact flow; restart mid-flight recovery regression; restart-during-HITL regression; endpoint reachability verification; data integrity assertions | No | This is a testing-only task. No new backend APIs were added beyond what tasks 30–42 provide. No UI changes are implied. | Completed / PASS |

---

## Detailed Notes Per Task

### Task 31 — HITL Post-Restart Reconstruction

**Backend:** Implements the post-restart resume path in the HITL answer handler. When an answer arrives and no live `oneshot::Sender` exists in the `HitlRegistry` (because the daemon restarted), the handler reconstructs the step executor from the durable session store. The restart recovery scan now recognizes `waiting_hitl` dispatch status as a stable state and skips re-execution.

**UI implications:** The HITL answer panel in the dashboard must work correctly after a backend restart. The backend already exposes this transparently through `POST /api/v1/hitl-requests/{id}/answer` (Task 33), but the UI must not assume the answer call will always wake a live task synchronously. The UI should poll or subscribe to SSE to detect when a post-restart step executor finishes after the answer is submitted. No new page is needed, but the HITL interaction component needs resilience handling.

---

### Task 32 — Task And Subtask Control-Plane Plus Replanning

**Backend:** Full CRUD for tasks and subtasks stored in `compozy.db`. The `replan` endpoint applies `cancel_subtasks`, `create_subtasks`, and `update_subtasks` atomically. Linked context sub-resources expose artifacts, docs, and files per task. Subtask list filters include `ready`, `blocked`, `assignee_ref`, `kind`.

**UI implications:** This is the primary task management surface. The dashboard needs:
- A **Tasks page** with paginated list, status/priority filters, and create task button.
- A **Task detail page** with title, description, status, priority, source, owner, and tabs for subtasks, artifacts, docs, and files.
- A **Subtask list** within task detail, showing `depends_on`, `parallelizable`, `assignee`, `kind`, `status`, with `ready`/`blocked` indicator badges.
- A **Replan modal** with an operation builder (cancel IDs, create new subtask forms, update existing subtasks), reason field, and effects summary after submission.

---

### Task 33 — Dispatch And HITL Control-Plane Surfaces

**Backend:** All dispatch and HITL API surfaces from `API-SPEC.md` sections 10 and 11. The HITL answer endpoint triggers the two-branch resume from Task 31. SSE stubs registered at `/api/v1/dispatches/{id}/events` and `/api/v1/hitl-requests/stream`.

**UI implications:**
- A **Dispatches section** within workflow run detail, showing all dispatches with status, kind, attempt, target agent, and cancel/retry actions.
- A **HITL Inbox** page or panel — the primary surface for operators to see pending questions, read context, and submit answers. Default sort is `created_at ASC` so oldest questions appear first.
- **Dispatch children view** to show multi-agent delegation trees.
- SSE upgrade (Task 42 delivers the real events): once SSE endpoints are live, replace polling with live streams in the dispatch and HITL views.

**Note from review:** Tests for the HTTP API layer (Task 33.9) were not implemented. The routes are functional but coverage is a gap if regression tests are needed before UI integration.

---

### Task 34 — Looper Durable Schema And Runtime

**Backend:** `looper_run` and `looper_subtask` tables, `LooperRuntime` with sequential and parallel execution modes, `depends_on` enforcement, pause/resume/cancel, and restart recovery.

**UI implications:**
- A **Looper Runs page** (navigable from a task detail) listing runs for a task with status, execution policy mode, and progress (total/completed/failed).
- A **Looper Run detail page** showing the execution policy, progress bar, current subtask, and the subtask execution table (`looper_subtask` view showing each subtask's dispatch status and result).
- Pause, Resume, Cancel action buttons on the detail page.

---

### Task 35 — Trigger v2 Types And Definition CRUD

**Backend:** New trigger v2 type system replacing the legacy `TriggerPattern` enum. All thirteen trigger endpoints: CRUD, validate, compile, compiled, fork, runtime, enable, disable, test.

**UI implications:**
- The existing **Triggers page** needs to be upgraded or replaced to use the v2 API (`/api/v1/triggers` instead of `/api/triggers`).
- Trigger create/edit form needs fields for `match.event`, `match.source`, `match.contains`, `match.filters`, target kind selector, `max_fires`, `cooldown_secs`.
- Enable/disable toggle per row in the triggers list.
- A **Trigger test panel** — input a synthetic event payload and see `matched`, `resolved_target`, `would_dispatch`, `explanation` without actually firing the trigger.
- **Runtime status column** in the trigger list: `fire_count`, `last_fired_at`, `enabled`.
- Fork button for pack-managed triggers (links to pack detail).

---

### Task 36 — Event Ingress Pipeline And Match Engine

**Backend:** `TriggerMatchEngine` with full match evaluation for all conditions; `POST /api/v1/events` dispatching matched actions; `POST /api/v1/events/dry-run`; `fire_count`/`last_fired_at` state persistence.

**UI implications:**
- An **Event Ingress panel** or developer tool — compose a synthetic event (event type, source, payload) and submit it, seeing the matched triggers and effects.
- Dry-run toggle: test an event without actually dispatching.
- Alternatively, this could be integrated into the Triggers test panel or a dedicated **Events page** showing a log of recent ingested events with their matched trigger IDs and effect counts.

---

### Task 37 — Artifact And Doc Versioning

**Backend:** Append-only artifact and document versioning schema with content-addressable hash lookup. Provenance fields link each version to the dispatch or run that produced it.

**UI implications:**
- A **Version history panel** within artifact and document detail views, showing all versions with `version_no`, `content_hash`, `created_by` (kind + ref), and `created_at`.
- Content viewer for the current version body (content_json rendered appropriately for the artifact type).
- Provenance link — clicking `created_by.ref` (e.g., a dispatch ID) navigates to the dispatch detail page.

---

### Task 38 — Artifact And Doc Standalone Read Endpoints

**Backend:** Six read-only endpoints for artifacts and documents independent of task context. Filters by `artifact_type`, `task_id`, `q`. Version history paginated descending by `version_number`.

**UI implications:**
- A **standalone Artifacts page** in the main nav with filters for type and task, listing all artifacts.
- A **standalone Documents page** with the same pattern.
- Artifact detail page with current version inline and version history tab.
- Document detail page mirroring artifact detail.
- These are browseable without navigating through a specific task, making them useful for cross-task artifact discovery.

---

### Task 39 — Looper Control-Plane And SSE Surfaces

**Backend:** All seven looper control-plane endpoints including `POST /api/v1/looper-runs` (create); SSE endpoint `GET /api/v1/looper-runs/{id}/events` with bounded ring buffer, `stream.reset`, `keepalive`; looper runtime registry in `AppState`.

**UI implications:**
- The Looper Runs pages from Task 34 should consume the SSE endpoint to show live progress updates — progress bar animates as `run.updated` events arrive; subtask rows update on `subtask.started`, `subtask.completed`, `subtask.failed`.
- A **Create Looper Run** form/modal: task selector, execution policy (mode toggle between sequential/parallel, max_parallelism input, selection strategy).
- `Last-Event-ID` support enables reconnection without losing events, which should be handled in the SSE client layer.

---

### Task 40 — Pack List Detail And CRUD Endpoints

**Backend:** `PackManifest` type, `PackRegistry` boot-time scan of `~/.compozy/packs/`, four endpoints: list, detail, objects list, fork.

**UI implications:**
- A **Packs page** in the dashboard navigation.
- Pack list with badges: `installed`, `managed`, source kind (`bundled` vs `external`), and object count summary (agents, workflows, triggers, schedules, templates).
- Pack detail page with full manifest and managed objects list.
- Objects list within pack detail showing each object's `resource_type`, `resource_id`, and `forked` status.
- **Fork button** per object that copies the object to user-owned space with provenance metadata.

---

### Task 41 — Pack System Install Upgrade And Bootstrap

**Backend:** `PackInstaller` with install, upgrade, upgrade dry-run, uninstall; four operational endpoints; SDLC built-in pack bootstrapped at startup using the same code path as the API.

**UI implications:**
- **Install Pack** button/modal on the Packs page — source kind selector (bundled/external), pack ID field, version field.
- **Upgrade button** on pack detail with a **dry-run preview** before committing — shows `effects.managed_objects_added`, `managed_objects_updated`, `managed_objects_removed`, `forks_untouched`.
- **Uninstall button** with confirmation dialog that warns if user forks exist (the API returns the list of forked IDs, which can be shown in the dialog).
- On first launch, the SDLC pack is auto-bootstrapped; the UI may show a "Getting started" indicator or SDLC pack already present in the pack list.

---

### Task 42 — Retention Policies And Remaining SSE Endpoints

**Backend:** Indexes and retention job for `workflow_checkpoint`; three remaining SSE endpoints: `GET /api/v1/runs/{id}/events`, `GET /api/v1/dispatches/{id}/events`, `GET /api/v1/hitl-requests/stream` (global, with `run_id`/`status` filters).

**UI implications:**
- Existing run detail, dispatch detail, and HITL views should be **upgraded from polling to SSE** to consume these endpoints for live state updates.
- The **HITL operator inbox** can subscribe to `/api/v1/hitl-requests/stream?status=pending` to receive new questions in real time without polling, using the `run_id` filter when scoped to a specific run.
- Retention settings (checkpoint age threshold, max rows) could be exposed in an admin/settings page, but this is optional for core UI completeness.
- No entirely new pages are required; this task is about wiring live events into views already needed by Tasks 33, 34, and 39.

---

### Task 43 — E2E Integration Test And Restart Recovery Regression

**Backend:** Testing-only task. Comprehensive E2E and restart recovery integration tests. No new API endpoints or schemas introduced.

**UI implications:** None. This task validates the backend slice end-to-end but adds no new public surfaces requiring UI work.

---

## Prioritized UI Work Summary

Based on the backend features above, the following UI areas need to be built or updated (highest priority first):

1. **HITL Inbox / Operator Panel** (Tasks 31, 33, 42) — Core interactive surface; operators need to see and answer pending HITL questions, subscribe to live updates via SSE.
2. **Tasks and Subtasks Pages** (Task 32) — Primary task management surface; replanning is a key workflow action.
3. **Looper Runs Pages with Live SSE** (Tasks 34, 39) — Iterative task execution view with real-time progress driven by SSE events.
4. **Triggers Page (v2)** (Tasks 35, 36) — Replace or upgrade existing triggers UI to support v2 match/target model, enable/disable, fire count, and the event test panel.
5. **Packs Page** (Tasks 40, 41) — Pack management: list, detail, fork, install, upgrade with dry-run preview, uninstall.
6. **Artifacts and Documents Pages** (Tasks 37, 38) — Standalone browseable artifact/doc pages with version history and provenance links.
7. **Dispatches Section** (Task 33) — Within run detail: dispatch tree, cancel/retry actions.
8. **SSE Integration Upgrades** (Task 42) — Wire existing run and dispatch detail views to consume live SSE streams instead of polling.
