# ADR-001: Dashboard Audience — Both Operators and Developers

## Status

Accepted

## Date

2026-03-27

## Context

The OpenFang dashboard needs to integrate ~140 new backend endpoints spanning operations (HITL, runs, dispatches) and authoring (workflows, triggers, tasks). The dashboard could optimize for one audience or serve both. The CLI already exists as a power-user tool.

## Decision

The dashboard serves as the single control plane for both operators (monitoring, HITL, run management) and developers (authoring workflows, triggers, testing events). Every feature gets both read and write UI.

## Alternatives Considered

### Alternative 1: Operators/DevOps First

- **Description**: Optimize for monitoring, alerts, real-time visibility. Authoring stays CLI-only.
- **Pros**: Smaller scope, faster delivery, focused UX
- **Cons**: Developers forced to use CLI for all authoring. Dashboard becomes read-only for half the features.
- **Why rejected**: The v1 API has rich authoring endpoints (validate, compile, dry-run) that benefit greatly from visual feedback.

### Alternative 2: Minimal/Read-Only Dashboard

- **Description**: Dashboard is secondary to CLI. Show status only.
- **Pros**: Minimal UI work.
- **Cons**: Wastes the investment in v1 API endpoints. HITL requires interactive UI by definition.
- **Why rejected**: HITL requests block execution and cannot be effectively answered through a read-only dashboard.

## Consequences

### Positive

- One place for all users to manage the system
- Authoring features (validate, compile, dry-run) benefit from visual feedback
- HITL inbox is interactive by default

### Negative

- Larger scope: every domain gets full CRUD
- More complex UI with more pages/navigation
- Longer implementation timeline

### Risks

- UX complexity: too many features may overwhelm users. Mitigation: workflow-centric navigation grouping.

## References

- `tasks/prd-ui/analysis_cli_features.md` — CLI features that need UI counterparts
