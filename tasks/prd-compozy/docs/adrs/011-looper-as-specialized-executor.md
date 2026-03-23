# ADR-011: Looper As Specialized Executor

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The looper is a specialized executor for iterative subtask work. It is not the
general workflow system and not a separate orchestration engine.

## Rationale

- Recipes and workflows model processes.
- The looper models repeated work over executable subtasks with progress,
  continuation, and re-planning behavior.
- Keeping the looper specialized prevents the product from collapsing into one orchestration abstraction used for everything.

## Consequences

- The looper gets its own durable run object.
- Looper execution policy should be explicit rather than inferred.
- It should reuse the shared durable workflow foundation wherever possible.
