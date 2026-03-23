# ADR-016: Extend Triggers And Workflows, Do Not Redefine Everything

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The main schema and runtime evolution in the fork happens in `workflows` and `triggers`.

- Workflows are hardened for durable runs and richer product use.
- Triggers are extended so they can target durable workflow behavior, not only wake agents.
- Where the current OpenFang public surfaces block that evolution, the fork introduces new Compozy-facing workflow and trigger surfaces instead of preserving old contracts by default.
- The fork avoids redefining unrelated systems that already work well enough.

## Rationale

- The current OpenFang workflow runtime is the largest mismatch with Compozy's durability goals.
- The current trigger model is strongly agent-centric and likely needs broader targeting semantics.
- These are the areas where fork investment has the highest leverage.

## Consequences

- Workflow runtime refactoring is a foundational project.
- Trigger semantics may widen beyond the current agent-wakeup model.
- Skills, schedules, and agent manifests should not be destabilized without a stronger reason.
- Old OpenFang workflow and trigger shapes can survive as import or legacy paths, but they are not the design center.
