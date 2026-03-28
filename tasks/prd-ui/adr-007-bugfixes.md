# ADR-007: Fix All 3 Bugs Before UI Overhaul

## Status

Accepted

## Date

2026-03-27

## Context

Three confirmed bugs exist in the current UI:
1. Trigger routes (`/api/triggers`) not registered in `server.rs` — scheduler trigger tab returns 404
2. `comms.js` SSE references non-existent `OpenFangAPI.baseUrl`/`apiKey` — SSE silently fails
3. `workflows.js` uses `window.confirm()` instead of `OpenFangToast.confirm()` — inconsistent UX

These could be fixed inline with their respective page rebuilds (triggers in Phase 3, comms in Phase 6, workflows in Phase 3) or as a prerequisite.

## Decision

Fix all 3 bugs as a prerequisite Phase 0 step before starting the UI overhaul. This gets the existing UI to a working baseline.

## Alternatives Considered

### Alternative 1: Fix Inline with Rebuilds

- **Description**: Each bug is fixed when its page is rebuilt on v1.
- **Pros**: No separate bug-fix phase, changes are bundled
- **Cons**: Trigger tab stays broken for months until Phase 3. Comms SSE stays broken until Phase 6.
- **Why rejected**: The trigger 404 and comms SSE failures affect users today. Waiting months is unacceptable.

### Alternative 2: Fix Critical Only First

- **Description**: Fix only the trigger route registration (visible 404s). Defer the others.
- **Pros**: Smaller initial fix
- **Cons**: Comms SSE stays broken (silent failure, hard to diagnose)
- **Why rejected**: All 3 fixes are small and independent. No reason to leave known bugs unfixed.

## Consequences

### Positive

- Existing UI works correctly before overhaul begins
- Quick win, small PR, fast to review
- Establishes quality baseline

### Negative

- The trigger legacy route registration may become dead code after Phase 3 migrates to v1 triggers. Acceptable: it's 3 lines in `server.rs`.

## Implementation Notes

- Bug 1: Add `.route("/api/triggers", get(list_triggers))` and sibling routes to `server.rs`
- Bug 2: Replace `${OpenFangAPI.baseUrl}/api/comms/events/stream` with proper URL construction using `OpenFangSSE` or `new EventSource('/api/comms/events/stream?token=...')`
- Bug 3: Replace `if (confirm(...))` with `OpenFangToast.confirm(title, msg, callback)`

## References

- `tasks/prd-ui/analysis_api_routes.md` — section "Critical Bug: Unregistered Trigger Routes"
- `tasks/prd-ui/analysis_current_ui.md` — section "Known Bugs & Limitations"
