# ADR-003: Migrate All Pages to /api/v1/ Endpoints

## Status

Accepted

## Date

2026-03-27

## Context

The existing UI uses ~90 legacy `/api/` endpoints. The backend also exposes ~140 `/api/v1/` endpoints with richer data models, structured validation, and compile capabilities. The v1 endpoints are a superset of the legacy ones for most domains. Maintaining two API surfaces in the UI creates inconsistency.

## Decision

Migrate all existing pages to `/api/v1/` endpoints. New pages use v1 from the start. The UI will have one unified API layer.

## Alternatives Considered

### Alternative 1: v1 for New Pages Only

- **Description**: Leave existing pages on legacy `/api/`. Only new pages use `/api/v1/`.
- **Pros**: No risk to existing pages, faster initial delivery
- **Cons**: Two API patterns in the codebase, inconsistent data models, can't share components across old/new pages
- **Why rejected**: Creates long-term maintenance burden and prevents feature sharing (e.g., agent selector component can't work with both API versions).

### Alternative 2: Full v1 Migration First

- **Description**: Migrate all existing pages before building any new ones.
- **Pros**: Clean foundation before new work
- **Cons**: Delays all new feature work (HITL, runs, tasks) by weeks
- **Why rejected**: HITL inbox is operationally critical and should not wait for migration of less urgent pages.

## Consequences

### Positive

- One API client, one data model, shared components
- Existing pages gain v1 features (validate, compile, runtime status) during migration
- Cleaner codebase for future contributors

### Negative

- More work per phase: each rebuilt page must be fully tested
- Risk of regression in existing pages during migration
- Legacy endpoints stay in the backend but are unused by UI

### Risks

- Migration may surface behavioral differences between legacy and v1 endpoints. Mitigation: migrate one page at a time, test thoroughly before moving to the next.

## Implementation Notes

- Phase 0: shared infra (API client v1) provides the foundation
- Phases 1-2: new pages use v1 from scratch
- Phases 3-4: existing pages rebuilt on v1 (workflows, triggers, schedules, agents)
- The legacy API client methods in `api.js` remain for any edge cases but are no longer the primary path

## References

- `tasks/prd-ui/analysis_api_routes.md` — full endpoint inventory with UI coverage mapping
