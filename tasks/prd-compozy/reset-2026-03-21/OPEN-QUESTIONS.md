# Open Questions

These questions remain open after the architecture reset. They do not block the baseline, but they do affect implementation shape.

## Workflow Runtime

- How much of the existing OpenFang `WorkflowEngine` can be refactored in place behind the new Compozy workflow surface?
- Should checkpoints be event-like rows, coarse snapshots, or a hybrid?
- When does `workflow_step_run` become necessary as a first-class table instead of derived or embedded run state?

## Definitions and Packaging

## Tool Runtime

- How should the tool registry plug into the current OpenFang tool runner without destabilizing the existing runtime?
- Which primitives should be true tools versus special runtime handlers?
- How are skill-provided tools validated and namespaced?

## Domain Model

- What versioning rules apply to artifacts and docs?

## UX and API

- Which legacy OpenFang workflow and trigger import paths are worth keeping at all?

## Persistence and Schema

- What exact SQL types, indexes, and retention policies should back the table outline in `DATABASE-SCHEMA.md`?

## Networking

- How much of OFP/A2A should appear in the first Compozy UX?
- Should remote agent networks be visible as first-class workflow targets, or stay behind advanced configuration at first?

## Migration and Delivery

- What importer path is worth keeping from OpenFang workflow and trigger definitions into the new Compozy-facing surfaces?
- Which OpenFang areas should be upstreamed versus maintained only in the fork?
- How should existing `_analysis` material be mined for reusable content without reintroducing stale assumptions?
