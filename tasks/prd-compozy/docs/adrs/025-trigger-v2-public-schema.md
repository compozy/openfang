# ADR-025: Trigger v2 Public Schema

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The public trigger contract is centered on explicit matching and explicit targets.

Top-level trigger fields:

- `id`
- `name`
- `description`
- `enabled`
- `max_fires`
- `cooldown_secs`
- `match`
- `target`

Match fields remain intentionally simple:

- `event`
- `source`
- `contains`
- `filters`

Supported target kinds:

- `agent_message`
- `workflow_start`
- `workflow_signal`

`workflow_signal` includes an explicit selector for the destination run.

## Rationale

- The OpenFang trigger matcher is useful, but the current public trigger contract is too tied to `agent_id + prompt_template`.
- The product needs to react to events by starting and signaling durable workflows, not only waking agents.
- An explicit target model is the cleanest way to evolve triggers without creating a second event system.

## Consequences

- Trigger authoring becomes clearer and more product-oriented.
- Internally, the fork can still compile matching to the existing OpenFang trigger engine where appropriate.
- The public trigger model no longer treats agent wakeup as the only first-class outcome.
