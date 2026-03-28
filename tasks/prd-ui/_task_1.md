## status: pending

<task_context>
<domain>openfang-api/static/js</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 1.0: Shared Infrastructure — SSE Client, API v1 Client, Shared Utils

## Overview

Build the three foundational JS modules that all subsequent UI tasks depend on: a shared SSE client utility, a v1 API client extending the existing `OpenFangAPI`, and shared utility functions replacing duplicated logic across pages.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_current_ui.md` before start
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms modules load correctly
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- SSE client must support auto-reconnect with exponential backoff matching the WS pattern: `min(1000 * 2^n, 10000)` ms
- SSE client must support `Last-Event-ID` for seamless reconnection
- SSE client must inject auth token via `?token=` query parameter
- API v1 client must provide typed wrappers for all `/api/v1/` domains (runs, hitl, dispatches, tasks, subtasks, workflows, triggers, schedules, agents, events, looper, packs, artifacts, docs, skills, providerProfiles)
- API v1 client must reuse existing `OpenFangAPI.get/post/put/del` as transport
- API v1 client must handle the v1 error envelope `{error: {code, message, details}}`
- Shared utils must replace the 5+ duplicate `timeAgo` implementations
- Shared utils must provide `statusBadge(status)` mapping status strings to CSS classes
- Shared utils must provide `confirmAction()` wrapping `OpenFangToast.confirm()`
</requirements>

## Subtasks

- [ ] 1.1 Create `js/sse.js` — `OpenFangSSE` module with `connect(path, handlers, options)`, auto-reconnect, `Last-Event-ID`, auth token injection, connection state tracking
- [ ] 1.2 Create `js/api-v1.js` — `OpenFangAPI.v1` namespace with all domain methods as specified in the techspec
- [ ] 1.3 Create `js/utils.js` — `OpenFangUtils` with `timeAgo()`, `formatDate()`, `formatDateTime()`, `statusBadge()`, `confirmAction()`, `truncate()`, `copyToClipboard()`
- [ ] 1.4 Add `<script>` tags for new files in `index_head.html` (load order: utils.js before api-v1.js before sse.js)
- [ ] 1.5 Verify all three modules load without errors in the browser console
- [ ] 1.6 Verify `OpenFangAPI.v1.runs.list()` makes a correct GET request to `/api/v1/runs`
- [ ] 1.7 Verify `OpenFangSSE.connect('/api/logs/stream', {...})` establishes an EventSource connection

## Implementation Details

### `js/sse.js` — SSE Client

```js
const OpenFangSSE = {
  connect(path, handlers, options = {}) {
    // path: '/api/v1/hitl-requests/stream?status=pending'
    // handlers: { 'hitl.created': fn, 'hitl.answered': fn }
    // options: { reconnect: true, lastEventId: null, token: null }
    // Returns: { close(), isConnected() }
  }
}
```

- Use `EventSource` with `?token=` for auth
- Parse `event:` field to route to correct handler
- Track `lastEventId` from each event for reconnection
- On error: if `reconnect: true`, backoff with `min(1000 * 2^attempt, 10000)` ms, max 5 attempts
- Integrate with `Alpine.store('app').connectionState` if available

### `js/api-v1.js` — API v1 Client

Extends `OpenFangAPI` with a `v1` namespace. Each domain method is a thin wrapper:

```js
OpenFangAPI.v1 = {
  runs: {
    list(params) { return OpenFangAPI.get('/api/v1/runs?' + new URLSearchParams(params)) },
    get(id) { return OpenFangAPI.get(`/api/v1/runs/${id}`) },
    // ... etc
  },
  // ... all domains from techspec
}
```

See `tasks/prd-ui/techspec.md` section "Core Interfaces > API Client v1" for the full method list.

### `js/utils.js` — Shared Utilities

```js
const OpenFangUtils = {
  timeAgo(dateStr) { /* single implementation */ },
  formatDate(dateStr) { /* "Mar 27, 2026" */ },
  formatDateTime(dateStr) { /* "Mar 27, 2026 3:45 PM" */ },
  statusBadge(status) { /* maps to CSS class: 'badge running', 'badge failed', etc */ },
  confirmAction(title, msg, onConfirm) { /* wraps OpenFangToast.confirm() */ },
  truncate(str, maxLen) { /* truncate with ellipsis */ },
  copyToClipboard(text) { /* clipboard API */ }
}
```

### Relevant Files

- `crates/openfang-api/static/js/sse.js` (NEW)
- `crates/openfang-api/static/js/api-v1.js` (NEW)
- `crates/openfang-api/static/js/utils.js` (NEW)
- `crates/openfang-api/static/index_head.html` (MODIFY — add script tags)
- `crates/openfang-api/static/js/api.js` (REFERENCE — existing transport layer)

### Dependent Files

- `crates/openfang-api/static/js/app.js` — will use SSE in task 2
- All `js/pages/*.js` — will use API v1 and utils in subsequent tasks

## Deliverables

- `js/sse.js` with `OpenFangSSE.connect()` supporting reconnect, `Last-Event-ID`, auth
- `js/api-v1.js` with `OpenFangAPI.v1.*` covering all v1 domains from techspec
- `js/utils.js` with shared utilities
- Updated `index_head.html` with script tags in correct load order
- No regressions in existing pages (all existing functionality still works)

## Tests

### Manual Browser Tests (Required)

- [ ] Open dashboard, verify no console errors from new scripts
- [ ] Run `OpenFangAPI.v1.runs.list()` in console, verify network request to correct URL
- [ ] Run `OpenFangSSE.connect('/api/logs/stream', {message: console.log})` in console, verify EventSource connection
- [ ] Run `OpenFangUtils.timeAgo(new Date(Date.now() - 60000).toISOString())` in console, verify "1 minute ago"
- [ ] Run `OpenFangUtils.statusBadge('running')` in console, verify returns correct CSS class

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- All three modules load without errors in the browser
- `OpenFangAPI.v1` has methods for all 16 domains from the techspec
- `OpenFangSSE.connect()` establishes EventSource connections with auth
- `OpenFangUtils` functions work correctly in browser console
- No regressions in existing dashboard functionality
