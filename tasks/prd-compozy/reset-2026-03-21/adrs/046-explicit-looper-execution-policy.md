# ADR-046: Explicit Looper Execution Policy

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The looper must use an explicit execution policy instead of inferring
sequencing or concurrency automatically.

The minimum policy shape includes:

- `mode`
- `max_parallelism`
- `selection`

Recommended modes:

- `sequential`
- `parallel`

The execution policy is defined by the workflow step or other control-plane
surface that starts the looper.

Subtasks may further restrict execution through fields such as:

- `depends_on`
- `parallelizable`

Subtasks do not widen the looper policy beyond what the looper was configured
to allow.

## Rationale

- Different task families need different execution shapes.
- PRD-oriented subtasks often need sequential execution.
- Review-oriented subtasks often benefit from bounded parallel execution.
- Explicit policy keeps the runtime predictable for humans, scripts, and
  internal agents.

## Consequences

- Looper runs should expose execution policy in the public model.
- The public looper surface should speak in terms of subtasks, not generic
  queue items.
- Parallel subtask execution is supported, but only under an explicit looper
  policy.
