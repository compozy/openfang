## status: completed

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 15.0: Packs Page

## Overview

Build the Packs page — pack management with install, upgrade (dry-run preview), uninstall, fork, and managed objects inspection.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (tasks 40, 41)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms page works
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- List packs from `GET /api/v1/packs` with source badges (bundled/external)
- Pack detail: manifest, managed objects list with forked indicators
- Objects tab: `GET /api/v1/packs/{id}/objects` with resource type/forked status
- Install form: source kind selector, pack ID, version
- Upgrade with dry-run preview modal: shows added/updated/removed/forks_untouched before commit
- Uninstall with fork-warning dialog
- Fork per-object
</requirements>

## Subtasks

- [x] 15.1 Create `js/pages/packs.js` — `packsPage()` Alpine component
- [x] 15.2 Implement pack list — source badges, object count summary
- [x] 15.3 Implement pack detail — manifest display, metadata
- [x] 15.4 Implement objects tab — resource type, resource ID, forked status
- [x] 15.5 Implement install form — source kind selector, pack ID, version fields
- [x] 15.6 Implement upgrade with dry-run preview — call dry-run first, show diff modal, confirm to commit
- [x] 15.7 Implement uninstall with fork-warning dialog
- [x] 15.8 Implement fork per-object — `POST /api/v1/packs/{id}/fork`
- [x] 15.9 Add Packs page template in `index_body.html`

## Implementation Details

### Upgrade Dry-Run Flow

1. User clicks "Upgrade" on pack detail
2. UI calls `POST /api/v1/packs/{id}/upgrade/dry-run`
3. Modal shows: `managed_objects_added`, `managed_objects_updated`, `managed_objects_removed`, `forks_untouched`
4. User reviews diff and clicks "Confirm Upgrade"
5. UI calls `POST /api/v1/packs/{id}/upgrade`

### API Endpoints Used

All 8 endpoints under `OpenFangAPI.v1.packs.*` from the techspec.

### Relevant Files

- `crates/openfang-api/static/js/pages/packs.js` (NEW)
- `crates/openfang-api/static/index_body.html` (MODIFY)
- `crates/openfang-api/static/css/components.css` (MODIFY)

## Deliverables

- `js/pages/packs.js` with list, detail, objects, install, upgrade, uninstall, fork
- Packs page template in HTML
- Upgrade dry-run preview modal

## Tests

### Manual Browser Tests (Required)

- [ ] Pack list — verify source badges and counts
- [ ] Pack detail — verify manifest and objects display
- [ ] Install pack — verify form and installation
- [ ] Upgrade — verify dry-run preview modal, then commit
- [ ] Uninstall — verify fork-warning dialog
- [ ] Fork object — verify fork creates user-owned copy

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test`

## Success Criteria

- Packs page with full lifecycle management
- Upgrade dry-run preview shows accurate diff before commit
- Fork-warning displayed on uninstall when forks exist
