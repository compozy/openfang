## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>low</complexity>
<dependencies>task_1</dependencies>
</task_context>

# Task 17.0: Budget & Analytics Enhancement

## Overview

Add budget management to the analytics/usage page. The existing analytics page shows read-only usage stats. This task adds budget limit management (global and per-agent) using the existing budget API endpoints.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_api_routes.md` (Budget section)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms budget features work
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Budget tab in analytics page (or new section)
- Global budget: display and edit via `GET/PUT /api/budget`
- Per-agent budget ranking: `GET /api/budget/agents`
- Per-agent budget detail and edit: `GET/PUT /api/budget/agents/{id}`
- Budget limit form: hourly, daily, monthly limits
</requirements>

## Subtasks

- [ ] 17.1 Add Budget tab to analytics page in `usage.js`
- [ ] 17.2 Implement global budget display from `GET /api/budget`
- [ ] 17.3 Implement global budget edit form — hourly/daily/monthly limits via `PUT /api/budget`
- [ ] 17.4 Implement per-agent budget ranking table from `GET /api/budget/agents`
- [ ] 17.5 Implement per-agent budget detail and edit from `GET/PUT /api/budget/agents/{id}`
- [ ] 17.6 Update `index_body.html` analytics template for budget tab

## Implementation Details

### API Endpoints Used

- `GET /api/budget` — global budget status
- `PUT /api/budget` — update global limits
- `GET /api/budget/agents` — per-agent ranking
- `GET /api/budget/agents/{id}` — agent budget detail
- `PUT /api/budget/agents/{id}` — update agent limits

### Relevant Files

- `crates/openfang-api/static/js/pages/usage.js` (MODIFY)
- `crates/openfang-api/static/index_body.html` (MODIFY)

## Deliverables

- Budget tab with global and per-agent management
- Budget limit edit forms

## Tests

### Manual Browser Tests (Required)

- [ ] Budget tab displays global budget status
- [ ] Edit global limits — verify changes persist
- [ ] Per-agent ranking table displays
- [ ] Edit per-agent limits — verify changes persist

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Budget management accessible from analytics page
- Global and per-agent limits editable
