# ADR-050: Initial Dual-Database Migration Slice

**Status:** Accepted
**Date:** 2026-03-21

## Decision

The first concrete migration slice should be split by database ownership.

`runtime.db` starts with:

1. schema migration tracking
2. `agent_runtime`
3. `agent_session`
4. `agent_message`
5. `schedule_runtime`
6. `schedule_execution`

`compozy.db` starts with:

1. schema migration tracking
2. `workflow_run`
3. `workflow_checkpoint`
4. `workflow_signal`

The initial durable workflow runtime should also adopt conservative recovery:

- `waiting_signal` survives restart as waiting
- in-flight `running` runs are downgraded to `paused` until later phases add
  more detailed execution durability

## Rationale

- This is the smallest slice that makes dual-database ownership real instead of
  theoretical.
- It delivers durable workflow identity and signal handling without blocking on
  dispatch, HITL, tasks, or looper execution.
- Conservative restart behavior is safer than pretending early phases can
  automatically resume arbitrary in-flight work.

## Consequences

- migration streams must be maintained independently for `runtime.db` and
  `compozy.db`
- Phase 0 and Phase 1 runtime code should target only these tables first
- richer runtime tables remain deferred to later phases already accepted in the
  implementation plan
- the detailed slice and handler order should live in
  `INITIAL-RUNTIME-MIGRATIONS.md`
