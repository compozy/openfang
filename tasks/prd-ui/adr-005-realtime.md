# ADR-005: SSE Everywhere It Exists

## Status

Accepted

## Date

2026-03-27

## Context

The backend exposes 6 SSE endpoints (runs, dispatches, HITL stream, looper, comms, logs). The UI currently uses polling (5s global, 15s channels/peers, 2s logs fallback) for almost everything. Only logs and comms attempt SSE, and the comms SSE is broken due to a bug. WebSocket is used only for chat streaming.

## Decision

Build a shared SSE client utility (`OpenFangSSE`). Use SSE for all 6 domains that support it. Keep polling only for domains without SSE endpoints. The HITL nav badge is driven by a global SSE stream that stays connected across page navigations.

## Alternatives Considered

### Alternative 1: SSE for Critical Paths Only

- **Description**: Wire SSE only for HITL and run events. Keep polling for everything else.
- **Pros**: Less infrastructure work
- **Cons**: Misses the benefit of real-time dispatches and looper progress
- **Why rejected**: The backend already exposes the SSE endpoints. The shared utility makes wiring them trivial.

### Alternative 2: WebSocket Hub

- **Description**: Single multiplexed WebSocket connection for all real-time events.
- **Pros**: Single connection, lower overhead
- **Cons**: Requires backend changes to create a hub endpoint, more complex protocol
- **Why rejected**: Backend already implements SSE. Creating a WS hub adds backend work that is out of scope.

### Alternative 3: Keep Polling, Optimize Intervals

- **Description**: Tune polling intervals per domain.
- **Pros**: Simplest approach, no new infrastructure
- **Cons**: Higher server load, worse responsiveness (up to 5s delay for HITL)
- **Why rejected**: HITL needs sub-second responsiveness. Polling at 1s for HITL would create unnecessary load when SSE is free.

## Consequences

### Positive

- Sub-second HITL notification via global SSE stream
- Live progress for runs, dispatches, looper without polling
- Reduced server load (SSE connections are idle until events fire)
- `Last-Event-ID` enables seamless reconnection

### Negative

- More open connections per browser tab (up to 6 SSE + 1 WS)
- Browser limit of ~6 connections per domain may be hit. Mitigation: SSE connections are only opened on relevant pages, plus one global HITL stream.

### Risks

- SSE endpoints may have bugs not caught by the backend E2E tests (task 33.9 HTTP tests are missing). Mitigation: test each SSE endpoint manually during UI integration.

## Implementation Notes

- Global HITL SSE: connected in `app.js` on boot, drives `$store.app.pendingHitlCount`
- Page-scoped SSE: connected on page mount, disconnected on page leave
- Fallback: if SSE connection fails after max retries, fall back to polling for that domain

## References

- `tasks/prd-ui/analysis_api_routes.md` — section "SSE Endpoints Available but Unused by UI"
- `tasks/prd-ui/analysis_current_ui.md` — section "Polling Intervals" and "Real-time Patterns"
