## status: pending

<task_context>
<domain>openfang-api/static + openfang-api/src</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 2.0: Bug Fixes + Navigation Restructure

## Overview

Fix the 3 confirmed bugs in the existing UI and restructure the sidebar navigation into workflow-centric groups to accommodate ~26 pages. This task touches both the Rust backend (server.rs for trigger route registration) and the frontend (JS + HTML).

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/adr-004-navigation.md` and `tasks/prd-ui/adr-007-bugfixes.md`
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms bugs are fixed
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Register missing trigger legacy routes in `server.rs`: `GET /api/triggers`, `PUT /api/triggers/{id}`, `DELETE /api/triggers/{id}`
- Fix `comms.js` SSE: replace references to non-existent `OpenFangAPI.baseUrl`/`apiKey` with correct EventSource construction
- Fix `workflows.js`: replace `window.confirm()` with `OpenFangToast.confirm()`
- Restructure sidebar into 7 groups: Chat, Operations, Workspace, Resources, Outputs, Monitor, System
- Add placeholder nav items for new pages (hitl, runs, dispatches, looper, tasks, triggers, schedules, packs, artifacts, documents, integrations, events)
- Add HITL badge placeholder in nav (driven by SSE in task 3)
- Update hash routing in `app.js` to handle new page names
- Add route aliases for backwards compatibility
</requirements>

## Subtasks

- [ ] 2.1 Register legacy trigger routes in `crates/openfang-api/src/server.rs` — add `GET /api/triggers`, `PUT /api/triggers/{id}`, `DELETE /api/triggers/{id}` using existing handler functions
- [ ] 2.2 Fix `comms.js` SSE — replace `${OpenFangAPI.baseUrl}/api/comms/events/stream` with `new EventSource('/api/comms/events/stream?token=' + (OpenFangAPI.getToken() || ''))` or use `OpenFangSSE` if task 1 is complete
- [ ] 2.3 Fix `workflows.js` — replace `if (confirm(...))` with `OpenFangToast.confirm(title, msg, callback)` pattern
- [ ] 2.4 Restructure sidebar HTML in `index_body.html` — 7 groups with collapsible sections: Chat, Operations (HITL, Runs, Dispatches, Looper), Workspace (Tasks, Workflows, Triggers, Schedules), Resources (Agents, Skills, Hands, Packs), Outputs (Artifacts, Documents), Monitor (Overview, Analytics, Logs), System (Channels, Integrations, Settings, Runtime)
- [ ] 2.5 Update `app.js` routing — add new page names to valid pages list, add redirect aliases, add `pendingHitlCount` to store
- [ ] 2.6 Add placeholder page templates in `index_body.html` for each new page (empty `<template x-if="page === 'hitl'">`... sections)
- [ ] 2.7 Verify trigger tab in scheduler page no longer returns 404
- [ ] 2.8 Verify comms SSE event stream connects successfully
- [ ] 2.9 Verify workflow delete uses styled confirm dialog

## Implementation Details

### Bug Fix 1: Trigger Routes (Rust)

In `server.rs`, the handlers `list_triggers`, `update_trigger`, `delete_trigger` exist but are not registered. Add to the router:

```rust
.route("/api/triggers", get(list_triggers))
.route("/api/triggers/:id", put(update_trigger).delete(delete_trigger))
```

### Bug Fix 2: Comms SSE (JS)

In `comms.js`, `startSSE()` currently does:
```js
const source = new EventSource(`${OpenFangAPI.baseUrl}/api/comms/events/stream`);
```
Replace with:
```js
const token = OpenFangAPI.getToken();
const url = '/api/comms/events/stream' + (token ? `?token=${token}` : '');
const source = new EventSource(url);
```

### Bug Fix 3: Workflow confirm() (JS)

In `workflows.js`, replace:
```js
if (confirm('Are you sure...'))
```
With:
```js
OpenFangToast.confirm('Delete Workflow', 'Are you sure...', async () => { ... })
```

### Navigation Structure

See `tasks/prd-ui/adr-004-navigation.md` for the full group structure.

### Relevant Files

- `crates/openfang-api/src/server.rs` (MODIFY — add trigger routes)
- `crates/openfang-api/static/js/pages/comms.js` (MODIFY — fix SSE)
- `crates/openfang-api/static/js/pages/workflows.js` (MODIFY — fix confirm)
- `crates/openfang-api/static/index_body.html` (MODIFY — nav + placeholder templates)
- `crates/openfang-api/static/js/app.js` (MODIFY — routing, store)
- `crates/openfang-api/static/css/layout.css` (MODIFY — collapsible nav groups if needed)

### Dependent Files

- `crates/openfang-api/src/routes.rs` — trigger handler functions (already exist)

## Deliverables

- Trigger routes registered and returning data (not 404)
- Comms SSE connecting successfully
- Workflow delete using styled confirm dialog
- Sidebar with 7 groups and all ~26 page items
- Hash routing working for all new page names
- Placeholder templates for new pages (showing "Coming soon" or similar)

## Tests

### Unit Tests (Required — Rust)

- [ ] Verify trigger route registration by running `cargo test -p openfang-api --all-features`
- [ ] Existing API integration tests still pass

### Manual Browser Tests (Required)

- [ ] Navigate to Scheduler > Triggers tab — verify trigger list loads (not 404)
- [ ] Navigate to Comms — verify SSE event stream connects (check Network tab)
- [ ] Navigate to Workflows — delete a workflow — verify styled confirm modal appears
- [ ] Click each new nav item — verify correct page name in hash, placeholder content shows
- [ ] Verify all existing pages still work (agents, chat, sessions, approvals, etc.)
- [ ] Verify sidebar groups collapse/expand correctly

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- All 3 bugs fixed and verified
- Sidebar shows 7 groups with all pages
- All existing functionality preserved
- `make fmt && make lint && make test` pass
