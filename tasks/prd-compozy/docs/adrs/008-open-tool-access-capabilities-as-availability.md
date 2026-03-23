# ADR-008: Open Tool Access, Capabilities As Availability

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Tools are available by default when they exist in the runtime. Capabilities describe environment availability and operational mode, not interactive approval.

Examples:

- `git available`
- `network enabled`
- `github configured`
- `worktree supported`

## Rationale

- The platform should remain flexible and easy to compose.
- Capability metadata is still useful, but it should describe reality, not create a constant approval barrier.

## Consequences

- Runtime checks focus on whether a capability exists, not whether a human approves it right now.
- Optional static restrictions can exist later, but they do not define the baseline product model.
