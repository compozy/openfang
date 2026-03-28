# UI Integration Design for OpenFang v1 Backend Features

> Date: 2026-03-27
> Status: Accepted
> Scope: Full dashboard overhaul to integrate ~140 unused backend endpoints

---

## Problem Statement

The OpenFang backend has evolved significantly through PRD-Compozy (43 tasks) and PRD-CLI (8 tasks), introducing a rich `/api/v1/` namespace with workflows v2, runs, dispatches, HITL, tasks, subtasks, looper runs, packs, triggers v2, schedules v2, artifacts, docs, and events. The web dashboard still operates entirely on legacy `/api/` endpoints, leaving ~140 endpoints unused and major features invisible to users.

Three confirmed bugs compound the problem:
1. Trigger routes (`/api/triggers`) are not registered in `server.rs` — scheduler trigger tab returns 404
2. `comms.js` SSE references non-existent `OpenFangAPI.baseUrl`/`apiKey` properties — SSE silently fails
3. `workflows.js` uses native `window.confirm()` instead of `OpenFangToast.confirm()`

## Decisions Made

| # | Decision | Choice | Rationale |
|---|----------|--------|-----------|
| 1 | Primary audience | Both operators and developers equally | Dashboard is the single control plane |
| 2 | Frontend framework | Keep Alpine.js + vanilla JS | Consistency, no build step, sufficient for the complexity |
| 3 | API migration strategy | Migrate all pages to `/api/v1/` | One unified API layer, avoid maintaining two surfaces |
| 4 | Navigation restructure | Workflow-centric groups | Groups by user workflow: Operations, Workspace, Resources, Outputs, Monitor, System |
| 5 | Real-time strategy | SSE everywhere it exists | Shared SSE client utility, 6 SSE endpoints wired, polling only where no SSE |
| 6 | Implementation depth | Full CRUD for all domains | Every domain gets complete create, read, update, delete, plus all actions |
| 7 | Bug fix timing | Fix all 3 bugs first | Quick prerequisite PR before the overhaul |
| 8 | Build order | Infra -> HITL -> Runs -> Tasks -> Workflows -> rest | Shared infra first, then by operational criticality |
| 9 | Arky provider UI | Dedicated Phase 4.5 | Provider profiles, driver-specific config, reasoning effort — too complex for agents rebuild |

See ADR-001 through ADR-009 in `tasks/prd-ui/` for detailed rationale on each decision.

---

## Architecture

### Shared Infrastructure (Phase 0)

#### API Client v1

Extend the existing `OpenFangAPI` singleton in `api.js` with a `v1` namespace:

```js
OpenFangAPI.v1.runs.list({ status: 'running', workflow_id: '...' })
OpenFangAPI.v1.runs.get(id)
OpenFangAPI.v1.runs.pause(id)
OpenFangAPI.v1.hitl.list({ status: 'pending' })
OpenFangAPI.v1.hitl.answer(id, { answer: '...' })
// ... etc for each domain
```

Features:
- Typed wrappers for all `/api/v1/` endpoints
- Pagination helpers (`limit`, `offset`)
- Standard error envelope parsing (`{error: {code, message, details}}`)
- Reuses existing `OpenFangAPI.get/post/put/del` as transport

#### SSE Client Utility

New `OpenFangSSE` module in `js/sse.js`:

```js
OpenFangSSE.connect('/api/v1/hitl-requests/stream?status=pending', {
  'hitl.created': (data) => { /* new request */ },
  'hitl.answered': (data) => { /* request answered */ },
  'keepalive': () => { /* ignore */ }
}, {
  reconnect: true,         // auto-reconnect with backoff
  lastEventId: true,       // resume from last event
  token: OpenFangAPI.getToken()
})
```

Features:
- `connect(endpoint, handlers, options)` — creates `EventSource` with `?token=` auth
- Auto-reconnect with exponential backoff: `min(1000 * 2^n, 10000)` ms
- `Last-Event-ID` support for seamless reconnection
- Event type routing per stream
- Connection state tracking integrated with `$store.app.connectionState`
- Global HITL SSE connection in `app.js` driving nav badge count

#### Shared Utilities

New `js/utils.js` extracting duplicated logic:

- `timeAgo(date)` — single implementation replacing 5+ per-page copies
- `formatDate(date)` / `formatDateTime(date)` — consistent formatting
- `statusBadge(status)` — maps status strings to CSS badge classes
- `confirmAction(title, message, onConfirm)` — wraps `OpenFangToast.confirm()`

#### Navigation Restructure

```
Chat            -> agents (existing, inline chat)
--- Operations ---
  HITL Inbox    -> hitl        [NEW] badge with pending count via SSE
  Runs          -> runs        [NEW]
  Dispatches    -> dispatches  [NEW]
  Looper        -> looper      [NEW]
--- Workspace ---
  Tasks         -> tasks       [NEW]
  Workflows     -> workflows   [REBUILT on v1]
  Triggers      -> triggers    [REBUILT on v1]
  Schedules     -> schedules   [REBUILT on v1]
--- Resources ---
  Agents        -> agents      [REBUILT on v1]
  Skills        -> skills      [existing + v1 enrichment]
  Hands         -> hands       [existing]
  Packs         -> packs       [NEW]
--- Outputs ---
  Artifacts     -> artifacts   [NEW]
  Documents     -> documents   [NEW]
--- Monitor ---
  Overview      -> overview    [existing, updated]
  Analytics     -> analytics   [existing + budget]
  Logs          -> logs        [existing]
--- System ---
  Channels      -> channels    [existing]
  Integrations  -> integrations [NEW]
  Settings      -> settings    [existing]
  Runtime       -> runtime     [existing]
```

---

## Feature Domains

### Phase 0: Bug Fixes

1. Register trigger legacy routes in `server.rs` (`GET /api/triggers`, `PUT/DELETE /api/triggers/{id}`)
2. Fix `comms.js` SSE to use new `OpenFangSSE` utility instead of non-existent properties
3. Replace `window.confirm()` with `OpenFangToast.confirm()` in `workflows.js`

### Phase 1: HITL Inbox + Runs + Dispatches (Critical Operations)

#### HITL Inbox (`pages/hitl.js`)

- Global SSE to `/api/v1/hitl-requests/stream?status=pending`
- Request cards: question text, kind badge (clarification/approval/choice/freeform), run link, dispatch link, time waiting
- Inline answer form: text for freeform, approve/reject for approval, buttons for choice
- Submit: `POST /api/v1/hitl-requests/{id}/answer`
- Cancel: `POST /api/v1/hitl-requests/{id}/cancel`
- Filter: pending / answered / cancelled / timed_out
- Nav badge driven by SSE stream count in `$store.app`
- Resilient to backend restarts (post-restart HITL reconstruction is transparent)

#### Runs (`pages/runs.js`)

- List: `GET /api/v1/runs` with status/workflow filters
- 8 status badges: pending, running, waiting_signal, waiting_hitl, paused, completed, failed, cancelled
- Run detail with tabs:
  - Overview: status, workflow, timestamps, current step, progress
  - Checkpoints: timeline from `GET /api/v1/runs/{id}/checkpoints`
  - Signals: list + send form (`POST /api/v1/runs/{id}/signals`)
  - Dispatches: table from `GET /api/v1/runs/{id}/dispatches` with retry/cancel
  - HITL: scoped requests from `GET /api/v1/runs/{id}/hitl-requests`
  - Events: live SSE from `GET /api/v1/runs/{id}/events`
- Actions: Pause / Resume / Cancel (conditionally enabled per status)
- Recovery indicator: tooltip on paused runs with `run_recovered_needs_resume`

#### Dispatches (`pages/dispatches.js`)

- List: `GET /api/v1/dispatches` with status/kind filters
- Detail: kind (call/send/spawn), status, target agent, attempt count, parent/child tree
- Actions: retry, cancel
- SSE from `GET /api/v1/dispatches/{id}/events`

### Phase 2: Tasks + Subtasks (Core Domain)

#### Tasks (`pages/tasks.js`)

- List: `GET /api/v1/tasks` with status/priority filters, search
- Columns: title, status, priority, owner, complexity, created
- Task detail with tabs:
  - Subtasks: nested list with ready/blocked badges, dependency indicators
  - Artifacts: linked from `GET /api/v1/tasks/{id}/artifacts`
  - Docs: linked from `GET /api/v1/tasks/{id}/docs`
  - Files: from `GET /api/v1/tasks/{id}/files`
- Create/edit forms for tasks and subtasks
- Replan modal: `POST /api/v1/tasks/{id}/replan` with operation builder
- Subtask inline status progression

### Phase 3: Workflows v2 + Triggers v2 + Schedules v2 + Events (Authoring)

#### Workflows v2 (rebuild `pages/workflows.js`)

- Migrate from `/api/workflows` to `/api/v1/workflows`
- List: steps count, enabled toggle, runtime status, origin badge
- Editor: all 8 step kinds, flow mode picker, save_as bindings, input/output contracts
- Validate: `POST /api/v1/workflows/validate` with inline issue display
- Compile: `POST /api/v1/workflows/compile` with IR viewer
- Fork, run trigger form, dry-run preview
- Visual Builder updated for v2 step types

#### Triggers v2 (new `pages/triggers.js`)

- Migrate from broken `/api/triggers` to `/api/v1/triggers`
- Full CRUD with match fields and target kind selector
- Enable/disable toggle, runtime status column
- Validate/compile actions
- Test panel: JSON editor -> `POST /api/v1/triggers/{id}/test` -> match result display
- Fork action for pack-managed triggers

#### Schedules v2 (new `pages/schedules.js`)

- Migrate from `/api/cron/jobs` to `/api/v1/schedules`
- Typed cron editor with timezone, action kind selector
- Enable/disable, run-now, dry-run preview
- Runtime status, validate/fork actions

#### Events (new `pages/events.js`)

- Event ingress tester panel
- JSON editor for payload
- Dry-run: `POST /api/v1/events/dry-run` -> matched triggers + effects
- Send: `POST /api/v1/events` -> result with triggered run links

### Phase 4: Agents v1 Migration

#### Agents (rebuild `pages/agents.js`)

- Migrate from `/api/agents` to `/api/v1/agents`
- List: enabled, group, tags, origin, runtime_status columns
- Structured `AgentDefinition` form (not raw TOML)
- Validate/compile with per-field issue display
- Runtime panel: state, mode, health, start/stop/restart, mode selector
- Sessions tab: list with activate/reset/compact
- Skills and MCP servers assignment
- Provider profiles dropdown

#### Chat (update `pages/chat.js`)

- SSE streaming option via `POST /api/v1/agents/{id}/messages/stream`
- Dry-run mode toggle
- Keep WebSocket as primary for bidirectional

### Phase 4.5: Arky Provider System

**Backend prerequisite**: New `GET/POST/PUT/DELETE /api/v1/provider-profiles` CRUD endpoint must be created before this phase. Also fix `ValidationContext.known_profiles` (currently always empty — latent correctness bug).

The Arky provider subsystem (10 crates) introduces a layered provider SDK with 10 drivers, 3-tier config merging, and driver-specific typed configs. None of this is currently exposed in the UI.

#### Provider Profiles (new section in Settings or standalone page)

- List profiles with driver, model, defaults (max_tokens, reasoning_effort)
- Create/edit form with driver selector -> typed config fields per driver
- Delete with confirmation
- Used as reference when editing agent provider config (profile picker)

#### Spawn Wizard Upgrade

- Replace model-catalog provider dropdown with Arky driver selector:
  `codex`, `claude-code`, `openrouter`, `bedrock`, `vertex`, `ollama`, `zai`, `vercel`, `moonshot`, `minimax`
- Driver-specific config fields shown based on selected driver:
  - `codex`: sandbox_mode, web_search, reasoning_summary
  - `claude-code`: allowed_tools, disallowed_tools, mcp_servers, max_budget_usd, fallback_model
  - `claude_compatible` (bedrock/vertex/etc): selected_model, region, project_id
- Profile picker from provider profiles
- Reasoning effort selector (None/Low/Medium/High/XHigh)
- max_tokens override field

#### Agent Detail: Provider Config Section

- Show resolved driver, model, profile reference
- Display/edit driver-specific config
- Display defaults (reasoning_effort, max_tokens)
- "View Compiled Binding" expandable: calls `GET /api/v1/agents/{id}/compiled` to show the resolved `ProviderBinding` (driver, provider_id, model ref, resolved config)

#### Inline MCP Distinction

- Differentiate global MCP servers (from kernel config, shown in Skills page) from inline MCP servers (from `provider.config.claude_code.mcp_servers`)
- Edit inline MCP servers in the provider config section of agent detail

### Phase 5: Looper, Packs, Artifacts, Docs

#### Looper (`pages/looper.js`)

- List: `GET /api/v1/looper-runs` with status/mode filters
- Detail: progress bar, execution mode, subtask grid
- SSE from `GET /api/v1/looper-runs/{id}/events` for live progress
- Actions: pause/resume/cancel
- Create form: task selector, execution policy config

#### Packs (`pages/packs.js`)

- List: `GET /api/v1/packs` with source badges
- Detail: manifest, managed objects, forked indicators
- Install form, upgrade with dry-run preview modal, uninstall with fork-warning
- Fork per-object

#### Artifacts (`pages/artifacts.js`)

- List: `GET /api/v1/artifacts` with type/task filters
- Detail: current version content, metadata
- Version history: version_no, SHA-256, provenance, timestamps

#### Documents (`pages/documents.js`)

- Same pattern as Artifacts but with Markdown rendering for doc body
- List: `GET /api/v1/docs` with task filter

### Phase 6: Remaining Features

#### Budget (expand analytics or settings)

- `GET/PUT /api/budget` for global limits
- `GET /api/budget/agents`, `GET/PUT /api/budget/agents/{id}` for per-agent

#### Integrations (`pages/integrations.js`)

- List: `GET /api/integrations` + available
- Add/remove/reconnect, health status

#### A2A Management (enhance comms page)

- Discover: `POST /api/a2a/discover` with URL input
- Send task: `POST /api/a2a/send`
- Status tracking: `GET /api/a2a/tasks/{id}/status`

#### Comms (enhance existing)

- Wire SSE properly using `OpenFangSSE`
- Enhanced topology with live updates

#### Minor Updates

- Sessions: label support (`PUT /api/sessions/{id}/label`)
- Overview: real security data from `/api/security`, per-DB health, workflow readiness
- Wizard: v1 agent creation flow

---

## File Inventory

### New Files

| File | Domain | Phase |
|------|--------|-------|
| `js/sse.js` | Shared SSE client | 0 |
| `js/utils.js` | Shared utilities | 0 |
| `js/api-v1.js` | v1 API client | 0 |
| `js/pages/hitl.js` | HITL Inbox | 1 |
| `js/pages/runs.js` | Workflow Runs | 1 |
| `js/pages/dispatches.js` | Dispatches | 1 |
| `js/pages/tasks.js` | Tasks + Subtasks | 2 |
| `js/pages/triggers.js` | Triggers v2 | 3 |
| `js/pages/schedules.js` | Schedules v2 | 3 |
| `js/pages/events.js` | Event Ingress | 3 |
| `js/pages/looper.js` | Looper Runs | 5 |
| `js/pages/packs.js` | Pack Management | 5 |
| `js/pages/artifacts.js` | Artifacts | 5 |
| `js/pages/documents.js` | Documents | 5 |
| `js/pages/integrations.js` | Integrations | 6 |

### Modified Files

| File | Changes | Phase |
|------|---------|-------|
| `index_body.html` | New nav structure, new page templates | 0+ |
| `index_head.html` | New script tags for new JS files | 0+ |
| `js/app.js` | Nav groups, global HITL SSE, route aliases | 0 |
| `js/api.js` | Bug fix (comms SSE), minor enhancements | 0 |
| `css/components.css` | New badge variants, HITL card styles | 0+ |
| `js/pages/workflows.js` | Full rebuild on v1 API | 3 |
| `js/pages/scheduler.js` | Deprecated (split into triggers.js + schedules.js) | 3 |
| `js/pages/agents.js` | Full rebuild on v1 API | 4 |
| `js/pages/chat.js` | SSE streaming, dry-run mode | 4 |
| `js/pages/comms.js` | Bug fix + SSE wiring | 0+6 |
| `js/pages/skills.js` | v1 enrichment | 4 |
| `js/pages/usage.js` | Budget management | 6 |
| `js/pages/overview.js` | Real security data, DB health | 6 |
| `js/pages/settings.js` | Provider profiles section | 4 |

### Backend Changes

| File | Changes | Phase |
|------|---------|-------|
| `crates/openfang-api/src/server.rs` | Register legacy trigger routes | 0 |
| `crates/openfang-api/src/routes.rs` | New `/api/v1/provider-profiles` CRUD handlers | 4.5 (prerequisite) |
| `crates/openfang-api/src/server.rs` | Register provider-profiles routes | 4.5 (prerequisite) |
| `crates/openfang-api/src/routes.rs` | Fix `ValidationContext.known_profiles` seeding | 4.5 (prerequisite) |

---

## Endpoint Coverage

After full implementation, the UI will cover:

- **Phase 0**: ~90 existing + 3 bug fixes = ~90 endpoints
- **Phase 1**: +25 (runs, dispatches, HITL) = ~115 endpoints
- **Phase 2**: +14 (tasks, subtasks) = ~129 endpoints
- **Phase 3**: +35 (workflows v1, triggers v1, schedules v1, events) = ~164 endpoints
- **Phase 4**: +22 (agents v1, chat v1) = ~186 endpoints
- **Phase 5**: +22 (looper, packs, artifacts, docs) = ~208 endpoints
- **Phase 6**: +15 (budget, integrations, A2A, comms, misc) = ~223 endpoints

Remaining uncovered (~7): OpenAI-compat endpoints, MCP HTTP transport, shutdown, metrics, device pairing — intentionally excluded from dashboard UI.

---

## References

- Analysis files: `tasks/prd-ui/analysis_*.md` (6 files)
- ADRs: `tasks/prd-ui/adr-*.md` (8 files)
- Tech spec: `tasks/prd-ui/techspec.md`
- PRD-Compozy tasks: `tasks/prd-compozy/_task_1.md` through `_task_43.md`
- PRD-CLI tasks: `tasks/prd-cli/_task_1.md` through `_task_8.md`
