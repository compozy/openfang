## markdown

## status: pending

<task_context>
<domain>openfang-memory,openfang-kernel</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>low</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 48.0: HITL Timeout Enforcement

## Overview

The `hitl_request` table stores a `timeout_at` column, but no background task monitors for expired requests. Pending HITL requests that exceed their timeout remain in `pending` status indefinitely, blocking workflow execution. This task adds a periodic sweep that transitions expired requests to `timed_out` and resumes the blocked workflow with a timeout error.

<critical>
- **ALWAYS READ** @CLAUDE.md before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-gaps/techspec.md` (Gap 8) before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass at 100%
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Add `expire_timed_out_requests()` method to `SqliteHitlRepository`
- The method must atomically transition `pending` requests with `timeout_at < now()` to `timed_out`
- Spawn a background task in the kernel that runs the sweep every 30 seconds
- The background task must respect `CancellationToken` for graceful shutdown
- Non-expired and already-answered requests must be unaffected
- The sweep must be idempotent (safe to run multiple times)
</requirements>

## Subtasks

- [ ] 48.1 Add `expire_timed_out_requests()` to `SqliteHitlRepository` in `crates/openfang-memory/src/hitl.rs`
- [ ] 48.2 The query must use `UPDATE hitl_request SET status = 'timed_out' WHERE status = 'pending' AND timeout_at IS NOT NULL AND timeout_at < datetime('now')`
- [ ] 48.3 Return the count of expired requests and their IDs (for logging/notification)
- [ ] 48.4 Add background sweep task in `start_background_agents()` in `crates/openfang-kernel/src/kernel.rs`
- [ ] 48.5 Wire `CancellationToken` so the sweep stops on daemon shutdown
- [ ] 48.6 Log expired requests at `warn` level with request IDs
- [ ] 48.7 Write unit tests for the expiry query
- [ ] 48.8 Write integration test for the full sweep lifecycle
- [ ] 48.9 Run `make fmt && make lint && make test` — all must pass

## Implementation Details

### SQL Query

```sql
UPDATE hitl_request
SET status = 'timed_out',
    answered_at = datetime('now')
WHERE status = 'pending'
  AND timeout_at IS NOT NULL
  AND timeout_at < datetime('now')
RETURNING hitl_request_id, run_id, step_id
```

### Background Task

```rust
// In kernel.rs, start_background_agents():
tokio::spawn({
    let stores = workflow_stores.clone();
    let cancel = self.workflow_dispatch_cancel.clone();
    async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {
                    match stores.hitl.expire_timed_out_requests().await {
                        Ok(expired) if !expired.is_empty() => {
                            tracing::warn!(
                                count = expired.len(),
                                ids = ?expired,
                                "Expired timed-out HITL requests"
                            );
                        }
                        Err(e) => {
                            tracing::warn!("HITL timeout sweep failed: {e}");
                        }
                        _ => {}
                    }
                }
            }
        }
    }
});
```

### Relevant Files

- `crates/openfang-memory/src/hitl.rs` — Add `expire_timed_out_requests()` method
- `crates/openfang-kernel/src/kernel.rs` — Add background sweep in `start_background_agents()`

### Dependent Files

- `crates/openfang-memory/migrations/compozy/20260324_009_hitl_request.sql` — Schema reference (read-only, no changes)

## Deliverables

- `expire_timed_out_requests()` method on `SqliteHitlRepository`
- Background sweep task in kernel boot sequence
- CancellationToken integration for graceful shutdown
- Warning-level logging for expired requests
- Unit and integration tests

## Tests

### Unit Tests (Required)

- [ ] Insert HITL request with `timeout_at` in the past → `expire_timed_out_requests()` transitions to `timed_out`
- [ ] Insert HITL request with `timeout_at` in the future → remains `pending` after sweep
- [ ] Insert HITL request with `timeout_at = NULL` → remains `pending` (no timeout configured)
- [ ] Insert HITL request with `status = 'answered'` and past `timeout_at` → remains `answered` (already resolved)
- [ ] Run sweep twice on same data → second run returns empty (idempotent)
- [ ] Multiple expired requests → all transitioned in single sweep

### Integration Tests (Required)

- [ ] End-to-end: Create workflow run with HITL step, set short timeout, wait for sweep, verify run receives timeout error
- [ ] Verify sweep does not interfere with concurrent `answer_hitl_request` calls (no race condition)

### Regression and Anti-Pattern Guards

- [ ] Existing HITL tests pass unchanged (no regressions in answer/cancel flows)
- [ ] No test-only production APIs introduced
- [ ] Assertions target observable state (DB status column), not internal sweep mechanics

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Expired HITL requests automatically transition to `timed_out`
- Non-expired requests unaffected
- Sweep runs every 30s and stops cleanly on shutdown
- Zero warnings, zero errors, zero test failures

---

## Notes

- This task is independent of tasks 44-47 and can be executed in parallel
- The 30-second interval balances responsiveness vs DB load for SQLite
- `RETURNING` clause provides IDs for logging without a separate SELECT query
