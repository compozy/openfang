# Technical Specification: OpenFang UI Integration

## Executive Summary

This tech spec covers the full integration of OpenFang's v1 backend APIs into the web dashboard. The existing Alpine.js SPA (16 pages, ~90 legacy endpoints) will be extended with ~15 new page modules and 3 shared infrastructure modules, migrating all pages to `/api/v1/` endpoints and wiring 6 SSE streams for real-time updates. The work is organized into 7 phases, starting with shared infrastructure and bug fixes, then building outward from critical operations (HITL, Runs) through authoring (Workflows, Triggers) to supporting features (Packs, Artifacts).

Total scope: ~140 previously unused endpoints will be integrated. ~15 new JS files, ~14 modified JS files, navigation restructured into 7 groups, and 1 backend bug fix (`server.rs`).

## System Architecture

### Domain Placement

All UI code lives in `crates/openfang-api/static/`:

- `js/api.js` — existing API client (transport layer)
- `js/api-v1.js` — new v1 API client with domain-specific wrappers
- `js/sse.js` — new shared SSE client utility
- `js/utils.js` — new shared utilities (timeAgo, statusBadge, confirmAction)
- `js/app.js` — Alpine.js app bootstrap, global store, navigation
- `js/pages/*.js` — one file per page/domain
- `css/` — theme, layout, components (extended, not replaced)
- `index_body.html` — SPA template with all page sections
- `index_head.html` — script/style imports

Backend change: `crates/openfang-api/src/server.rs` — register missing trigger routes.

### Component Overview

```
Browser
  |
  +-- app.js (Alpine.js global store, routing, global HITL SSE)
  |     |
  |     +-- api.js (transport: get/post/put/del with auth)
  |     |     |
  |     |     +-- api-v1.js (v1 domain wrappers: runs, hitl, tasks, ...)
  |     |
  |     +-- sse.js (SSE client: connect, reconnect, event routing)
  |     |
  |     +-- utils.js (timeAgo, statusBadge, confirmAction)
  |     |
  |     +-- pages/*.js (one Alpine component per page)
  |
  +-- index_body.html (nav + page templates)
  +-- css/ (theme tokens + component styles)
```

Data flow: pages call `OpenFangAPI.v1.*` methods which use `OpenFangAPI.get/post` as transport. Real-time data arrives via `OpenFangSSE.connect()` which creates `EventSource` connections with auth tokens. The global HITL SSE stream runs in `app.js` and updates `$store.app.pendingHitlCount` for the nav badge.

## Implementation Design

### Core Interfaces

#### API Client v1 (`js/api-v1.js`)

```js
// Extends OpenFangAPI with v1 domain methods
OpenFangAPI.v1 = {
  runs: {
    list(params)        // GET /api/v1/runs?status=&workflow_id=&limit=&offset=
    get(id)             // GET /api/v1/runs/{id}
    pause(id)           // POST /api/v1/runs/{id}/pause
    resume(id)          // POST /api/v1/runs/{id}/resume
    cancel(id)          // POST /api/v1/runs/{id}/cancel
    checkpoints(id)     // GET /api/v1/runs/{id}/checkpoints
    signals(id)         // GET /api/v1/runs/{id}/signals
    sendSignal(id, sig) // POST /api/v1/runs/{id}/signals
    dispatches(id)      // GET /api/v1/runs/{id}/dispatches
    hitlRequests(id)    // GET /api/v1/runs/{id}/hitl-requests
  },
  hitl: {
    list(params)        // GET /api/v1/hitl-requests?status=&run_id=
    get(id)             // GET /api/v1/hitl-requests/{id}
    answer(id, body)    // POST /api/v1/hitl-requests/{id}/answer
    cancel(id)          // POST /api/v1/hitl-requests/{id}/cancel
  },
  dispatches: {
    list(params)        // GET /api/v1/dispatches?status=&kind=
    get(id)             // GET /api/v1/dispatches/{id}
    children(id)        // GET /api/v1/dispatches/{id}/children
    retry(id)           // POST /api/v1/dispatches/{id}/retry
    cancel(id)          // POST /api/v1/dispatches/{id}/cancel
  },
  tasks: {
    list(params)        // GET /api/v1/tasks?status=&priority=&q=
    get(id)             // GET /api/v1/tasks/{id}
    create(body)        // POST /api/v1/tasks
    update(id, body)    // PUT /api/v1/tasks/{id}
    delete(id)          // DELETE /api/v1/tasks/{id}
    subtasks(id, p)     // GET /api/v1/tasks/{id}/subtasks?ready=&blocked=
    replan(id, body)    // POST /api/v1/tasks/{id}/replan
    artifacts(id)       // GET /api/v1/tasks/{id}/artifacts
    docs(id)            // GET /api/v1/tasks/{id}/docs
    files(id)           // GET /api/v1/tasks/{id}/files
  },
  subtasks: {
    list(params)        // GET /api/v1/subtasks?task_id=&status=
    get(id)             // GET /api/v1/subtasks/{id}
    update(id, body)    // PUT /api/v1/subtasks/{id}
    delete(id)          // DELETE /api/v1/subtasks/{id}
  },
  workflows: {
    list()              // GET /api/v1/workflows
    get(id)             // GET /api/v1/workflows/{id}
    create(body)        // POST /api/v1/workflows
    update(id, body)    // PUT /api/v1/workflows/{id}
    delete(id)          // DELETE /api/v1/workflows/{id}
    validate(body)      // POST /api/v1/workflows/validate
    compile(body)       // POST /api/v1/workflows/compile
    compiled(id)        // GET /api/v1/workflows/{id}/compiled
    fork(id)            // POST /api/v1/workflows/{id}/fork
    runtime(id)         // GET /api/v1/workflows/{id}/runtime
    runs(id)            // GET /api/v1/workflows/{id}/runs
    startRun(id, body)  // POST /api/v1/workflows/{id}/runs
    dryRun(id, body)    // POST /api/v1/workflows/{id}/runs/dry-run
  },
  triggers: {
    list()              // GET /api/v1/triggers
    get(id)             // GET /api/v1/triggers/{id}
    create(body)        // POST /api/v1/triggers
    update(id, body)    // PUT /api/v1/triggers/{id}
    delete(id)          // DELETE /api/v1/triggers/{id}
    validate(body)      // POST /api/v1/triggers/validate
    compile(body)       // POST /api/v1/triggers/compile
    compiled(id)        // GET /api/v1/triggers/{id}/compiled
    fork(id)            // POST /api/v1/triggers/{id}/fork
    runtime(id)         // GET /api/v1/triggers/{id}/runtime
    enable(id)          // POST /api/v1/triggers/{id}/enable
    disable(id)         // POST /api/v1/triggers/{id}/disable
    test(id, event)     // POST /api/v1/triggers/{id}/test
  },
  schedules: {
    list()              // GET /api/v1/schedules
    get(id)             // GET /api/v1/schedules/{id}
    create(body)        // POST /api/v1/schedules
    update(id, body)    // PUT /api/v1/schedules/{id}
    delete(id)          // DELETE /api/v1/schedules/{id}
    validate(body)      // POST /api/v1/schedules/validate
    fork(id)            // POST /api/v1/schedules/{id}/fork
    runtime(id)         // GET /api/v1/schedules/{id}/runtime
    enable(id)          // POST /api/v1/schedules/{id}/enable
    disable(id)         // POST /api/v1/schedules/{id}/disable
    runNow(id)          // POST /api/v1/schedules/{id}/run-now
    dryRun(id)          // POST /api/v1/schedules/{id}/run-now/dry-run
  },
  agents: {
    list()              // GET /api/v1/agents
    get(id)             // GET /api/v1/agents/{id}
    create(body)        // POST /api/v1/agents
    update(id, body)    // PUT /api/v1/agents/{id}
    delete(id)          // DELETE /api/v1/agents/{id}
    validate(body)      // POST /api/v1/agents/validate
    compile(body)       // POST /api/v1/agents/compile
    compiled(id)        // GET /api/v1/agents/{id}/compiled
    runtime(id)         // GET /api/v1/agents/{id}/runtime
    startRuntime(id)    // POST /api/v1/agents/{id}/runtime/start
    stopRuntime(id)     // POST /api/v1/agents/{id}/runtime/stop
    restartRuntime(id)  // POST /api/v1/agents/{id}/runtime/restart
    setMode(id, body)   // PUT /api/v1/agents/{id}/runtime/mode
    sessions(id)        // GET /api/v1/agents/{id}/sessions
    createSession(id)   // POST /api/v1/agents/{id}/sessions
    activateSession(id, sid) // POST /api/v1/agents/{id}/sessions/{sid}/activate
    resetSession(id, sid)    // POST /api/v1/agents/{id}/sessions/{sid}/reset
    compactSession(id, sid)  // POST /api/v1/agents/{id}/sessions/{sid}/compact
    sendMessage(id, body)    // POST /api/v1/agents/{id}/messages
    dryRunMessage(id, body)  // POST /api/v1/agents/{id}/messages/dry-run
  },
  events: {
    send(body)          // POST /api/v1/events
    dryRun(body)        // POST /api/v1/events/dry-run
  },
  looper: {
    list(params)        // GET /api/v1/looper-runs?status=&execution_mode=
    get(id)             // GET /api/v1/looper-runs/{id}
    create(body)        // POST /api/v1/looper-runs
    subtasks(id)        // GET /api/v1/looper-runs/{id}/subtasks
    pause(id)           // POST /api/v1/looper-runs/{id}/pause
    resume(id)          // POST /api/v1/looper-runs/{id}/resume
    cancel(id)          // POST /api/v1/looper-runs/{id}/cancel
  },
  packs: {
    list()              // GET /api/v1/packs
    get(id)             // GET /api/v1/packs/{id}
    objects(id)         // GET /api/v1/packs/{id}/objects
    install(body)       // POST /api/v1/packs/install
    upgrade(id)         // POST /api/v1/packs/{id}/upgrade
    upgradeDryRun(id)   // POST /api/v1/packs/{id}/upgrade/dry-run
    uninstall(id)       // POST /api/v1/packs/{id}/uninstall
    fork(id)            // POST /api/v1/packs/{id}/fork
  },
  artifacts: {
    list(params)        // GET /api/v1/artifacts?artifact_type=&task_id=&q=
    get(id)             // GET /api/v1/artifacts/{id}
    versions(id)        // GET /api/v1/artifacts/{id}/versions
  },
  docs: {
    list(params)        // GET /api/v1/docs?task_id=&q=
    get(id)             // GET /api/v1/docs/{id}
    versions(id)        // GET /api/v1/docs/{id}/versions
  },
  skills: {
    list(params)        // GET /api/v1/skills?q=&limit=&offset=
    get(id)             // GET /api/v1/skills/{id}
  },
  providerProfiles: {
    list()              // GET /api/v1/provider-profiles
    get(id)             // GET /api/v1/provider-profiles/{id}
    create(body)        // POST /api/v1/provider-profiles
    update(id, body)    // PUT /api/v1/provider-profiles/{id}
    delete(id)          // DELETE /api/v1/provider-profiles/{id}
  }
}
```

#### SSE Client (`js/sse.js`)

```js
const OpenFangSSE = {
  // Connect to an SSE endpoint with event type routing
  connect(path, handlers, options = {}) {
    // path: '/api/v1/hitl-requests/stream?status=pending'
    // handlers: { 'hitl.created': fn, 'hitl.answered': fn, ... }
    // options: { reconnect: true, lastEventId: null, token: null }
    // Returns: { close(), isConnected() }
  }
}
```

#### Shared Utilities (`js/utils.js`)

```js
const OpenFangUtils = {
  timeAgo(dateStr)           // "2 minutes ago", "1 hour ago", etc.
  formatDate(dateStr)        // "Mar 27, 2026"
  formatDateTime(dateStr)    // "Mar 27, 2026 3:45 PM"
  statusBadge(status)        // returns CSS class for badge color
  confirmAction(title, msg, onConfirm)  // OpenFangToast.confirm wrapper
  truncate(str, maxLen)      // truncate with ellipsis
  copyToClipboard(text)      // clipboard API wrapper
}
```

### Data Models

The v1 API uses structured JSON for all entities. Key models consumed by the UI:

**WorkflowRun**: `{ id, workflow_id, status, current_step_index, started_at, completed_at, error_message, waiting_kind, waiting_ref, active_hitl_request_id, active_dispatch_id }`

**HitlRequest**: `{ id, run_id, dispatch_id, step_index, sequence_no, kind, status, context_json, answer_json, created_at, answered_at }`

**Dispatch**: `{ id, run_id, step_index, parent_dispatch_id, kind, status, target_agent_id, attempt, provider_driver, session_id, created_at, completed_at }`

**Task**: `{ id, slug, title, description, status, priority, complexity, owner, source_run_id, created_at, updated_at }`

**Subtask**: `{ id, task_id, position, title, kind, status, assignee_ref, depends_on, parallelizable, created_at }`

**WorkflowV2Definition**: `{ id, name, version, enabled, tags, steps[], defaults, input, output, outputs }`

**TriggerV2**: `{ id, name, enabled, match { event, source, contains, filters }, target { kind, ... }, max_fires, cooldown_secs }`

**PackManifest**: `{ id, name, version, source, managed_objects[] }`

**Artifact**: `{ id, artifact_type, title, task_id, current_version_no, created_at }`

**ArtifactVersion**: `{ id, artifact_id, version_no, content_json, content_hash, created_by_kind, created_by_ref, created_at }`

**ProviderBlock** (within AgentDefinition): `{ driver, model, profile, defaults: { max_tokens, reasoning_effort }, config: CodexConfig | ClaudeCodeConfig | ClaudeCompatibleConfig, request_extra }`

**ProviderProfileConfig**: `{ id, name, driver, model, defaults: { max_tokens, reasoning_effort }, config: ProviderBehaviorLayer }`

**ProviderBinding** (compiled output): `{ driver, provider_id, model: { id, provider_model_id }, profile, defaults, config: ResolvedProviderBehaviorConfig }`

**Arky Driver Taxonomy** (10 valid drivers): `codex`, `claude-code`, `openrouter`, `bedrock`, `vertex`, `ollama`, `zai`, `vercel`, `moonshot`, `minimax`

**Driver-Specific Config Namespaces**:
- `codex`: `{ sandbox_mode, sandbox_network_access, include_plan_tool, resume_last, web_search, rmcp_client, reasoning_summary, model_verbosity }`
- `claude_code`: `{ continue_conversation, fork_session, additional_directories, enable_file_checkpointing, allowed_tools, disallowed_tools, mcp_servers, max_budget_usd, fallback_model }`
- `claude_compatible`: extends claude_code with `{ selected_model, region, project_id }`

### API Endpoints

See `tasks/prd-ui/analysis_api_routes.md` for the complete endpoint inventory (~230 endpoints). The API client v1 section above covers all endpoints to be integrated.

### SSE Streams

| Stream | Endpoint | Event Types | Used By |
|--------|----------|-------------|---------|
| HITL Global | `GET /api/v1/hitl-requests/stream` | `hitl.created`, `hitl.answered`, `hitl.cancelled`, `keepalive` | app.js (global badge), hitl.js |
| Run Events | `GET /api/v1/runs/{id}/events` | `run.updated`, `step.started`, `step.completed`, `checkpoint.created`, `signal.received`, `keepalive` | runs.js |
| Dispatch Events | `GET /api/v1/dispatches/{id}/events` | `dispatch.updated`, `dispatch.completed`, `keepalive` | dispatches.js |
| Looper Events | `GET /api/v1/looper-runs/{id}/events` | `run.updated`, `subtask.started`, `subtask.completed`, `subtask.failed`, `keepalive` | looper.js |
| Comms Events | `GET /api/comms/events/stream` | agent communication events | comms.js |
| Logs | `GET /api/logs/stream` | log lines | logs.js (already wired) |

## Impact Analysis

| Affected Component | Type of Impact | Description & Risk Level | Required Action |
|---------------------|----------------|--------------------------|-----------------|
| `server.rs` | Route Registration | Add 3 missing trigger legacy routes. Low risk. | Add routes, test |
| `index_body.html` | Template Changes | New nav structure, ~15 new page sections. Medium risk. | Incremental additions per phase |
| `js/app.js` | Store Changes | New nav groups, HITL SSE, new route aliases. Medium risk. | Careful testing of routing |
| `js/api.js` | Minor Fix | Fix comms SSE bug. Low risk. | Targeted fix |
| `js/pages/workflows.js` | Full Rebuild | Migrate to v1 API. High risk. | Thorough regression testing |
| `js/pages/agents.js` | Full Rebuild | Migrate to v1 API. High risk. | Thorough regression testing |
| `js/pages/scheduler.js` | Deprecated | Split into triggers.js + schedules.js. High risk. | Verify all scheduler features preserved |
| `css/components.css` | Additions | New badge variants, card styles. Low risk. | Visual review |

## Testing Approach

### Per-Page Testing

Each new page must be manually tested against a running daemon:

1. Build fresh: `cargo build --release -p openfang-cli`
2. Start daemon: `target/release/openfang start &`
3. Verify page loads, data displays, CRUD operations work
4. Test SSE connections (check browser DevTools Network tab for EventSource)
5. Test error states (stop daemon, verify graceful degradation)

### SSE Testing

- Verify `EventSource` connects with auth token
- Verify reconnection after brief disconnection
- Verify `Last-Event-ID` resume
- Verify global HITL badge updates in real-time

### Regression Testing

- After each page rebuild (workflows, agents, scheduler), verify all existing functionality preserved
- Run existing integration tests: `cargo test -p openfang-api --all-features`

## Development Sequencing

### Build Order

1. **Phase 0: Infrastructure + Bug Fixes** (~3 files new, ~4 files modified)
   - `js/sse.js`, `js/utils.js`, `js/api-v1.js` (new shared modules)
   - Bug fixes: `server.rs`, `comms.js`, `workflows.js`
   - Navigation restructure in `index_body.html` + `app.js`
   - Why first: all subsequent phases depend on these

2. **Phase 1: HITL + Runs + Dispatches** (~3 new page files)
   - `js/pages/hitl.js` — HITL inbox with global SSE
   - `js/pages/runs.js` — run list + detail with SSE events
   - `js/pages/dispatches.js` — dispatch list + detail
   - Why second: HITL blocks production workflows, highest operational urgency

3. **Phase 2: Tasks + Subtasks** (~1 new page file)
   - `js/pages/tasks.js` — task CRUD, subtask management, replan
   - Why third: core domain object that runs, dispatches, and looper reference

4. **Phase 3: Workflows v2 + Triggers v2 + Schedules v2 + Events** (~3 new, ~1 rebuilt)
   - Rebuild `js/pages/workflows.js` on v1
   - New `js/pages/triggers.js` replacing scheduler triggers tab
   - New `js/pages/schedules.js` replacing scheduler cron tab
   - New `js/pages/events.js` for event ingress testing
   - Why fourth: authoring tools depend on understanding the runtime (phases 1-2)

5. **Phase 4: Agents v1 + Chat** (~2 rebuilt)
   - Rebuild `js/pages/agents.js` on v1
   - Update `js/pages/chat.js` with SSE streaming + dry-run
   - Why fifth: agents page is most complex rebuild, benefits from v1 client maturity

6. **Phase 4.5: Arky Provider System** (~1 new page, ~2 modified)
   - Backend prerequisite: `/api/v1/provider-profiles` CRUD endpoint + fix `known_profiles` validation
   - New provider profiles management page (in Settings or standalone)
   - Spawn wizard upgrade: Arky driver selector with driver-specific config fields
   - Agent detail: provider config section with compiled binding inspector
   - Reasoning effort selector, max_tokens override
   - Inline MCP server distinction from global MCP
   - Why here: depends on Phase 4 agents rebuild, enhances it with full provider awareness

7. **Phase 5: Looper + Packs + Artifacts + Documents** (~4 new page files)
   - `js/pages/looper.js`, `js/pages/packs.js`, `js/pages/artifacts.js`, `js/pages/documents.js`
   - Why sixth: supporting features that enhance the core workflow

8. **Phase 6: Budget + Integrations + A2A + Minor** (~1 new, ~5 updated)
   - New `js/pages/integrations.js`
   - Update `js/pages/usage.js` (budget), `js/pages/comms.js` (A2A, enhanced SSE)
   - Update `js/pages/overview.js`, `js/pages/settings.js`, `js/pages/sessions.js`
   - Why last: polish and completeness

### Technical Dependencies

- Phase 0 must complete before all other phases
- Phase 1 depends on Phase 0 (SSE client, API v1 client)
- Phases 2-6 depend on Phase 0 but are otherwise independent of each other
- Phase 3 (scheduler rebuild) should happen before Phase 6 (comms/settings updates) to avoid duplicate work

## Technical Considerations

### Key Decisions

See ADR-001 through ADR-008 in `tasks/prd-ui/` for full rationale on:
1. Audience: both operators and developers (ADR-001)
2. Framework: stay on Alpine.js (ADR-002)
3. API migration: all pages to v1 (ADR-003)
4. Navigation: workflow-centric groups (ADR-004)
5. Real-time: SSE everywhere (ADR-005)
6. Depth: full CRUD (ADR-006)
7. Bug fixes: all 3 first (ADR-007)
8. Build order: infra -> HITL -> runs -> tasks -> workflows -> rest (ADR-008)
9. Arky provider UI: dedicated Phase 4.5 (ADR-009)

### Known Risks

1. **Browser SSE connection limit**: Browsers allow ~6 connections per domain. With global HITL SSE + page-scoped SSE + WS for chat, we could hit this. Mitigation: only open page-scoped SSE on the active page; close on navigation.

2. **v1 endpoint behavior differences**: Some v1 endpoints may behave differently from legacy equivalents. Mitigation: test each migrated page thoroughly before moving to the next.

3. **Missing backend tests**: Task 33.9 (dispatch/HITL HTTP tests) was never implemented. The endpoints work but have no automated contract tests. Mitigation: manual testing during UI integration.

4. **Workflow editor complexity**: The v2 workflow editor (8 step kinds, flow modes, contracts) is the most complex single UI. Mitigation: build iteratively within Phase 3, starting with list/detail before the editor.

5. **Arky provider-profiles backend dependency**: Phase 4.5 requires a new `/api/v1/provider-profiles` CRUD endpoint that does not yet exist. This is a backend task that must be completed before the provider profiles UI. Mitigation: driver-specific config and reasoning effort can be built using existing agent definition fields while the profiles endpoint is being developed.

6. **ValidationContext.known_profiles always empty**: Agent validation silently skips profile reference checks. This is a latent correctness bug in the backend. Mitigation: file as a backend bug to fix alongside the provider-profiles endpoint.

### Standards Compliance

- All JS follows existing patterns (IIFE singletons, Alpine component functions)
- CSS uses existing design tokens (no new color primitives)
- No build step introduced
- No new CDN dependencies (Alpine.js, marked.js, highlight.js remain)
- All destructive actions use `OpenFangToast.confirm()` (fixes the confirm() bug pattern)
