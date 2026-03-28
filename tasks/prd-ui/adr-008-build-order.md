# ADR-008: Build Order — Infra, HITL, Runs, Tasks, Workflows, Rest

## Status

Accepted

## Date

2026-03-27

## Context

~12 new feature domains need to be built. The build order determines which features are available first and which can benefit from shared infrastructure.

## Decision

Build in this order:
1. **Phase 0**: Shared infrastructure (SSE client, API v1 client, utils) + bug fixes
2. **Phase 1**: HITL Inbox + Runs + Dispatches (critical operations)
3. **Phase 2**: Tasks + Subtasks (core domain)
4. **Phase 3**: Workflows v2 + Triggers v2 + Schedules v2 + Events (authoring)
5. **Phase 4**: Agents v1 migration + Chat updates
6. **Phase 5**: Looper, Packs, Artifacts, Documents
7. **Phase 6**: Budget, Integrations, A2A, Comms enhancements, minor updates

## Alternatives Considered

### Alternative 1: Agents First, Then Operations

- **Description**: Migrate the Agents page to v1 first since it is the biggest existing page.
- **Pros**: Validates v1 API client early on the most-used page
- **Cons**: Delays HITL and Runs which are operationally critical
- **Why rejected**: HITL requests block workflow execution. Every day without HITL UI is operational risk.

### Alternative 2: All New Pages First, Migrate Later

- **Description**: Build all new domains, migrate existing pages last.
- **Pros**: Ships new value faster
- **Cons**: Existing workflow/trigger/schedule pages stay on legacy, creating inconsistency
- **Why rejected**: Triggers are currently broken (404), so they need to be rebuilt. Workflows and schedules benefit from v1 features (validate, compile).

### Alternative 3: Domain Clusters

- **Description**: Build related features together (Workflows+Runs+Signals+HITL, then Tasks+Looper+Artifacts).
- **Pros**: Each cluster is self-contained
- **Cons**: Mixes infrastructure dependencies; cluster 1 is very large
- **Why rejected**: The chosen order achieves similar clustering but with better incremental value delivery.

## Consequences

### Positive

- HITL inbox available first (blocks production workflows)
- Shared infra (Phase 0) benefits all subsequent phases
- Each phase is independently shippable
- Critical ops (Phases 1-2) are available before authoring (Phase 3)

### Negative

- Agents page (most-used) is not rebuilt until Phase 4
- Full feature parity takes 7 phases

### Risks

- Phase 0 shared infra may need iteration as Phase 1 reveals gaps. Mitigation: keep Phase 0 minimal, extend as needed.

## References

- `tasks/prd-ui/analysis_cli_features.md` — prioritization section
- `tasks/prd-ui/analysis_prd_tasks_31_43.md` — prioritization section
