## markdown

## status: pending

<task_context>
<domain>engine/workflows/definitions</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 7.0: Workflow Definition Source-Of-Truth Consistency

## Overview

Fix the current inconsistency between in-memory workflow state and file-backed
workflow definitions so that restart cannot resurrect stale workflow definitions
and no mutation path can leave memory and disk in a diverged state.

The current implementation in `crates/openfang-kernel/src/workflow.rs` maintains
`WorkflowEngine` with a purely in-memory `HashMap<WorkflowId, Workflow>`. The
`create_workflow` handler in `crates/openfang-api/src/routes.rs` persists to
disk on create (as a best-effort `tracing::warn!` on failure), but `update_workflow`
does not persist to disk at all, and `delete_workflow` does not remove the file.
This means a restart after an update or delete will reload the pre-mutation
version from disk — exactly the kind of stale-definition resurrection that later
run-durability work cannot tolerate.

Per ADR-037, file-backed definitions under `~/.compozy/workflows/` are the
canonical source of truth. Per ADR-004, CLI and API mutations must write the
canonical file-backed representation. This task enforces both rules across all
three mutation paths before any Phase 1 run-durability work begins.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Create, update, and delete must keep filesystem state and runtime state coherent as a single atomic operation — file write must precede or accompany memory mutation, never follow it as a best-effort afterthought.
- Restart must not reload stale workflow definitions from disk. `load_workflows_from_dir` in `kernel.rs` must only load definitions that reflect the current canonical state.
- The file-backed representation must be the canonical TOML or JSON document for the definition — per ADR-040, the authoring format is TOML and the transport format is JSON. The persisted file must be the same logical model the API returns.
- Delete must atomically remove the definition file and deregister from the in-memory registry. A partial delete (file removed but registry still has entry, or vice versa) must be detected and rejected.
- Update must overwrite the canonical definition file before updating the in-memory entry. The old on-disk content must not remain after a successful update response.
- No code path may write to the in-memory registry without also writing to disk, and no code path may write to disk without also updating the in-memory registry. Per ADR-037, the database must not become a competing definition store.
- Error handling for filesystem operations must produce proper `Result` propagation — the current `tracing::warn!`-and-continue pattern in `create_workflow` must be replaced with a real error path that returns a 500 to the caller if the file write fails.
</requirements>

## Subtasks

- [ ] 7.1 Audit all three mutation paths (`create_workflow`, `update_workflow`, `delete_workflow` in `crates/openfang-api/src/routes.rs`) and map exactly where file and memory diverge.
- [ ] 7.2 Fix `update_workflow`: persist the updated canonical definition file before calling `WorkflowEngine::update_workflow`, returning an error if the file write fails. File path must use the same naming convention as create.
- [ ] 7.3 Fix `delete_workflow`: remove the definition file atomically alongside `WorkflowEngine::remove_workflow`. If the file does not exist, treat it as a warning rather than a hard error, but do not silently succeed if the in-memory entry was also missing.
- [ ] 7.4 Harden `create_workflow`: replace the best-effort `tracing::warn!` file-write with a real error path. If disk persistence fails, roll back the in-memory registration and return a 500.
- [ ] 7.5 Fix `load_workflows_from_dir` in `crates/openfang-kernel/src/kernel.rs`: confirm it loads exactly the files present on disk and does not re-register definitions that were deleted since the last boot. Add deduplication guard if the same `WorkflowId` is registered more than once.
- [ ] 7.6 Introduce a `WorkflowDefinitionStore` abstraction (or equivalent internal helper) in `crates/openfang-kernel/src/workflow.rs` that encapsulates the file-path convention and the read/write/delete operations, so future callers cannot bypass disk persistence accidentally.
- [ ] 7.7 Add restart-behavior integration tests that exercise create, update, and delete across a simulated restart by calling `load_workflows_from_dir` on a temp directory.

## Implementation Details

The current state of the workflow mutation paths in `crates/openfang-api/src/routes.rs`:

- `create_workflow` (line 776): registers in memory first via `kernel.register_workflow(workflow.clone()).await`, then attempts to persist to disk with a `tracing::warn!` on failure. This is a best-effort write, not an atomic one. A disk write failure leaves memory with a definition that has no corresponding file and will vanish on restart.
- `update_workflow` (line 988): calls `WorkflowEngine::update_workflow` only — no file persistence at all. After an update, the file on disk still holds the old definition. Restart will overwrite the in-memory update with the stale on-disk version.
- `delete_workflow` (line 1093): calls `WorkflowEngine::remove_workflow` only — no file removal. After a delete, the file on disk remains. Restart will resurrect the deleted workflow.

The `WorkflowEngine` struct in `crates/openfang-kernel/src/workflow.rs` holds:

- `workflows: Arc<RwLock<HashMap<WorkflowId, Workflow>>>` — pure in-memory, no file awareness.
- `runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>` — pure in-memory run state.

The `load_workflows_from_dir` method (kernel.rs line 3757) scans `.json` files and calls `register_workflow` for each. It does not check whether the `WorkflowId` already exists in memory, so a restart with a stale-updated file will silently register a second conflicting definition alongside any in-memory state that survived shutdown.

The fix should follow ADR-037 and ADR-040:

- Files under `~/.compozy/workflows/` remain the source of truth.
- Mutation paths must write the canonical file before or atomically with the memory mutation.
- The persisted JSON is the transport-format representation of the same logical model returned by the API.
- No database column should become a competing copy of definition content.

The `WorkflowDefinitionStore` abstraction should handle:

- Deterministic file paths: `{workflows_dir}/{workflow_id}.json` (preserving the existing naming convention).
- Atomic write semantics: write to a `.tmp` file first, then rename into place.
- Read-back verification: after persist, confirm the file is readable and deserializes to the same workflow.
- Clean delete: remove the file and return an error if the `std::fs::remove_file` fails for reasons other than `NotFound`.

### Relevant Files

- `crates/openfang-api/src/routes.rs` — mutation handlers (create, update, delete)
- `crates/openfang-kernel/src/kernel.rs` — `load_workflows_from_dir`, `register_workflow`, `run_workflow`
- `crates/openfang-kernel/src/workflow.rs` — `WorkflowEngine` struct and all registry methods
- `tasks/prd-compozy/docs/DESIGN.md` — section 6 (Config-First Surface), section 27 (Safe Delivery Order)
- `tasks/prd-compozy/docs/adrs/037-file-backed-definitions-and-db-ownership.md`
- `tasks/prd-compozy/docs/adrs/004-config-first-agents-and-workflows.md`
- `tasks/prd-compozy/docs/adrs/040-toml-authoring-json-transport-ir-execution.md`

### Dependent Files

- `crates/openfang-kernel/src/config.rs` — `KernelConfig.workflows_dir` field used for file paths
- Any test files under `crates/openfang-kernel/` or `crates/openfang-api/` that exercise workflow CRUD

## Deliverables

- All three mutation paths (create, update, delete) are coherent: file and memory are always in sync after a mutation succeeds.
- Restart-safe definition reload: `load_workflows_from_dir` only loads the canonical definitions that exist on disk at boot time.
- Regression tests that prove no stale-definition resurrection can occur after any of the three mutation paths.
- No best-effort fire-and-forget file writes remain in the workflow mutation paths.

## Tests

### Unit Tests (Required)

- [ ] `workflow_update_persists_canonical_definition`: call `update_workflow`, then read the file from disk, deserialize it, and assert the file content matches the updated definition — not the pre-update version.
- [ ] `workflow_delete_removes_definition_file`: call `delete_workflow`, then assert the corresponding `.json` file no longer exists in the workflows directory.
- [ ] `workflow_create_rolls_back_on_disk_failure`: simulate a disk write failure during `create_workflow` and assert the in-memory registry does not contain the new workflow after the error response.
- [ ] `workflow_update_rolls_back_on_disk_failure`: simulate a disk write failure during `update_workflow` and assert the in-memory registry still holds the pre-update version.
- [ ] `runtime_registry_and_file_store_stay_aligned`: after each of create, update, and delete, assert that the set of workflow IDs in memory equals the set of IDs from files present on disk.

### Integration Tests (Required)

- [ ] `create_then_restart_then_reload_returns_same_definition`: create a workflow via the route handler, simulate a restart by calling `load_workflows_from_dir` on the workflows directory, then assert the reloaded definition is identical to the one returned by the original create.
- [ ] `update_then_restart_then_reload_reflects_updated_definition`: update a workflow, simulate a restart, then assert the reloaded definition reflects the update — not the pre-update version.
- [ ] `delete_then_restart_does_not_resurrect_workflow`: delete a workflow, simulate a restart by calling `load_workflows_from_dir`, then assert the deleted workflow is not re-registered.
- [ ] `concurrent_update_and_reload_is_coherent`: run an update and a `load_workflows_from_dir` concurrently and assert no mixed state is visible to readers.

### Regression and Anti-Pattern Guards

- [ ] No code path calls `WorkflowEngine::register`, `WorkflowEngine::update_workflow`, or `WorkflowEngine::remove_workflow` without also performing the corresponding file operation.
- [ ] No code path writes a workflow file without also updating the in-memory registry.
- [ ] No database column or `compozy.db` table is introduced as a competing store of definition content — per ADR-037, the database stores runtime projections, not definition sources.
- [ ] File write errors must surface as HTTP 500 responses, not as `tracing::warn!` with a silent success.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- After any successful mutation (create, update, delete), the set of files in `~/.compozy/workflows/` exactly matches the set of definitions in the in-memory `WorkflowEngine` registry.
- A simulated restart (calling `load_workflows_from_dir` on the same directory) produces an identical registry to the one that existed before the restart, for every combination of create, update, and delete operations.
- All three mutation paths return a non-2xx status code if the filesystem operation fails — no silent partial mutations.
- The `WorkflowDefinitionStore` abstraction (or equivalent) is the only place in the codebase that constructs workflow file paths and performs workflow file I/O.
- `cargo test --workspace` passes with zero failures and `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.
- Phase 1 run-durability work (task 16) can document this task as a prerequisite without caveats about definition instability.

---

## Notes

- This task is a prerequisite for trustworthy workflow bootstrap (task 8) and run persistence (task 16). Do not begin either of those tasks until this one is complete and all tests pass.
- ADR-021 (runtime-first hardening) explicitly calls out that runtime durability should not sit on top of definition inconsistency. Definition coherence is therefore Phase 0, not Phase 1.
- The existing `WorkflowId(Uuid)` newtype uses UUID-based identity. The file naming convention `{id}.json` is already established in `create_workflow`. The fix for update and delete must use the same convention.
- Do not introduce a database-backed definition store as part of this fix. ADR-037 is explicit: databases own runtime projections, not definition content.
