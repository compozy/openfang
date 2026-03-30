## status: completed

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 6.0: Tasks & Subtasks Page

## Overview

Build the Tasks page — the core domain management surface. Tasks are the primary organizational unit in Compozy; all other objects (runs, dispatches, looper runs, artifacts, docs) reference tasks. Includes CRUD, subtask management with dependency visualization, replan action, and linked resource tabs.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (task 32)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- List tasks from `GET /api/v1/tasks` with status/priority/search filters
- Task detail with tabs: Subtasks, Artifacts, Docs, Files
- Create/edit forms for tasks and subtasks
- Replan modal: `POST /api/v1/tasks/{id}/replan` with operation builder
- Subtask list shows ready/blocked badges and dependency indicators
- Subtask inline status progression
</requirements>

## Subtasks

- [x] 6.1 Create `js/pages/tasks.js` — `tasksPage()` Alpine component
- [x] 6.2 Implement task list — fetch, status/priority filters, search, columns: title, status, priority, owner, complexity, created
- [x] 6.3 Implement task create form — title, description, priority, owner fields
- [x] 6.4 Implement task edit form — inline or modal editing
- [x] 6.5 Implement task delete with confirmation
- [x] 6.6 Implement task detail — Subtasks tab with nested list, ready/blocked badges, depends_on indicators
- [x] 6.7 Implement subtask create/edit/delete within task detail
- [x] 6.8 Implement subtask inline status toggle (planned -> in_progress -> completed)
- [x] 6.9 Implement Artifacts tab — linked artifacts from `OpenFangAPI.v1.tasks.artifacts(id)`
- [x] 6.10 Implement Docs tab — linked docs from `OpenFangAPI.v1.tasks.docs(id)`
- [x] 6.11 Implement Files tab — files from `OpenFangAPI.v1.tasks.files(id)`
- [x] 6.12 Implement Replan modal — operation builder for cancel_subtasks, create_subtasks, update_subtasks with reason field
- [x] 6.13 Add Tasks page template in `index_body.html`

## Implementation Details

### API Endpoints Used

- `GET /api/v1/tasks?status=&priority=&q=&limit=&offset=`
- `POST /api/v1/tasks`
- `GET /api/v1/tasks/{id}`
- `PUT /api/v1/tasks/{id}`
- `DELETE /api/v1/tasks/{id}`
- `GET /api/v1/tasks/{id}/subtasks?ready=&blocked=&kind=&assignee_ref=`
- `POST /api/v1/tasks/{id}/subtasks`
- `POST /api/v1/tasks/{id}/replan`
- `GET /api/v1/tasks/{id}/artifacts`
- `GET /api/v1/tasks/{id}/docs`
- `GET /api/v1/tasks/{id}/files`
- `GET /api/v1/subtasks/{id}`
- `PUT /api/v1/subtasks/{id}`
- `DELETE /api/v1/subtasks/{id}`

### Relevant Files

- `crates/openfang-api/static/js/pages/tasks.js` (NEW)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- `js/pages/tasks.js` with full CRUD, subtask management, replan, linked resources
- Tasks page template in HTML
- Replan modal with operation builder

## Tests

### Manual Browser Tests (Required)

- [x] Navigate to Tasks — verify page loads, list displays
- [x] Create a task — verify it appears in list
- [x] Edit a task — verify changes persist
- [x] Delete a task — verify confirmation and removal
- [x] Click task detail — verify Subtasks tab with ready/blocked badges
- [x] Create/edit/delete subtasks — verify CRUD works
- [x] Toggle subtask status inline — verify status changes
- [x] Open Replan modal — verify operation builder works
- [x] View Artifacts/Docs/Files tabs — verify linked resources display

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- Tasks page with full CRUD for tasks and subtasks
- Replan action works with atomic operation builder
- Linked resources (artifacts, docs, files) display correctly
- Subtask dependency indicators show ready/blocked status
