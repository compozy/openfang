# ADR-013: Optional Delegated Repo And Workspace Automation

**Status:** Accepted
**Date:** 2026-03-21

## Decision

Git, PR, and workspace automation are optional delegated capabilities. The product must work without forcing a single repo automation model on every installation.

## Rationale

- The product does not need parity with the older Compozy worktree-heavy model.
- Repo automation remains useful, but it should be opt-in and composable.

## Consequences

- The system needs a clear way to express whether repo automation is available.
- Workflows and agents should adapt to the workspace mode instead of assuming one mandatory strategy.
