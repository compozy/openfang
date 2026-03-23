## markdown

## status: pending

<task_context>
<domain>product/hardening/finalization</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task22,task29,task30,task31</dependencies>
</task_context>

# Task 32.0: Final Hardening And E2E Integration

## Overview

Final hardening pass across the entire Compozy runtime. This task covers: pack
install/upgrade/uninstall operationalization, indexing and retention policies for
append-only tables (`workflow_checkpoint`, `artifact_version`), SSE
infrastructure for remaining watch endpoints, restart/recovery regression
coverage, and a comprehensive end-to-end integration test spanning the full
event -> trigger -> workflow -> dispatch -> HITL -> completion flow.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- The system survives restart with all durable state intact.
- Pack operations work end-to-end.
- Retention policies prevent unbounded table growth.
- SSE endpoints stream real-time events.
- The E2E test validates the full automation loop.
</requirements>

## Subtasks

- [ ] 32.1 Implement pack install/upgrade/uninstall endpoints and built-in pack bootstrapping.
- [ ] 32.2 Add indexing and retention policies for append-only tables. Implement remaining SSE watch endpoints.
- [ ] 32.3 Write comprehensive E2E integration test: event ingress -> trigger match -> workflow start -> step dispatch -> HITL pause/resume -> workflow completion. Verify restart recovery across the full flow.

## Implementation Details

Pack endpoints follow API-SPEC.md section 7 (Packs). The `POST /api/packs/install`
endpoint accepts a pack source descriptor. `POST /api/packs/{id}/upgrade` and
`POST /api/packs/{id}/uninstall` handle lifecycle operations. Built-in packs are
bootstrapped on first startup through the same pack system. Upgrade dry-run
(`POST /api/packs/{id}/upgrade/dry-run`) previews effects before mutation.

Append-only tables (`workflow_checkpoint`, `artifact_version`, `doc_version`)
need indexing for common query patterns (by `run_id`, by `artifact_id`, by
`created_at`) and retention policies to prevent unbounded growth. Retention
should be configurable and should default to reasonable limits.

Remaining SSE watch endpoints from API-SPEC.md section 14 that are not yet
implemented should be completed here: `GET /api/runs/{id}/events`,
`GET /api/dispatches/{id}/events`, `GET /api/hitl-requests/stream`.

The E2E integration test exercises the full automation loop:

1. Submit an event via `POST /api/events`
2. Verify trigger matching fires the correct target action
3. Verify workflow run starts and advances through steps
4. Verify step dispatch reaches the target agent
5. Verify HITL pause occurs and resume works
6. Verify workflow completion with final output
7. Stop and restart the daemon mid-flow, verify recovery

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `crates/openfang-kernel/src/kernel.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`
- `tasks/prd-compozy/reset-2026-03-21/IMPLEMENTATION-PLAN.md`

### Dependent Files

- All crates

## Deliverables

- Pack system endpoints (install, upgrade, upgrade dry-run, uninstall)
- Retention policies for append-only tables
- SSE infrastructure for remaining watch endpoints
- E2E integration test covering the full automation loop
- Restart recovery regression suite

## Tests

### Unit Tests (Required)

- [ ] Pack install/upgrade/uninstall logic handles bundled and external sources correctly.
- [ ] Retention policy enforcement deletes old records beyond the configured limit.
- [ ] SSE message serialization produces correct event types for all watch endpoints.

### Integration Tests (Required)

- [ ] E2E flow test spanning event -> trigger -> workflow -> dispatch -> HITL -> completion.
- [ ] Restart mid-flow and verify recovery of all durable state.
- [ ] Pack install from bundled and external sources creates the correct managed definitions.

### Regression and Anti-Pattern Guards

- [ ] No append-only table grows unboundedly under sustained workload.
- [ ] All SSE endpoints stream without memory leaks over long connections.
- [ ] Restart never loses committed state from prior tasks.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- The first cohesive Compozy runtime slice is complete end-to-end.
- The full automation loop works from event ingress through workflow completion.
- Restart recovery is verified across the entire flow.
- Remaining gaps are operational polish, not missing core architecture.

---

## Prior Implementation Reference

The old TypeScript codebase has integration patterns and the full domain system surface:

- `~/Dev/compozy/compozy-code/packages/tools/src/integration/` — Integration test patterns
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/` — All backend modules (tasks, artifacts, prds, techspecs, repos, orgs, subscriptions, etc.)
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/` — 34 domain systems showing the full product surface

The old codebase is the most useful reference for E2E flow validation — it shows how events flow
through the system end-to-end, what edge cases exist in real usage, and what the complete product
surface looks like when all domain pieces are wired together.

## Notes

- This task is intentionally the final task in the PRD. It closes all cross-cutting gaps after the core system exists.
- CLI commands are deferred to future work (do not touch openfang-cli).
