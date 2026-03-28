# PRD Tasks 16–30: UI Integration Analysis

> Generated: 2026-03-27
> Purpose: Identify which backend tasks (16–30) have dashboard/UI implications and describe what UI work is needed.

---

## Summary Table

| Task# | Title | Backend Feature | UI Needed? | UI Description | Status |
|-------|-------|-----------------|------------|----------------|--------|
| 16 | Durable Workflow Run Repository And Transition Writer | Durable `workflow_run`, `workflow_checkpoint`, `workflow_signal` tables in `compozy.db`; `TransitionWriter`; run status state machine; `GET /api/v1/runs/{id}`, `GET /api/v1/runs`, `GET /api/v1/workflows/{id}/runs`, `GET /api/v1/runs/{id}/checkpoints` | YES | Workflow runs list page: display run status (`pending`, `running`, `waiting_signal`, `waiting_hitl`, `paused`, `completed`, `failed`, `cancelled`); run detail page showing checkpoints trail; `waiting_signal` / `paused` / `interrupted` visual states need distinct badges | PASS |
| 17 | Workflow Signal Persistence And Waiting-State Integration | `workflow_signal` table; `POST /api/v1/runs/{id}/signals`; `GET /api/v1/runs/{id}/signals`; signal idempotency; `waiting_signal` run status; eager-consume path; `wait_signal` compile-time validation | YES | Signal submission UI within a run detail page (form to send a signal by name + payload + source); signal list panel showing consumed/unconsumed signals with timestamps; run status badge for `waiting_signal` state | PASS |
| 18 | Agent Definition Validation And Compile Pipeline | Four-stage validation pipeline (`stage1`–`stage4`) and `compile` function in `openfang-agent-definition` crate; `AgentDefinition`, `CompiledAgentDefinition`, `ProviderBinding`, `AgentProductMetadata` types; `ValidationIssue` with severity/code/path/message | YES | Agent definition editor needs a "Validate" button that shows structured validation issues (per-field path + severity color); "Compile" button that shows the three-layer compiled output; issue list rendered inline next to fields | PASS |
| 19 | Restart Recovery And Durable Run Control Surfaces | Startup recovery scan (downgrades `running` → `paused`); `POST /api/v1/runs/{id}/pause`, `/resume`, `/cancel`; `GET /api/v1/runs?status=paused`; `?waiting_kind` filter; `run_recovered_needs_resume` checkpoint kind | YES | Run control-plane actions in the run detail page: Pause / Resume / Cancel buttons with correct enabled states per status; `paused` status badge (with recovery tooltip for `run_recovered_needs_resume`); run list filterable by `status` and `waiting_kind` | PASS |
| 20 | Agent Definition CRUD And Compile Routes | `GET/POST /api/v1/agents`, `GET/PUT/DELETE /api/v1/agents/{id}`, `POST /api/v1/agents/validate`, `POST /api/v1/agents/compile`, `GET /api/v1/agents/{id}/compiled`; `AgentDefinition`-first API; `origin` and `forked_from` provenance | YES | Agents page: list agents with new fields (`enabled`, `group`, `tags`, `origin`, `runtime_status`); agent detail/edit page using `AgentDefinition` JSON form (not raw TOML); validate/compile actions inline; compiled output viewer (three-layer); origin/provenance display | PASS |
| 21 | Agent Runtime Operational Sub-Resources | `GET /api/v1/agents/{id}/runtime`; `POST .../runtime/start`, `/stop`, `/restart`; `PUT .../runtime/mode`; `GET/POST /api/v1/agents/{id}/sessions`; `GET/POST .../sessions/{id}/activate`, `/reset`, `/compact` | YES | Agent detail page: runtime status panel (state, mode, health, active sessions count, active dispatches count); Start/Stop/Restart buttons; mode selector dropdown; sessions tab with list, activate/reset/compact per session | PASS |
| 22 | Agent Sessions Messages And SSE Streaming | `POST /api/v1/agents/{id}/messages`; `POST .../messages/stream` (SSE with `message.delta`, `message.completed`, `tool.started`, `tool.completed`, `error`, `keepalive`); `POST .../messages/dry-run` | YES | Chat/conversation panel within agent session: message input, streaming response (SSE delta rendering), tool call indicators; dry-run preview mode that shows `would_execute`, `resolved`, `effects`, `explanation` without sending | PASS |
| 23 | agent_dispatch Schema And Persistence Layer | `agent_dispatch` table in `compozy.db`; `DispatchKind` (call/send/spawn), `DispatchStatus` (pending/running/waiting_hitl/completed/failed/cancelled); `DispatchRepository` with parent-child lineage; attempt counter | YES | Dispatch list/detail view under a run or separately; parent-child lineage tree; status badges per dispatch kind and status; attempt counter display; reserved for future API surface (task 33) | PASS |
| 24 | hitl_request Schema And Persistence Layer | `hitl_request` table in `compozy.db`; `HitlStatus` (pending/answered/cancelled/timed_out); `HitlKind` (clarification/approval/choice/freeform); `sequence_no` ordering; `HitlRepository` with atomic answer operation | YES | HITL requests panel within run/dispatch detail; pending requests prominently highlighted; answer submission form; `sequence_no` ordering within a step; status indicators (pending/answered/cancelled/timed_out) | PASS |
| 25 | Workflow Definition CRUD Control-Plane Surfaces | `GET/POST /api/v1/workflows`, `GET/PUT/DELETE /api/v1/workflows/{id}`, `POST .../validate`, `POST .../compile`, `GET .../compiled`, `POST .../fork`, `GET .../runtime`, `POST .../runs`, `POST .../runs/dry-run`, `GET .../runs`; file-backed atomic writes | YES | Workflows page: list with steps count, runtime status, enabled toggle; workflow detail/editor; validate/compile actions; compiled IR viewer; fork action; runtime status panel per workflow; run-trigger form; dry-run preview | PASS |
| 26 | Schedule Control-Plane Surfaces | `GET/POST /api/v1/schedules`, `GET/PUT/DELETE ./{id}`, `POST .../validate`, `POST ./{id}/fork`, `GET ./{id}/runtime`, `POST ./{id}/enable`, `POST ./{id}/disable`, `POST ./{id}/run-now`, `POST ./{id}/run-now/dry-run`; typed cron model | YES | Schedules page: list with cron expression, action kind, enabled toggle, last/next run; schedule detail/editor with typed cron fields; validate button; enable/disable toggle; run-now button; dry-run preview; runtime status panel | PASS |
| 27 | Skills Listing Endpoint | `GET /api/v1/skills` (paginated, `q` filter); `GET /api/v1/skills/{id}`; read-only, no CRUD; in-memory registry | YES | Skills page (read-only): searchable list of skills with id/name/description/source; skill detail modal or page showing source file path and timestamps; no edit/delete controls | PASS |
| 28 | Task And Subtask Domain Schema And Repositories | `task` and `subtask` tables in `compozy.db`; `TaskRepository`, `SubtaskRepository`; `replan` transactional operation; `ready`/`blocked` dependency filters; `slug` uniqueness; `source_run_id` linkage | YES | Tasks page: task list with status (planned/in_progress/completed/cancelled), priority, complexity, owner; task detail with subtask list; subtask dependency visualization; `ready`/`blocked` filter toggles; replan action; links from task to source workflow run | PASS |
| 29 | Dispatch Runtime Integration With Provider-Native Sessions | Dispatch record creation wired into workflow step executor; `provider_driver`, `session_id`, `provider_resume_token` written durably; three dispatch modes (`call`, `send`, `spawn`) with runtime semantics; JoinSet-tracked background tasks | NO (UI surface provided by task 33) | Backend-only wiring task. UI will be delivered when task 33 implements the `/api/v1/dispatches` API surface. No direct new UI needed from this task alone. | PASS |
| 30 | HITL Single-Turn Live Pause And Resume | `HitlRegistry` with `tokio::oneshot` channels; pause path: `hitl_request` creation + dispatch `waiting_hitl` + `active_hitl_request_id` update + `hitl_requested` checkpoint; resume path: answer injection as continuation turn; multi-turn support within a step | YES | HITL answer UI: when a run has `active_hitl_request_id` set, show a HITL prompt/answer panel in the run detail page; the panel displays the question (`context_json`) and a text/choice input for the answer; on submit calls the answer API; sequence within a step shown with `sequence_no` ordering | PASS |

---

## Detailed Notes Per Task

### Task 16 — Durable Workflow Run Repository And Transition Writer

**Backend:** Workflow runs are now durable from creation onward. The `workflow_run`, `workflow_checkpoint`, and `workflow_signal` tables in `compozy.db` are the source of truth. The run status machine includes: `pending`, `running`, `waiting_signal`, `waiting_hitl`, `paused`, `completed`, `failed`, `cancelled`.

**UI implications:**
- The existing Workflows/Runs page (if any) must read from the new API endpoints (`GET /api/v1/runs`, `GET /api/v1/runs/{id}`) instead of any legacy in-memory surface.
- Status display must support all 8 status values with distinct visual treatment.
- A Checkpoints tab under run detail (`GET /api/v1/runs/{id}/checkpoints`) is now meaningful — shows full audit trail of lifecycle events.
- The `MAX_RETAINED_RUNS` cap no longer applies to data visible via API; the UI should not assume a bounded list.

**Pages/sections affected:**
- Workflow runs list (new tab or section under Workflows page)
- Run detail page (checkpoints trail, status badge)

---

### Task 17 — Workflow Signal Persistence And Waiting-State Integration

**Backend:** Signals are first-class durable objects. `POST /api/v1/runs/{id}/signals` submits a signal; `GET /api/v1/runs/{id}/signals` lists them (with `?consumed=true/false` filter). A run in `waiting_signal` status has `waiting_kind="signal"` and `waiting_ref=<signal_name>` set.

**UI implications:**
- Run detail page must show a "Waiting for Signal" state prominently when `status = waiting_signal` and `waiting_ref` is set.
- A "Send Signal" action form (name, payload, source, idempotency key) must be accessible when the run is in this state.
- Signals panel listing all signals for a run (consumed/unconsumed, source, timestamps).

**Pages/sections affected:**
- Run detail page: waiting state indicator, signal submission form, signals list panel

---

### Task 18 — Agent Definition Validation And Compile Pipeline

**Backend:** Pure library code in `openfang-agent-definition` crate. Validation stages 1–4 produce `ValidationIssue` objects (severity/code/path/message). `compile` produces `CompiledAgentDefinition` with three layers.

**UI implications:**
- Agent editor needs real-time or on-demand validation with per-field error display using the `path` field to locate the issue.
- Compile view shows the three-layer output (`agent_manifest`, `provider_binding`, `product_metadata`) for inspection.
- Issue severity coloring: `error` = red, `warning` = yellow.

**Pages/sections affected:**
- Agent definition editor (validate button + inline issue list)
- Compiled output viewer panel

---

### Task 19 — Restart Recovery And Durable Run Control Surfaces

**Backend:** On restart, `running` runs are downgraded to `paused` with `run_recovered_needs_resume` checkpoint. New control-plane endpoints: pause/resume/cancel.

**UI implications:**
- Run list must be filterable by `?status=paused` to find all recovered runs.
- Run detail page needs Pause / Resume / Cancel action buttons with conditional availability (e.g., Resume only enabled when `status=paused`).
- A recovery indicator or tooltip on runs showing `run_recovered_needs_resume` checkpoint — helps operators understand why a run is paused.

**Pages/sections affected:**
- Run list: status filter dropdown including `paused`
- Run detail: action buttons (Pause/Resume/Cancel), recovery note

---

### Task 20 — Agent Definition CRUD And Compile Routes

**Backend:** Full `/api/v1/agents` CRUD surface with `AgentDefinition` JSON (not legacy TOML blob). New endpoints: validate, compile, `/{id}/compiled`. Responses include `origin` and `forked_from` provenance.

**UI implications:**
- The existing Agents page must be updated to use the new `/api/v1/agents` endpoints (old `/api/agents` is legacy).
- Agent list shows `enabled`, `group`, `tags`, `origin.kind`, `runtime_status` — new columns needed.
- Agent create/edit form must accept structured `AgentDefinition` fields (not raw TOML textarea).
- Validate button with structured issue list.
- Compiled output viewer (`/{id}/compiled`).
- Provenance display (`origin.kind`, `forked_from.pack_id`).

**Pages/sections affected:**
- Agents list page (new columns)
- Agent create/edit form (structured fields)
- Agent detail page (compiled output panel, provenance section)

---

### Task 21 — Agent Runtime Operational Sub-Resources

**Backend:** Runtime lifecycle endpoints (start/stop/restart/mode) and session management endpoints (create/list/activate/reset/compact) under `/api/v1/agents/{id}/`.

**UI implications:**
- Agent detail page gains a Runtime tab/panel: shows `loaded`, `state`, `mode`, `healthy`, `active_sessions`, `active_dispatches`.
- Start/Stop/Restart buttons (with confirmation for destructive actions).
- Mode selector (e.g., `autonomous` vs `supervised` dropdown).
- Sessions tab: list sessions with `active` indicator, message count; per-session actions (activate, reset, compact).

**Pages/sections affected:**
- Agent detail page: Runtime panel, Sessions tab

---

### Task 22 — Agent Sessions Messages And SSE Streaming

**Backend:** Message submission, SSE streaming (delta/completed/tool events), dry-run.

**UI implications:**
- Chat interface within an agent session: text input, streaming response with incremental delta rendering, tool call status indicators (`tool.started` / `tool.completed`).
- SSE connection management (reconnect on drop, keepalive handling).
- Dry-run preview panel: shows `would_execute`, `resolved`, `effects`, `explanation` before sending.
- Error handling for SSE `error` events (not HTTP 4xx — must parse SSE error events).

**Pages/sections affected:**
- Agent session chat panel (primary new UI feature)
- Dry-run preview mode toggle

---

### Task 23 — agent_dispatch Schema And Persistence Layer

**Backend:** `agent_dispatch` table with full status lifecycle and parent-child lineage. API surface not yet wired (that is task 33). Three kinds: `call`, `send`, `spawn`.

**UI implications:**
- No direct UI yet — the public API endpoints exposing dispatches (`/api/v1/dispatches`) are implemented in task 33.
- However, dispatch data is referenced in run detail (via `active_dispatch_id`) and HITL requests (via `dispatch_id`), so the UI needs to be prepared to link to dispatch records once task 33 lands.
- Future: Dispatches tab on run detail, parent-child lineage tree view.

**Pages/sections affected:**
- Placeholder: Run detail → Dispatches tab (deferred to task 33)

---

### Task 24 — hitl_request Schema And Persistence Layer

**Backend:** `hitl_request` table with `HitlKind` (clarification/approval/choice/freeform), `sequence_no` ordering, atomic `answer` operation.

**UI implications:**
- HITL requests panel in the run detail and dispatch detail pages.
- Pending requests highlighted with answer input form.
- `sequence_no` ordering within a step shown as a numbered conversation.
- Status badges: pending (highlighted), answered (muted), cancelled/timed_out (error color).
- HITL API surface exposed in task 33 — UI implementation deferred until then, but schema informs the data model for the UI.

**Pages/sections affected:**
- Run detail page: HITL requests panel (deferred until task 33)
- Dispatch detail page: associated HITL questions

---

### Task 25 — Workflow Definition CRUD Control-Plane Surfaces

**Backend:** Full `/api/v1/workflows` CRUD, validate, compile, fork, runtime status, run trigger, dry-run. File-backed atomic writes.

**UI implications:**
- Workflows page needs full redesign around the new API:
  - List: steps count, enabled toggle, `runtime_status` (active_runs, waiting_runs, last_run_at), `origin.kind`.
  - Create/edit form: structured definition fields (id, name, version, steps, input/output contracts, defaults, outputs).
  - Validate button with per-field issue list.
  - Compile button with IR viewer.
  - Fork action (produces user-owned copy).
  - Runtime panel: active_runs count, last_run_at, healthy indicator.
  - "Run Now" button (triggers `POST .../runs`) with input form.
  - Dry-run preview.
  - Runs sub-list per workflow.

**Pages/sections affected:**
- Workflows list page (significant update)
- Workflow detail/editor page (major new UI)
- Workflow runtime panel
- Workflow runs sub-list

---

### Task 26 — Schedule Control-Plane Surfaces

**Backend:** Full `/api/v1/schedules` surface including typed cron model, enable/disable, run-now, dry-run, fork.

**UI implications:**
- Schedules page: list with cron expression display, action kind chip, enabled toggle, last/next run timestamps.
- Schedule create/edit form: typed cron fields (kind, expression, timezone), action kind selector with dynamic fields per kind, delivery kind selector.
- Validate button (per-field issues with `path: "schedule.expr"` etc.).
- Enable/Disable toggle (live scheduler notification — must reflect immediately).
- Run Now button (bypasses cron timer).
- Dry-run preview.
- Runtime status section: consecutive_errors, one_shot, last_status.
- Fork action.

**Pages/sections affected:**
- Schedules list page (update to typed model)
- Schedule detail/editor page

---

### Task 27 — Skills Listing Endpoint

**Backend:** `GET /api/v1/skills` (paginated, `q` filter) and `GET /api/v1/skills/{id}`. Read-only.

**UI implications:**
- New Skills page (read-only).
- Searchable list with id, name, description, source path.
- Skill detail: full fields including timestamps.
- No create/edit/delete UI (filesystem-managed).
- Could be linked from agent definition editor (to show available skills for `capabilities.skills` field).

**Pages/sections affected:**
- New Skills page (list + detail)
- Agent editor skill picker (cross-reference)

---

### Task 28 — Task And Subtask Domain Schema And Repositories

**Backend:** `task` and `subtask` tables in `compozy.db`. `TaskRepository`, `SubtaskRepository` with `ready`/`blocked` dependency filters. `replan` transactional operation. API surface exposed in task 32.

**UI implications:**
- Tasks is a major new product-domain feature requiring a full Tasks page:
  - Task list with status, priority, complexity, owner, source_run_id linkage.
  - Task detail with subtask list (ordered by position, with dependency graph).
  - Subtask status indicators: planned, ready, in_progress, completed, failed, cancelled.
  - `ready`/`blocked` filter toggles (resolved server-side).
  - Replan action (batch cancel + create + update subtasks).
  - Links from task to source workflow run.
  - Ref panels: artifacts, docs, files, repositories, labels.
- Note: API routes land in task 32 — UI implementation deferred until then.

**Pages/sections affected:**
- New Tasks page (major new feature, deferred to task 32)
- Run detail: `source_run_id` linkage → Task link

---

### Task 29 — Dispatch Runtime Integration With Provider-Native Sessions

**Backend:** Wires `agent_dispatch` persistence into the live step executor. Session identity (`provider_driver`, `session_id`, `provider_resume_token`) captured durably. Three dispatch modes with runtime semantics.

**UI implications:**
- This task is purely backend plumbing. No new API endpoints are exposed here.
- The UI will benefit from this via task 33 (dispatch API surface) and task 30 (HITL resume).
- No immediate UI changes required from task 29 alone.

**Pages/sections affected:**
- None (backend-only)

---

### Task 30 — HITL Single-Turn Live Pause And Resume

**Backend:** `HitlRegistry` with `tokio::oneshot`; full pause/resume cycle; `active_hitl_request_id` on workflow run; multi-turn within a step.

**UI implications:**
- When a run has `active_hitl_request_id` set, the run detail page must show a prominent HITL prompt panel:
  - Display the HITL question text and context from `context_json`.
  - Show `sequence_no` and `kind` (clarification / approval / choice / freeform).
  - Provide an answer input (text field, or choice buttons for `choice` kind).
  - Submit triggers the answer API (task 33 will expose this).
  - Optimistic UI: after submitting, show "answer submitted, waiting for resume" until run transitions back to `running`.
- Run detail page status banner: "Waiting for your input" when `active_hitl_request_id` is non-null.
- Multi-turn: the same panel should handle sequential questions (sequence_no 1, 2, ...) without page reload.

**Pages/sections affected:**
- Run detail page: HITL answer panel (high-priority UI feature)
- Run list: visual indicator when any run is awaiting HITL

---

## Key UI Priority Classification

### High Priority (Core Workflow)
- **Task 22**: Chat/SSE streaming interface (agent conversations)
- **Task 25**: Workflow CRUD + editor + run trigger
- **Task 30**: HITL answer panel (blocks user on live runs)
- **Task 19**: Run pause/resume/cancel controls

### Medium Priority (Visibility and Management)
- **Task 20**: Agent definition CRUD (structured form, validate/compile)
- **Task 21**: Agent runtime controls + sessions management
- **Task 16**: Run list + checkpoint trail
- **Task 17**: Signal submission + signals panel
- **Task 26**: Schedule CRUD + typed cron editor

### Lower Priority (Supporting Features)
- **Task 28**: Tasks page (pending task 32 API)
- **Task 27**: Skills listing page (read-only)
- **Task 23/24**: Dispatch + HITL detail views (pending task 33 API)
- **Task 18**: Validation issue display in editor

### No Immediate UI Needed
- **Task 29**: Backend wiring only

---

## API Endpoints Summary (New in Tasks 16–30)

| Endpoint Group | Tasks | Status |
|---|---|---|
| `GET/PUT /api/v1/runs`, `GET /api/v1/runs/{id}`, `/checkpoints`, `/signals`, `/dispatches` | 16, 17, 19 | Implemented |
| `POST /api/v1/runs/{id}/signals` | 17 | Implemented |
| `POST /api/v1/runs/{id}/pause`, `/resume`, `/cancel` | 19 | Implemented |
| `GET/POST/PUT/DELETE /api/v1/agents`, `/validate`, `/compile`, `/{id}/compiled` | 20 | Implemented |
| `GET/POST /api/v1/agents/{id}/runtime`, `/start`, `/stop`, `/restart`, `/mode` | 21 | Implemented |
| `GET/POST /api/v1/agents/{id}/sessions`, `/{sid}/activate`, `/reset`, `/compact` | 21 | Implemented |
| `POST /api/v1/agents/{id}/messages`, `/stream`, `/dry-run` | 22 | Implemented |
| `GET/POST/PUT/DELETE /api/v1/workflows`, `/validate`, `/compile`, `/{id}/compiled`, `/{id}/fork`, `/{id}/runtime`, `/{id}/runs`, `/{id}/runs/dry-run` | 25 | Implemented |
| `GET/POST/PUT/DELETE /api/v1/schedules`, `/validate`, `/{id}/fork`, `/{id}/runtime`, `/{id}/enable`, `/{id}/disable`, `/{id}/run-now`, `/{id}/run-now/dry-run` | 26 | Implemented |
| `GET /api/v1/skills`, `GET /api/v1/skills/{id}` | 27 | Implemented |
| Dispatch/HITL/Task/Subtask API routes | 28, 23, 24, 30 | Schema only — API routes in tasks 32, 33 |
