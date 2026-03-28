## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 18.0: Integrations + A2A + Comms + Minor Updates

## Overview

Final polish phase covering: new Integrations page, A2A management in Comms, enhanced Comms SSE, and minor updates to Overview, Settings, Sessions, and Wizard pages.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_api_routes.md`
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms all features work
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Integrations page: list, add, remove, reconnect, health from `/api/integrations`
- A2A management: discover (`POST /api/a2a/discover`), send task, status tracking in Comms page
- Comms: wire SSE properly using `OpenFangSSE` for live topology updates
- Overview: replace hardcoded security panel with data from `/api/security`, add per-DB health
- Settings: provider profiles section link (from task 13)
- Sessions: add label support (`PUT /api/sessions/{id}/label`)
- Wizard: update to use v1 agent creation flow
</requirements>

## Subtasks

- [ ] 18.1 Create `js/pages/integrations.js` — `integrationsPage()` Alpine component
- [ ] 18.2 Implement integration list — `GET /api/integrations`, available types from `GET /api/integrations/available`
- [ ] 18.3 Implement add/remove/reconnect actions
- [ ] 18.4 Implement health display from `GET /api/integrations/health`
- [ ] 18.5 Add A2A section to Comms page — discover form (`POST /api/a2a/discover` with URL input)
- [ ] 18.6 Implement A2A send task — `POST /api/a2a/send` with agent selector + payload
- [ ] 18.7 Implement A2A task status tracking — `GET /api/a2a/tasks/{id}/status`
- [ ] 18.8 Wire Comms SSE using `OpenFangSSE` for live event feed (replacing broken SSE)
- [ ] 18.9 Update Overview — replace hardcoded security with `/api/security` data
- [ ] 18.10 Update Overview — add per-database health indicators
- [ ] 18.11 Update Overview — add workflow registry readiness indicator
- [ ] 18.12 Update Sessions — add label support (`PUT /api/sessions/{id}/label`)
- [ ] 18.13 Update Wizard — use v1 agent creation flow
- [ ] 18.14 Add Integrations page template in `index_body.html`

## Implementation Details

### API Endpoints Used

Integrations:
- `GET /api/integrations`, `GET /api/integrations/available`, `POST /api/integrations/add`
- `DELETE /api/integrations/{id}`, `POST /api/integrations/{id}/reconnect`, `GET /api/integrations/health`

A2A:
- `POST /api/a2a/discover`, `POST /api/a2a/send`, `GET /api/a2a/tasks/{id}/status`

Comms: `GET /api/comms/events/stream` (SSE)
Sessions: `PUT /api/sessions/{id}/label`
Security: `GET /api/security`

### Relevant Files

- `crates/openfang-api/static/js/pages/integrations.js` (NEW)
- `crates/openfang-api/static/js/pages/comms.js` (MODIFY)
- `crates/openfang-api/static/js/pages/overview.js` (MODIFY)
- `crates/openfang-api/static/js/pages/sessions.js` (MODIFY)
- `crates/openfang-api/static/js/pages/settings.js` (MODIFY)
- `crates/openfang-api/static/js/pages/wizard.js` (MODIFY)
- `crates/openfang-api/static/index_body.html` (MODIFY)

## Deliverables

- `js/pages/integrations.js` with CRUD and health
- A2A management in Comms page
- All minor updates applied
- All existing pages enhanced

## Tests

### Manual Browser Tests (Required)

- [ ] Integrations page — list, add, remove, reconnect, health
- [ ] A2A in Comms — discover agent at URL, send task, check status
- [ ] Comms SSE — verify live event feed works
- [ ] Overview — verify security panel shows real data (not hardcoded)
- [ ] Overview — verify per-DB health indicators
- [ ] Sessions — verify label add/edit works
- [ ] Wizard — verify agent creation uses v1 flow

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- All remaining feature gaps closed
- Integrations page functional
- A2A management accessible
- All minor updates applied
- No regressions anywhere
