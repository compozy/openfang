## status: pending

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
</critical>

<requirements>
- Artifacts page: list from `GET /api/v1/artifacts` with type/task filters, search
- Artifacts detail: current version content viewer, metadata
- Artifacts version history: version_no, content_hash (SHA-256), created_by provenance, timestamps
- Documents page: same pattern with Markdown rendering for doc body
- Provenance links: click `created_by.ref` to navigate to dispatch/run detail
</requirements>

## Subtasks

- [ ] 16.1 Create `js/pages/artifacts.js` — `artifactsPage()` Alpine component
- [ ] 16.2 Implement artifact list — type/task filters, search, columns
- [ ] 16.3 Implement artifact detail — current version content display
- [ ] 16.4 Implement artifact version history — version_no, SHA-256, provenance, timestamps
- [ ] 16.5 Implement provenance links — navigate to dispatch or run detail
- [ ] 16.6 Create `js/pages/documents.js` — `documentsPage()` Alpine component
- [ ] 16.7 Implement document list — task filter, search
- [ ] 16.8 Implement document detail — Markdown rendering for body using `renderMarkdown()`
- [ ] 16.9 Implement document version history — same pattern as artifacts
- [ ] 16.10 Add Artifacts and Documents page templates in `index_body.html`

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

- [ ] Artifacts page — list, filter by type, search
- [ ] Artifact detail — version content display
- [ ] Artifact version history — verify SHA-256, provenance links work
- [ ] Documents page — list, filter, search
- [ ] Document detail — Markdown rendering
- [ ] Provenance link — click to navigate to dispatch/run

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Both pages functional with list, detail, version history
- Markdown rendering works for documents
- Provenance links navigate correctly
