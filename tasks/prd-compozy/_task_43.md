## markdown

## status: pending

<task_context>
<domain>integration/e2e</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task41,task42</dependencies>
</task_context>

# Task 43.0: E2E Integration Test And Restart Recovery Regression

## Overview

Write a comprehensive end-to-end integration test that exercises the full automation loop in a
single test process, and a restart recovery regression test that verifies all durable state
survives a kernel shutdown and re-initialization. This is the final validation pass for the
Compozy runtime, confirming that the full event -> trigger -> workflow -> dispatch -> HITL ->
completion flow works end-to-end.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start -- **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Write a comprehensive E2E integration test that exercises the full automation loop in a single
  test process using a real `compozy.db` and `runtime.db`:
  (1) submit an event via `POST /api/v1/events`;
  (2) verify trigger matching fires the correct target workflow;
  (3) verify workflow run starts and advances through steps;
  (4) verify step dispatch reaches the target agent (or a stub);
  (5) verify HITL pause occurs and answer it via `POST /api/v1/hitl-requests/{id}/answer`;
  (6) verify workflow completion with a final output artifact;
  (7) verify restart recovery by dropping and re-initializing the kernel mid-flow, then verifying
  the workflow run resumes from the correct checkpoint.
  Each step must be verified with repository or API queries, not just log output.
- The system must survive restart with all durable state intact across the full E2E flow. No
  committed record in `compozy.db` or `runtime.db` may be lost after a clean kernel shutdown and
  re-initialization.
- The test must use file-backed temp databases (not in-memory) to validate restart semantics.
- The test must use `pretty_assertions::assert_eq` for all assertions. It must not use `unwrap()`
  -- all fallible calls must use `?` with a test-level `anyhow::Result` or
  `Box<dyn std::error::Error>` return type.
</requirements>

## Subtasks

- [ ] 43.1 Write the comprehensive E2E integration test spanning all seven steps listed in
      requirements. The test must be a `#[tokio::test]` that:
      1. Creates a temp directory with fresh `runtime.db` and `compozy.db` files.
      2. Bootstraps the kernel with a test config that includes a minimal SDLC-like workflow and
         trigger definition (can be embedded as test fixtures, not necessarily the full SDLC pack).
      3. Submits an event via the kernel's event ingress or directly via the `EventBus`. Verifies
         that the trigger engine fires and a `workflow_run` is created in `compozy.db`.
      4. Verifies the workflow run advances: at least one `workflow_checkpoint` record is written,
         and the run's `current_step_id` changes.
      5. Verifies an `agent_dispatch` record is created for the agent step.
      6. Simulates HITL: verifies a `hitl_request` record is created, answers it via the repository
         layer, and verifies the dispatch continues.
      7. Waits for `workflow_run.status = "completed"` and verifies the output includes an artifact
         reference with a populated `artifact_version` record.

- [ ] 43.2 Write the restart recovery regression test. The test must:
      1. Start a workflow run and advance it to a mid-flight state (e.g., one completed step, one
         in-progress dispatch).
      2. Drop the kernel (simulating a process crash).
      3. Re-initialize the kernel with the same temp database paths.
      4. Run the restart recovery scan.
      5. Verify the completed run is still queryable with all fields intact.
      6. For the mid-flight run, verify it resumes from the last checkpoint and reaches completion.

- [ ] 43.3 Write a restart-during-HITL regression test. The test must:
      1. Start a workflow run and advance to a HITL pause.
      2. Drop the kernel.
      3. Re-initialize the kernel.
      4. Verify the HITL request is still pending and the dispatch is still `waiting_hitl`.
      5. Answer the HITL request via the repository layer.
      6. Verify the step resumes (via post-restart reconstruction from Task 31) and the workflow
         completes.

- [ ] 43.4 Verify all endpoint reachability. Every endpoint registered in Tasks 41 and 42 must
      be verified as reachable via the test router and returning correctly structured responses.
      This includes pack endpoints, SSE endpoints, and retention-related queries.

- [ ] 43.5 Add regression assertions that verify no data loss across restart:
      - All `workflow_run` records written before shutdown are readable after restart.
      - All `workflow_checkpoint` records are intact.
      - All `agent_dispatch` records are intact.
      - All `hitl_request` records are intact.
      - All `artifact_version` and `doc_version` records are intact.
      - Pack state in the `pack` table is intact.

- [ ] 43.6 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

### E2E Integration Test

The E2E test must be a `#[tokio::test]` that:

1. Creates a temp directory with fresh `runtime.db` and `compozy.db` files.
2. Bootstraps the kernel with a test config that includes a minimal SDLC-like workflow and trigger
   definition (can be embedded as test fixtures, not necessarily the full SDLC pack).
3. Submits an event via the kernel's event ingress or directly via the `EventBus`. Verifies that
   the trigger engine fires and a `workflow_run` is created in `compozy.db`.
4. Verifies the workflow run advances: at least one `workflow_checkpoint` record is written, and
   the run's `current_step_id` changes.
5. Verifies an `agent_dispatch` record is created for the agent step.
6. Simulates HITL: verifies a `hitl_request` record is created, answers it via the repository
   layer, and verifies the dispatch continues.
7. Waits for `workflow_run.status = "completed"` and verifies the output includes an artifact
   reference with a populated `artifact_version` record.
8. Restart recovery: drop the kernel, re-initialize with the same temp database paths, verify the
   completed run is still queryable with all fields intact. For a run that was mid-flight at
   "restart", verify the run resumes from the last checkpoint and reaches completion.

The test must use `pretty_assertions::assert_eq` for all assertions. It must not use `unwrap()` --
all fallible calls must use `?` with a test-level `anyhow::Result` or
`Box<dyn std::error::Error>` return type.

### Relevant Files

- `crates/openfang-api/src/routes.rs` -- AppState, handler patterns
- `crates/openfang-api/src/server.rs` -- route registration, `build_router`
- `crates/openfang-kernel/src/kernel.rs` -- boot sequence, background task spawning
- `crates/openfang-kernel/src/event_bus.rs` -- event bus for SSE fan-out
- `crates/openfang-memory/src/migration.rs` -- migration runner pattern
- `crates/openfang-memory/src/substrate.rs` -- shared connection pattern
- All crates -- this is the final integration task

### Dependent Files

- Task 41 -- pack system (verified by this task)
- Task 42 -- retention and SSE endpoints (verified by this task)
- Task 30 -- HITL live path (verified by this task)
- Task 31 -- HITL post-restart reconstruction (verified by this task)
- Task 36 -- event ingress pipeline (verified by this task)

## Deliverables

- Comprehensive E2E integration test covering all seven flow steps
- Restart recovery regression test (mid-flight run)
- Restart-during-HITL regression test
- Endpoint reachability verification
- Data integrity assertions across restart
- All tests pass with zero warnings

## Tests

### Integration Tests (Required)

- [ ] E2E flow test: event ingress -> trigger match -> workflow run creation -> step dispatch ->
      HITL pause -> HITL answer -> workflow completion -> output artifact with provenance. Each
      transition is verified with a repository or API query, not log inspection.
- [ ] Restart mid-flow recovery: a workflow run in `running` status with one completed checkpoint
      and one in-progress dispatch survives a kernel shutdown and re-initialization; after recovery
      the run resumes from the correct step, completes, and the final
      `workflow_run.status = "completed"`.
- [ ] Restart during HITL: a workflow run paused on a HITL request survives a kernel shutdown;
      after restart, the HITL request is still pending; answering it triggers post-restart
      reconstruction and the workflow completes.
- [ ] No data loss: all records written before kernel shutdown are readable after re-initialization,
      including workflow_run, workflow_checkpoint, agent_dispatch, hitl_request, artifact_version,
      doc_version, and pack records.
- [ ] Full control-plane round-trip: every endpoint registered in Tasks 41 and 42 is reachable
      via the test router and returns non-empty, correctly structured responses.

### Regression and Anti-Pattern Guards

- [ ] Restart never loses committed state from prior tasks; all records written before a kernel
      shutdown must be readable after re-initialization.
- [ ] The E2E test must use file-backed temp databases (not in-memory) to validate restart
      semantics -- in-memory databases cannot survive a kernel drop/re-init.
- [ ] All assertions use `pretty_assertions::assert_eq`, not `std::assert_eq`.
- [ ] No `unwrap()` in test code -- all fallible calls use `?` with proper error return types.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- The first cohesive Compozy runtime slice is complete end-to-end: events flow from ingress
  through trigger match, workflow execution, dispatch, HITL interaction, and completion, all
  backed by durable records in `compozy.db`.
- The E2E test exercises all seven flow steps and verifies restart recovery using file-backed
  databases.
- All durable state survives a clean kernel shutdown and re-initialization without data loss.
- Restart during a HITL interaction is safe: the pending state is preserved and answerable after
  restart.
- Remaining gaps after this task are operational polish, not missing core architecture.
- `cargo fmt --all`, `cargo clippy`, and `cargo test --workspace` all pass at zero warnings and
  zero failures.

---

## Prior Implementation Reference

The old TypeScript codebase has integration patterns and the full domain system surface:

- `~/Dev/compozy/compozy-code/packages/tools/src/integration/` -- Integration test patterns
- `~/Dev/compozy/compozy-code/packages/backend/src/modules/` -- All backend modules (tasks, artifacts, prds, techspecs, repos, orgs, subscriptions, etc.)
- `~/Dev/compozy/compozy-code/packages/tauri/src/renderer/systems/` -- 34 domain systems showing the full product surface

The old codebase is the most useful reference for E2E flow validation -- it shows how events flow
through the system end-to-end, what edge cases exist in real usage, and what the complete product
surface looks like when all domain pieces are wired together.

## Notes

- This task is intentionally the final task in the PRD. It closes all cross-cutting gaps after
  the core system exists.
- CLI commands are deferred to future work (do not touch openfang-cli).
- This task validates the work of Tasks 30, 31, 35, 36, 41, and 42 through integration coverage.
