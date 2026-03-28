## status: completed

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 3.0: HITL Inbox Page

## Overview

Build the HITL (Human-in-the-Loop) Inbox page — the most operationally critical new page. Pending HITL requests block workflow execution. This page uses a global SSE stream for real-time notifications and provides inline answer forms for each request kind.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (tasks 31, 33, 42)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms HITL page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Global SSE connection to `/api/v1/hitl-requests/stream?status=pending` in `app.js` driving nav badge count
- HITL page lists all requests with filter by status (pending/answered/cancelled/timed_out)
- Each request card shows: question text, kind badge, run link, dispatch link, time waiting
- Inline answer form adapts to kind: text input for freeform/clarification, approve/reject for approval, choice buttons for choice
- Submit calls `POST /api/v1/hitl-requests/{id}/answer`
- Cancel calls `POST /api/v1/hitl-requests/{id}/cancel`
- Page-scoped SSE updates the list in real-time without polling
- Resilient to backend restarts (post-restart HITL reconstruction is transparent)
</requirements>

## Subtasks

- [x] 3.1 Wire global HITL SSE in `app.js` — connect to `/api/v1/hitl-requests/stream?status=pending` on boot, update `$store.app.pendingHitlCount`
- [x] 3.2 Update nav badge in `index_body.html` — show pending count next to "HITL Inbox" nav item using `$store.app.pendingHitlCount`
- [x] 3.3 Create `js/pages/hitl.js` — `hitlPage()` Alpine component with list, filters, detail view
- [x] 3.4 Implement request list view — fetch from `OpenFangAPI.v1.hitl.list()`, display cards with kind badge, run link, time waiting
- [x] 3.5 Implement status filter — tabs or dropdown for pending/answered/cancelled/timed_out
- [x] 3.6 Implement answer form for `freeform` and `clarification` kinds — text input + submit
- [x] 3.7 Implement answer form for `approval` kind — approve/reject buttons
- [x] 3.8 Implement answer form for `choice` kind — choice buttons from `context_json`
- [x] 3.9 Implement cancel action with confirmation dialog
- [x] 3.10 Wire page-scoped SSE for real-time list updates — `hitl.created` adds to list, `hitl.answered`/`hitl.cancelled` updates status
- [x] 3.11 Add HITL page template section in `index_body.html`
- [x] 3.12 Add badge CSS styles for HITL kinds and status in `components.css`

## Implementation Details

### Global SSE in `app.js`

```js
// In Alpine.store('app').init() or boot sequence
const hitlSSE = OpenFangSSE.connect('/api/v1/hitl-requests/stream?status=pending', {
  'hitl.created': (data) => { Alpine.store('app').pendingHitlCount++ },
  'hitl.answered': (data) => { Alpine.store('app').pendingHitlCount = Math.max(0, Alpine.store('app').pendingHitlCount - 1) },
  'hitl.cancelled': (data) => { Alpine.store('app').pendingHitlCount = Math.max(0, Alpine.store('app').pendingHitlCount - 1) },
}, { reconnect: true })
```

### HITL Request Card Layout

```
+------------------------------------------+
| [APPROVAL]  Run: wf-run-abc123          |
| Agent: code-reviewer (dispatch d-456)    |
|                                          |
| "Should I proceed with the refactoring   |
|  of the auth module? This will change    |
|  15 files."                              |
|                                          |
| [Approve]  [Reject]     3 min ago       |
+------------------------------------------+
```

### API Endpoints Used

- `GET /api/v1/hitl-requests?status=pending` — list requests
- `GET /api/v1/hitl-requests/{id}` — request detail
- `POST /api/v1/hitl-requests/{id}/answer` — submit answer `{ answer: "..." }`
- `POST /api/v1/hitl-requests/{id}/cancel` — cancel request
- `GET /api/v1/hitl-requests/stream?status=pending` — global SSE stream

### Relevant Files

- `crates/openfang-api/static/js/pages/hitl.js` (NEW)
- `crates/openfang-api/static/js/app.js` (MODIFY — global SSE, pendingHitlCount)
- `crates/openfang-api/static/index_body.html` (MODIFY — page template, badge)
- `crates/openfang-api/static/css/components.css` (MODIFY — HITL badges/cards)

### Dependent Files

- `crates/openfang-api/static/js/sse.js` (from task 1)
- `crates/openfang-api/static/js/api-v1.js` (from task 1)
- `crates/openfang-api/static/js/utils.js` (from task 1)

## Deliverables

- `js/pages/hitl.js` with full HITL inbox functionality
- Global SSE driving nav badge in `app.js`
- HITL page template in `index_body.html`
- Answer forms for all 4 HITL kinds
- Real-time updates via SSE

## Tests

### Manual Browser Tests (Required)

- [x] Navigate to HITL Inbox — verify page loads, list displays (may be empty)
- [x] Verify nav badge shows pending count (or 0 if none pending)
- [x] If HITL requests exist: verify cards display question, kind, run link, time waiting
- [x] Submit an answer — verify request status changes to "answered"
- [x] Cancel a request — verify confirmation dialog, then status changes
- [x] Filter by status — verify correct filtering
- [x] Verify SSE connection in Network tab (EventSource to `/api/v1/hitl-requests/stream`)
- [x] Verify badge updates when new HITL request arrives (requires triggering a workflow with HITL)

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- HITL inbox page fully functional with all 4 answer kinds
- Nav badge shows real-time pending count via SSE
- Requests update in real-time without polling
- No regressions in existing pages
