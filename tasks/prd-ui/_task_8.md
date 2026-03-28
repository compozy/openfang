## status: pending

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 8.0: Triggers v2 Page

## Overview

Build a new Triggers v2 page replacing the broken trigger tab in the old scheduler page. Uses `/api/v1/triggers` with full CRUD, match/target editor, enable/disable toggle, validate/compile, runtime status, trigger test panel, and fork action.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (tasks 35, 36)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Full CRUD from `/api/v1/triggers`
- Trigger create/edit with match fields (event, source, contains, filters) and target kind selector (agent_message, workflow_start, workflow_signal)
- Enable/disable toggle per trigger
- Runtime status: fire_count, last_fired_at
- Validate/compile actions
- Test panel: JSON editor for synthetic event -> test endpoint -> match result display
- Fork for pack-managed triggers
</requirements>

## Subtasks

- [ ] 8.1 Create `js/pages/triggers.js` — `triggersPage()` Alpine component
- [ ] 8.2 Implement trigger list — columns: name, event match, target, enabled toggle, fire_count, last_fired_at
- [ ] 8.3 Implement trigger create form — match fields + target kind selector with dynamic fields
- [ ] 8.4 Implement trigger edit form
- [ ] 8.5 Implement trigger delete with confirmation
- [ ] 8.6 Implement enable/disable toggle — `POST .../enable` / `POST .../disable`
- [ ] 8.7 Implement validate action — `POST /api/v1/triggers/validate` with inline issues
- [ ] 8.8 Implement compile action — `POST /api/v1/triggers/compile`
- [ ] 8.9 Implement test panel — JSON editor, "Run Test" button calling `POST /api/v1/triggers/{id}/test`, result display (matched, resolved_target, would_dispatch, explanation)
- [ ] 8.10 Implement runtime status view from `GET /api/v1/triggers/{id}/runtime`
- [ ] 8.11 Implement fork action for pack-managed triggers
- [ ] 8.12 Add Triggers page template in `index_body.html`

## Implementation Details

### API Endpoints Used

All 13 endpoints under `OpenFangAPI.v1.triggers.*` from the techspec.

### Relevant Files

- `crates/openfang-api/static/js/pages/triggers.js` (NEW)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- `js/pages/triggers.js` with full CRUD, test panel, validate/compile, fork
- Triggers page template in HTML

## Tests

### Manual Browser Tests (Required)

- [ ] List triggers — verify columns display correctly
- [ ] Create trigger — verify match/target fields, trigger appears in list
- [ ] Enable/disable toggle — verify state change
- [ ] Test panel — enter event JSON, run test, verify match result display
- [ ] Validate trigger — verify issue display
- [ ] Delete trigger — verify confirmation and removal

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Triggers page replaces broken scheduler trigger tab
- Full CRUD + test panel + validate/compile functional
- Enable/disable toggle works
- Runtime status shows fire counts
