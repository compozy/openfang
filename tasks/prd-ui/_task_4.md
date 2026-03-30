## status: completed

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 4.0: Workflow Runs Page

## Overview

Build the Workflow Runs page — the primary execution visibility surface. Shows all workflow runs with 8 status states, run detail with 6 tabs (overview, checkpoints, signals, dispatches, HITL, events), control actions (pause/resume/cancel), and live SSE event streaming.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_16_30.md` (tasks 16, 17, 19)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms Runs page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- List all workflow runs from `GET /api/v1/runs` with status and workflow filters
- 8 status badges: pending, running, waiting_signal, waiting_hitl, paused, completed, failed, cancelled
- Run detail view with 6 tabs: Overview, Checkpoints, Signals, Dispatches, HITL, Events
- Signals tab includes "Send Signal" form with name, payload, source fields
- Dispatches tab shows run-scoped dispatches with retry/cancel actions
- HITL tab shows run-scoped HITL requests with answer forms
- Events tab shows live SSE stream from `GET /api/v1/runs/{id}/events`
- Action buttons: Pause/Resume/Cancel conditionally enabled per status
- Recovery indicator tooltip on paused runs with `run_recovered_needs_resume` checkpoint
</requirements>

## Subtasks

- [x] 4.1 Create `js/pages/runs.js` — `runsPage()` Alpine component
- [x] 4.2 Implement run list view — fetch from `OpenFangAPI.v1.runs.list()`, status/workflow filters, search
- [x] 4.3 Implement 8 status badges with distinct colors using `OpenFangUtils.statusBadge()`
- [x] 4.4 Implement run detail — Overview tab: status, workflow name, timestamps, current step, progress
- [x] 4.5 Implement Checkpoints tab — timeline from `OpenFangAPI.v1.runs.checkpoints(id)`, recovery indicator
- [x] 4.6 Implement Signals tab — signal list + "Send Signal" form calling `OpenFangAPI.v1.runs.sendSignal(id, sig)`
- [x] 4.7 Implement Dispatches tab — table from `OpenFangAPI.v1.runs.dispatches(id)` with status, kind, agent, retry/cancel
- [x] 4.8 Implement HITL tab — requests from `OpenFangAPI.v1.runs.hitlRequests(id)` with inline answer forms
- [x] 4.9 Implement Events tab — live SSE from `GET /api/v1/runs/{id}/events` rendered as scrollable event log
- [x] 4.10 Implement action buttons — Pause/Resume/Cancel with conditional enable states
- [x] 4.11 Add Runs page template in `index_body.html`
- [x] 4.12 Add waiting_signal and waiting_hitl badge styles in `components.css`

## Implementation Details

### Run Status State Machine

```
pending -> running -> completed
                   -> failed
                   -> cancelled
         -> waiting_signal (waiting for external signal)
         -> waiting_hitl (waiting for human input)
         -> paused (manual pause or recovery)
```

### Action Button Enable States

| Status | Pause | Resume | Cancel |
|--------|-------|--------|--------|
| pending | - | - | Yes |
| running | Yes | - | Yes |
| waiting_signal | Yes | - | Yes |
| waiting_hitl | - | - | Yes |
| paused | - | Yes | Yes |
| completed | - | - | - |
| failed | - | - | - |
| cancelled | - | - | - |

### API Endpoints Used

- `GET /api/v1/runs?status=&workflow_id=&limit=&offset=`
- `GET /api/v1/runs/{id}`
- `GET /api/v1/runs/{id}/checkpoints`
- `GET /api/v1/runs/{id}/signals`
- `POST /api/v1/runs/{id}/signals`
- `GET /api/v1/runs/{id}/dispatches`
- `GET /api/v1/runs/{id}/hitl-requests`
- `GET /api/v1/runs/{id}/events` (SSE)
- `POST /api/v1/runs/{id}/pause`
- `POST /api/v1/runs/{id}/resume`
- `POST /api/v1/runs/{id}/cancel`

### Relevant Files

- `crates/openfang-api/static/js/pages/runs.js` (NEW)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- `js/pages/runs.js` with full run list and detail functionality
- 6-tab detail view with all interactions
- SSE event streaming in Events tab
- Action buttons with correct enable states
- Runs page template in HTML

## Tests

### Manual Browser Tests (Required)

- [x] Navigate to Runs — verify page loads, list displays
- [x] Filter by status — verify correct filtering
- [x] Click a run — verify detail view with 6 tabs
- [x] Checkpoints tab — verify timeline displays
- [x] Signals tab — send a signal, verify it appears in list
- [x] Events tab — verify SSE connection and live events
- [x] Pause a running run — verify status changes
- [x] Resume a paused run — verify status changes
- [x] Cancel a run — verify confirmation and status change

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- Runs page fully functional with list, detail, 6 tabs, actions
- SSE events stream correctly in Events tab
- All 8 status badges display correctly
- Action buttons conditionally enabled
