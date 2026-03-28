## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 5.0: Dispatches Page

## Overview

Build the Dispatches page showing agent dispatch records with parent-child lineage trees, retry/cancel actions, and SSE event streaming. Dispatches represent individual agent executions within a workflow run.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (task 33)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- List dispatches from `GET /api/v1/dispatches` with status/kind filters
- Dispatch detail: kind (call/send/spawn), status, target agent, attempt count
- Parent-child lineage tree view for multi-agent delegation
- Actions: retry (`POST .../retry`), cancel (`POST .../cancel`)
- SSE events from `GET /api/v1/dispatches/{id}/events`
</requirements>

## Subtasks

- [ ] 5.1 Create `js/pages/dispatches.js` — `dispatchesPage()` Alpine component
- [ ] 5.2 Implement dispatch list — fetch, status/kind filters, columns: ID, kind, status, agent, attempt, timestamps
- [ ] 5.3 Implement dispatch detail — full info, parent link, children list from `OpenFangAPI.v1.dispatches.children(id)`
- [ ] 5.4 Implement parent-child lineage tree — recursive tree rendering for delegation chains
- [ ] 5.5 Implement retry action with confirmation
- [ ] 5.6 Implement cancel action with confirmation
- [ ] 5.7 Wire SSE from `GET /api/v1/dispatches/{id}/events` for live status updates in detail view
- [ ] 5.8 Add Dispatches page template in `index_body.html`
- [ ] 5.9 Add dispatch kind badges (call/send/spawn) in `components.css`

## Implementation Details

### API Endpoints Used

- `GET /api/v1/dispatches?status=&kind=`
- `GET /api/v1/dispatches/{id}`
- `GET /api/v1/dispatches/{id}/children`
- `POST /api/v1/dispatches/{id}/retry`
- `POST /api/v1/dispatches/{id}/cancel`
- `GET /api/v1/dispatches/{id}/events` (SSE)

### Relevant Files

- `crates/openfang-api/static/js/pages/dispatches.js` (NEW)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- `js/pages/dispatches.js` with list, detail, tree view, actions, SSE
- Dispatches page template in HTML
- Dispatch kind badges

## Tests

### Manual Browser Tests (Required)

- [ ] Navigate to Dispatches — verify page loads
- [ ] Filter by status/kind — verify filtering works
- [ ] Click a dispatch — verify detail with parent/children
- [ ] Retry a failed dispatch — verify confirmation and status change
- [ ] Verify SSE connection in detail view

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Dispatches page functional with list, detail, tree, actions, SSE
- Kind badges display correctly (call/send/spawn)
- Parent-child tree renders delegation chains
