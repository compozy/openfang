# ADR-015: OpenFang Skills And Schedules Remain Canonical

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The OpenFang skill system and cron/schedule system remain the canonical base in the Compozy fork.

Compozy adds product UX, packaging, defaults, and integrations on top of them instead of redefining their schemas from scratch.

For public scheduling surfaces, the fork standardizes on the typed cron scheduler model rather than the older ad hoc schedule CRUD.

## Rationale

- The skill system is already mature: `skill.toml`, `SKILL.md`, runtimes, registry, and conversion flow are all established.
- The cron/schedule model is already typed and operational, including workflow execution support.
- Redefining these systems now would add avoidable duplication and architectural drift.

## Consequences

- Skills remain compatible with the OpenFang mental model and file formats.
- Scheduling remains based on the OpenFang cron model, even if Compozy later adds better UX or extra policy fields.
- Public `schedules` surfaces in Compozy should map to the typed scheduler model, not to the older blob-style schedule storage.
- Public schedule contracts may extend the action family where required by the
  durable workflow model, especially for `workflow_signal`, but should still
  remain close to the typed cron base.
- Schedule action payloads should align with the rest of the Compozy control
  plane instead of introducing unrelated ad hoc payload formats.
- Workflow packs and product-level configuration should reference these systems rather than replace them.
- Unlike workflows and triggers, these systems do not currently justify a
  wholly separate scheduling DSL or schema family.
