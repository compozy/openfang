## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 9.0: Schedules v2 + Event Ingress

## Overview

Build two related pages: Schedules v2 (replacing the cron tab in the old scheduler) using `/api/v1/schedules`, and an Event Ingress tester page for sending events through the trigger match engine.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_16_30.md` (task 26) and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (task 36)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms both pages work
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Schedules: migrate from `/api/cron/jobs` to `/api/v1/schedules` with full CRUD
- Schedules: typed cron editor, timezone, action kind selector, enable/disable, run-now, dry-run
- Schedules: runtime status (consecutive_errors, last_status, one_shot)
- Events: JSON editor for event payload, dry-run and send buttons
- Events: result display showing matched triggers and effects
- Deprecate old `scheduler.js` (split into `triggers.js` from task 8 + `schedules.js`)
</requirements>

## Subtasks

- [ ] 9.1 Create `js/pages/schedules.js` — `schedulesPage()` Alpine component
- [ ] 9.2 Implement schedule list — cron expression display, action kind, enabled toggle, last/next run
- [ ] 9.3 Implement schedule create/edit form — typed cron fields, timezone, action kind selector
- [ ] 9.4 Implement enable/disable toggle
- [ ] 9.5 Implement run-now button
- [ ] 9.6 Implement dry-run preview — `POST /api/v1/schedules/{id}/run-now/dry-run`
- [ ] 9.7 Implement validate/fork actions
- [ ] 9.8 Implement runtime status section
- [ ] 9.9 Create `js/pages/events.js` — `eventsPage()` Alpine component
- [ ] 9.10 Implement event ingress form — JSON editor for event, source, payload, idempotency_key
- [ ] 9.11 Implement dry-run button — `POST /api/v1/events/dry-run` with matched triggers result
- [ ] 9.12 Implement send button — `POST /api/v1/events` with result and triggered run links
- [ ] 9.13 Deprecate `scheduler.js` — remove or mark as legacy, update imports
- [ ] 9.14 Add Schedules and Events page templates in `index_body.html`

## Implementation Details

### API Endpoints Used

Schedules: all 12 endpoints under `OpenFangAPI.v1.schedules.*`
Events: `OpenFangAPI.v1.events.send()` and `OpenFangAPI.v1.events.dryRun()`

### Relevant Files

- `crates/openfang-api/static/js/pages/schedules.js` (NEW)
- `crates/openfang-api/static/js/pages/events.js` (NEW)
- `crates/openfang-api/static/js/pages/scheduler.js` (DEPRECATE)
- `crates/openfang-api/static/index_body.html` (MODIFY)

## Deliverables

- `js/pages/schedules.js` with full CRUD, typed cron, enable/disable, run-now, dry-run
- `js/pages/events.js` with event tester panel
- Both page templates in HTML
- Old `scheduler.js` deprecated

## Tests

### Manual Browser Tests (Required)

- [ ] Schedules page — list, create, edit, delete, enable/disable, run-now
- [ ] Events page — compose event, dry-run, send, verify results
- [ ] Verify old scheduler page hash redirects to new pages

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Schedules v2 replaces legacy cron tab with full CRUD
- Event ingress tester works with dry-run and send
- Old scheduler.js deprecated
