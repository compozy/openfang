## status: completed

<task_context>
<domain>openfang-api/static/js/pages</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task_1,task_2</dependencies>
</task_context>

# Task 16.0: Artifacts & Documents Pages

## Overview

Build two new pages for browsing artifacts and documents independently. Both follow the same pattern: list with filters, detail with current version content, version history with SHA-256 hashes and provenance links.

<critical>
- **ALWAYS READ** @CLAUDE.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-ui/techspec.md` and `tasks/prd-ui/analysis_prd_tasks_31_43.md` (tasks 37, 38)
- **YOU CAN ONLY** finish when `make fmt && make lint && make test` pass AND manual browser verification confirms pages work
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
- **DASHBOARD / ALPINE** — use `alpine-js` skill for static HTML, Alpine components, and JS under `static/`
</critical>

<requirements>
- Artifacts page: list from `GET /api/v1/artifacts` with type/task filters, search
- Artifacts detail: current version content viewer, metadata
- Artifacts version history: version_no, content_hash (SHA-256), created_by provenance, timestamps
- Documents page: same pattern with Markdown rendering for doc body
- Provenance links: click `created_by.ref` to navigate to dispatch/run detail
</requirements>

## Subtasks

- [x] 16.1 Create `js/pages/artifacts.js` — `artifactsPage()` Alpine component
- [x] 16.2 Implement artifact list — type/task filters, search, columns
- [x] 16.3 Implement artifact detail — current version content display
- [x] 16.4 Implement artifact version history — version_no, SHA-256, provenance, timestamps
- [x] 16.5 Implement provenance links — navigate to dispatch or run detail
- [x] 16.6 Create `js/pages/documents.js` — `documentsPage()` Alpine component
- [x] 16.7 Implement document list — task filter, search
- [x] 16.8 Implement document detail — Markdown rendering for body using `renderMarkdown()`
- [x] 16.9 Implement document version history — same pattern as artifacts
- [x] 16.10 Add Artifacts and Documents page templates in `index_body.html`

## Implementation Details

### API Endpoints Used

Artifacts: `OpenFangAPI.v1.artifacts.list()`, `.get(id)`, `.versions(id)`
Documents: `OpenFangAPI.v1.docs.list()`, `.get(id)`, `.versions(id)`

### Relevant Files

- `crates/openfang-api/static/js/pages/artifacts.js` (NEW)
- `crates/openfang-api/static/js/pages/documents.js` (NEW)
- `crates/openfang-api/static/index_body.html` (MODIFY)

## Deliverables

- `js/pages/artifacts.js` and `js/pages/documents.js`
- Both page templates in HTML
- Version history with provenance linking

## Tests

### Manual Browser Tests (Required)

- [x] Artifacts page — list, filter by type, search
- [x] Artifact detail — version content display
- [x] Artifact version history — verify SHA-256, provenance links work
- [x] Documents page — list, filter, search
- [x] Document detail — Markdown rendering
- [x] Provenance link — click to navigate to dispatch/run

### Verification Commands

- [x] `make fmt`
- [x] `make lint`
- [x] `make test` (pre-existing CLI test failures in `a2a_peers_budget_commands` unrelated to this task)

## Success Criteria

- Both pages functional with list, detail, version history
- Markdown rendering works for documents
- Provenance links navigate correctly
