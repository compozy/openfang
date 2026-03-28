# ADR-006: Full CRUD for All Domains

## Status

Accepted

## Date

2026-03-27

## Context

The UI integration covers ~12 new feature domains. Each domain could be built at different depths: read-only, read + actions, or full CRUD with validate/compile/dry-run. The backend already exposes full CRUD for every domain.

## Decision

Every domain gets complete CRUD plus all available actions (validate, compile, fork, dry-run, enable/disable, test). No domain is built read-only.

## Alternatives Considered

### Alternative 1: Tiered Depth

- **Description**: Critical domains get full CRUD, medium get CRUD, low get read-only.
- **Pros**: Faster delivery of read-only views, scope control
- **Cons**: Read-only views feel incomplete, users must use CLI for writes
- **Why rejected**: User chose the dashboard as the single control plane. Read-only views undermine that.

### Alternative 2: Read-Only MVP, Then Iterate

- **Description**: First pass is read-only everywhere. Write operations added in later passes.
- **Pros**: Smallest initial scope
- **Cons**: Multiple implementation passes over the same pages, users frustrated by can-see-but-can't-act
- **Why rejected**: Creates unnecessary rework touching each page multiple times.

## Consequences

### Positive

- Dashboard is immediately useful for all operations
- No "go to CLI for this" escape hatches
- Each page is complete when shipped

### Negative

- Larger total scope (more forms, modals, validation)
- Longer implementation timeline
- More edge cases in write paths (error handling, optimistic UI)

### Risks

- Scope creep: "full CRUD" could expand to cover every possible action. Mitigation: scope is bounded by what the v1 API actually exposes. No features beyond existing endpoints.

## References

- `tasks/prd-ui/analysis_api_routes.md` — complete endpoint inventory defining the scope ceiling
