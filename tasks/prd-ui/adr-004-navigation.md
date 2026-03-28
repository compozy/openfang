# ADR-004: Workflow-Centric Navigation Groups

## Status

Accepted

## Date

2026-03-27

## Context

The current sidebar has 6 groups (Chat, Monitor, Agents, Automation, Extensions, System) with 16 items. The UI integration adds ~10 new page domains, bringing the total to ~26 navigable pages. The current grouping does not reflect the new operational and authoring workflows.

## Decision

Reorganize the sidebar into workflow-centric groups:

- **Chat** — agent conversations (existing)
- **Operations** — HITL Inbox (with badge), Runs, Dispatches, Looper
- **Workspace** — Tasks, Workflows, Triggers, Schedules
- **Resources** — Agents, Skills, Hands, Packs
- **Outputs** — Artifacts, Documents
- **Monitor** — Overview, Analytics, Logs
- **System** — Channels, Integrations, Settings, Runtime

## Alternatives Considered

### Alternative 1: Flat List with Search

- **Description**: All pages as flat nav items with quick-search filter.
- **Pros**: Simple, no grouping decisions needed
- **Cons**: Overwhelming with 26+ items, no semantic organization
- **Why rejected**: Users need to understand the system's domains to navigate effectively.

### Alternative 2: Keep Current Groups, Add New Section

- **Description**: Keep existing 6 groups, add a "Compozy" section for all new features.
- **Pros**: Minimal disruption to existing navigation
- **Cons**: Creates awkward split between old and new features, "Compozy" section would have 10+ items
- **Why rejected**: Users don't think in terms of "legacy vs new"; they think in terms of what they're trying to do.

### Alternative 3: Two-Level Nav with Top Tabs

- **Description**: Horizontal tabs for major domains, each with sidebar sub-nav.
- **Pros**: More structured, scales well
- **Cons**: Bigger UX change, more complex navigation state
- **Why rejected**: Adds navigation complexity without clear benefit over collapsible sidebar groups.

## Consequences

### Positive

- Navigation reflects user workflows (operate, author, manage resources, view outputs, monitor)
- HITL badge is prominent in Operations group (most urgent action)
- Related features are adjacent (Tasks near Workflows, Runs near HITL)

### Negative

- Existing users must relearn navigation
- More sidebar groups may require collapsible sections

### Risks

- 26 items may still feel like a lot. Mitigation: collapsible groups with last-used state persisted in localStorage.

## References

- `tasks/prd-ui/analysis_current_ui.md` — documents current sidebar structure
