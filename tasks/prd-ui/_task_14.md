## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task_1,task_2,task_6</dependencies>
</task_context>

# Task 14.0: Looper Runs Page

## Overview

Build the Looper Runs page — iterative task execution with real-time SSE progress, subtask grid, and lifecycle controls.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (tasks 34, 39)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- List looper runs from `GET /api/v1/looper-runs` with status/mode filters
- Looper run detail: progress bar (completed/total), execution mode (sequential/parallel), max_parallelism
- Subtask grid: each subtask with status (pending/running/done/failed), dispatch link
- SSE events from `GET /api/v1/looper-runs/{id}/events` driving live progress
- Actions: pause/resume/cancel
- Create looper run form: task selector, execution policy config
</requirements>

## Subtasks

- [ ] 14.1 Create `js/pages/looper.js` — `looperPage()` Alpine component
- [ ] 14.2 Implement looper run list — status/mode filters, progress column (completed/total)
- [ ] 14.3 Implement looper run detail — progress bar, execution mode badge, policy config display
- [ ] 14.4 Implement subtask grid — status indicators per subtask with dispatch links
- [ ] 14.5 Wire SSE from `GET /api/v1/looper-runs/{id}/events` — update progress bar and subtask grid in real-time
- [ ] 14.6 Implement pause/resume/cancel actions
- [ ] 14.7 Implement create looper run form — task selector, mode toggle, max_parallelism, selection strategy
- [ ] 14.8 Add Looper page template in `index_body.html`

## Implementation Details

### SSE Events

| Event | Action |
|-------|--------|
| `run.updated` | Update progress bar (completed, failed counts) |
| `subtask.started` | Set subtask status to "running" |
| `subtask.completed` | Set subtask status to "done", increment progress |
| `subtask.failed` | Set subtask status to "failed", increment failed count |

### API Endpoints Used

All 8 endpoints under `OpenFangAPI.v1.looper.*` from the techspec.

### Relevant Files

- `crates/openfang-api/static/js/pages/looper.js` (NEW)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- `js/pages/looper.js` with list, detail, SSE progress, actions, create form
- Looper page template in HTML

## Tests

### Manual Browser Tests (Required)

- [ ] List looper runs — verify columns display
- [ ] Looper run detail — verify progress bar and subtask grid
- [ ] SSE — verify live progress updates
- [ ] Pause/resume/cancel — verify actions work
- [ ] Create looper run — verify form and creation

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Looper page with live SSE-driven progress
- Subtask grid updates in real-time
- All lifecycle actions functional
