# ADR-002: Keep Alpine.js + Vanilla JS

## Status

Accepted

## Date

2026-03-27

## Context

The existing dashboard is an Alpine.js v3 SPA with vanilla JavaScript, no build step, served directly by the Rust backend. Adding ~15 new page modules raises the question of whether to stay on Alpine.js or adopt a more structured framework.

## Decision

Stay on Alpine.js + vanilla JS. Invest in shared utilities (SSE client, API client v1, time formatting, status badges) to reduce duplication, but do not introduce a build step or new framework.

## Alternatives Considered

### Alternative 1: Alpine.js + Petite-Vue Hybrid

- **Description**: Use Petite-Vue for complex new pages (workflow editor, task views).
- **Pros**: Better component model for complex UIs
- **Cons**: Two reactive systems in one app, cognitive overhead for contributors
- **Why rejected**: Complexity is in data flow and API integration, not component trees. Alpine.js handles this adequately.

### Alternative 2: Migrate to React/Preact

- **Description**: Full rewrite with a build step.
- **Pros**: Better component model, ecosystem, testing tools
- **Cons**: Massive migration effort, breaks the no-build-step philosophy, delays all new feature work
- **Why rejected**: Migration effort would consume the entire UI budget. The existing Alpine.js approach works well for 16 pages and can scale to 30.

## Consequences

### Positive

- Zero migration risk to existing 16 pages
- No build tooling to maintain
- Contributors can edit JS files directly without compilation
- Consistent patterns across old and new pages

### Negative

- No component-level testing framework (Alpine.js lacks this)
- State management stays manual (Alpine stores + page functions)
- Complex forms (workflow editor) require more manual wiring than React equivalents

### Risks

- The workflow v2 editor (8 step kinds, flow modes, contract fields) is the most complex UI. Alpine.js may feel limiting. Mitigation: the Visual Builder already handles SVG rendering with manual DOM manipulation, proving the approach works for complex UIs.

## References

- `tasks/prd-ui/analysis_current_ui.md` — documents existing Alpine.js architecture
