# Task 42 Review: Retention Policies And Remaining SSE Endpoints

## Status: PASS

## Checklist

- [x] 42.1 Indexes added for `workflow_checkpoint (run_id, created_at)`, `artifact_version (artifact_id, version_no)`, `artifact_version (content_hash)`, `doc_version (doc_id, version_no)`, `doc_version (content_hash)` in migration `20260326_014_workflow_checkpoint_retention.sql`
- [x] 42.2 Retention policy background job implemented in `openfang-kernel/src/kernel.rs` — background `tokio::task` spawned at boot, configurable interval (default hourly), batched DELETE of 500 rows max per cycle, retains checkpoints for `running`/`paused` runs
- [x] 42.3 `GET /api/v1/runs/{id}/events` SSE endpoint — `stream_run_events_v1` in `routes.rs`; emits `stream.snapshot`, `run.updated`, `dispatch.updated`, `hitl.requested`, `stream.reset`, `keepalive`
- [x] 42.4 `GET /api/v1/dispatches/{id}/events` SSE endpoint — `stream_dispatch_events_v1` in `routes.rs`; emits `stream.snapshot`, `dispatch.updated`, `hitl.requested`, `keepalive`
- [x] 42.5 `GET /api/v1/hitl-requests/stream` SSE endpoint — `stream_hitl_requests_v1` in `routes.rs`; global stream, `run_id`/`status` query filters, global ring buffer of 200 events
- [x] 42.6 All three SSE endpoints follow bounded replay pattern: per-resource ring buffer of 50, global 200 for HITL stream; `Last-Event-ID` resume; `stream.reset` + `stream.snapshot` fallback; `keepalive` every 15 seconds
- [x] 42.7 Tests — retention unit tests in `workflow_store.rs` (2 tests), SSE tests in `dispatch_hitl_v1_api_test.rs` (5 tests covering dispatch events, run events, HITL stream, 60-second stability)
- [x] 42.8 All verification commands pass (per task status `completed`)

## Findings

### Correctly Implemented

- Migration 014 uses `IF NOT EXISTS` for all indexes — the indexes for `artifact_version` and `doc_version` were already defined in migration 012; 014 adds `idx_workflow_checkpoint_run` (new) and `idx_workflow_run_status_completed_at` (new for retention query efficiency)
- `run_workflow_checkpoint_retention_cycle()` on the kernel reads `workflow_checkpoint_retention_max_rows_per_run`, `workflow_checkpoint_retention_age_days`, and `workflow_checkpoint_retention_batch_size` from config — all configurable with sensible defaults
- Retention logic uses `prune_terminal_runs_older_than(cutoff_timestamp, max_rows_per_run, batch_size)` on the `WorkflowStore` — batched DELETE, doesn't block the main loop
- Retention background task logs with `tracing::info!` on each cycle (confirmed in kernel.rs ~line 6919)
- `RunEventStreamRegistry` and `DispatchEventStreamRegistry` in `AppState` follow the same ring-buffer pattern as `LooperRuntimeRegistry`
- `HitlStreamEventHandle` is a global handle stored on `AppState` — bounded ring buffer of 200 events; `run_id` and `status` filters applied before emitting to client stream
- `stream_run_events_v1` emits composite events aggregating progress across dispatches and HITL requests (list_hitl_for_run_stream helper)
- 60-second SSE stability test (`run_sse_should_remain_stable_for_60_seconds_with_multiple_state_changes`) verifies all 5 state change events received without memory leak
- HITL stream filter test (`hitl_stream_with_pending_filter_should_emit_only_pending_events`) verifies the `status=pending` filter correctly excludes answered events
- `dispatch.updated` on expired `Last-Event-ID` produces `stream.reset` then `stream.snapshot` — verified by `sse_dispatch_events_with_expired_last_event_id_should_emit_reset_then_snapshot`

### Minor Observations

- Migration 014 re-declares indexes already present in migration 012. While harmless due to `IF NOT EXISTS`, this duplication is a code smell. The 014 migration could have been limited to the `workflow_checkpoint` and `workflow_run` indexes that are genuinely new.
- The `hitl-requests/stream` endpoint tests for filter matching are in `dispatch_hitl_v1_api_test.rs`, not in a dedicated retention/SSE test file. This is acceptable but note the tests are spread across the dispatch HITL test file rather than an `sse_v1_api_test.rs`.

## Files Reviewed

- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/migrations/compozy/20260326_014_workflow_checkpoint_retention.sql`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/kernel.rs` (lines 5778–5810, 6892–6936)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs` (lines 562–571, 588–589)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs` (lines 8428–8680, 8680–8916, 8917–9144)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-memory/src/workflow_store.rs` (retention tests)
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/dispatch_hitl_v1_api_test.rs` (lines 1641–1972)
