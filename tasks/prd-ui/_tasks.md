# PRD-UI: OpenFang Dashboard UI Integration

> Total tasks: 18
> Phases: 0 (infra) → 1 (critical ops) → 2 (core domain) → 3 (authoring) → 4 (agents) → 4.5 (providers) → 5 (supporting) → 6 (polish)
> Design doc: `docs/plans/2026-03-27-ui-integration-design.md`
> Tech spec: `tasks/prd-ui/techspec.md`
> ADRs: `tasks/prd-ui/adr-001.md` through `adr-009.md`

---

## Phase 0: Infrastructure + Bug Fixes

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 1 | Shared Infrastructure (SSE client, API v1 client, utils) | high | none | pending |
| 2 | Bug Fixes + Navigation Restructure | medium | none | pending |

## Phase 1: HITL + Runs + Dispatches (Critical Operations)

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 3 | HITL Inbox | high | 1, 2 | pending |
| 4 | Workflow Runs | high | 1, 2 | pending |
| 5 | Dispatches | medium | 1, 2 | pending |

## Phase 2: Tasks + Subtasks (Core Domain)

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 6 | Tasks & Subtasks | high | 1, 2 | pending |

## Phase 3: Workflows v2 + Triggers v2 + Schedules v2 + Events (Authoring)

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 7 | Workflows v2 (CRUD + Editor + Visual Builder) | critical | 1, 2 | pending |
| 8 | Triggers v2 | high | 1, 2 | pending |
| 9 | Schedules v2 + Event Ingress | medium | 1, 2 | pending |

## Phase 4: Agents v1 Migration

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 10 | Agents v1 Migration | critical | 1, 2 | pending |
| 11 | Chat SSE Streaming + Dry-Run | medium | 1, 10 | pending |

## Phase 4.5: Arky Provider System

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 12 | Arky Provider Backend (profiles CRUD endpoint) | high | none (Rust) | pending |
| 13 | Arky Provider UI (profiles, driver config, spawn wizard) | high | 1, 10, 12 | pending |

## Phase 5: Looper + Packs + Artifacts + Docs

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 14 | Looper Runs | medium | 1, 2, 6 | pending |
| 15 | Packs | medium | 1, 2 | pending |
| 16 | Artifacts & Documents | medium | 1, 2 | pending |

## Phase 6: Budget + Integrations + Remaining

| Task | Title | Complexity | Dependencies | Status |
|------|-------|------------|--------------|--------|
| 17 | Budget & Analytics Enhancement | low | 1 | pending |
| 18 | Integrations + A2A + Comms + Minor Updates | medium | 1, 2 | pending |

---

## Parallelism Notes

- Tasks 1 and 2 can run in parallel (no shared files except `index_head.html`)
- Task 12 (Rust backend) can run in parallel with any JS task
- Within each phase, tasks can run in parallel if dependencies are met
- Tasks 3, 4, 5 (Phase 1) can all run in parallel after 1+2 complete
- Tasks 7, 8, 9 (Phase 3) can all run in parallel after 1+2 complete
- Tasks 14, 15, 16 (Phase 5) can all run in parallel after 1+2 complete
- Tasks 17, 18 (Phase 6) can run in parallel after 1 completes

## File Inventory

### New Files (15 JS)

| File | Task |
|------|------|
| `js/sse.js` | 1 |
| `js/api-v1.js` | 1 |
| `js/utils.js` | 1 |
| `js/pages/hitl.js` | 3 |
| `js/pages/runs.js` | 4 |
| `js/pages/dispatches.js` | 5 |
| `js/pages/tasks.js` | 6 |
| `js/pages/triggers.js` | 8 |
| `js/pages/schedules.js` | 9 |
| `js/pages/events.js` | 9 |
| `js/pages/looper.js` | 14 |
| `js/pages/packs.js` | 15 |
| `js/pages/artifacts.js` | 16 |
| `js/pages/documents.js` | 16 |
| `js/pages/integrations.js` | 18 |

### Rebuilt Files (3 JS)

| File | Task |
|------|------|
| `js/pages/workflows.js` | 7 |
| `js/pages/agents.js` | 10 |
| `js/pages/scheduler.js` → deprecated | 9 |

### Modified Files (10+ JS/HTML/CSS)

| File | Tasks |
|------|-------|
| `index_body.html` | 2, 3-18 |
| `index_head.html` | 1 |
| `js/app.js` | 2, 3 |
| `js/api.js` | 2 |
| `js/pages/chat.js` | 11 |
| `js/pages/comms.js` | 2, 18 |
| `js/pages/usage.js` | 17 |
| `js/pages/overview.js` | 18 |
| `js/pages/sessions.js` | 18 |
| `js/pages/settings.js` | 13, 18 |
| `js/pages/wizard.js` | 18 |
| `css/components.css` | 2-18 |

### Backend Files (Rust)

| File | Task |
|------|------|
| `crates/openfang-api/src/server.rs` | 2, 12 |
| `crates/openfang-api/src/routes.rs` | 12 |
| `crates/openfang-api/tests/api_integration_test.rs` | 12 |
