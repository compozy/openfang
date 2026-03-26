## markdown

## status: completed

<task_context>
<domain>engine/dispatch/runtime</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task12,task20,task23</dependencies>
</task_context>

# Task 29.0: Dispatch Runtime Integration With Provider-Native Sessions

## Overview

Wire the `agent_dispatch` persistence layer (task 23) into the live agent execution path so that
every workflow-relevant delegation becomes a durable runtime record backed by the provider's
native session model. This task is where the three dispatch modes — `call`, `send`, and `spawn` —
gain their actual execution semantics on top of the Arky provider layer, and where provider/session
identifiers are captured durably so that HITL continuation (task 30) and post-restart recovery
can resume execution without re-initializing from scratch.

Per ADR-009 (Persisted Agent Delegation and Lineage), durable workflows cannot rely on invisible
runtime side effects. The dispatch record must exist before execution begins, must carry enough
provider identity to reconstruct the execution context, and must be updated atomically as
execution progresses. This task makes that true for the current OpenFang-backed execution paths
and for the Arky-backed Claude Code and Codex provider paths.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Integrate dispatch record creation into the workflow engine's step executor. Before the agent
  loop begins for any `agent`-kind workflow step, a `DispatchRecord` must be inserted with
  `status = pending` and the resolved `input_json`. The step executor must hold the `dispatch_id`
  in its execution context and use it for all subsequent status updates.
- Transition the dispatch record to `status = running` immediately after the provider session is
  established and execution begins. This transition must be durable — if the process crashes after
  the transition, the record correctly reflects that execution was in progress (task 19's restart
  recovery will then downgrade it to `paused`).
- Populate the provider identity columns added in task 23 (`provider_driver`, `session_id`,
  `provider_resume_token`) from the live provider session. For Arky-backed providers, extract the
  `SessionId` from `arky_session::SessionStore` immediately after session creation and write it to
  the dispatch record. For the existing OpenFang LLM driver path, record at minimum the
  `provider_driver` string and the session ID from `openfang_memory::session::Session`.
- Implement the three dispatch mode semantics at the runtime layer:
  - `call` — invoke the agent, await the result, write `result_json`, transition to `completed`.
    The workflow step waits synchronously for the result before advancing.
  - `send` — invoke the agent, return a dispatch ID immediately without waiting for the result.
    The dispatch record remains `running` and completes asynchronously. The workflow step advances
    without waiting.
  - `spawn` — create or locate a long-lived agent instance, write `spawned_agent_id` to the
    dispatch record, and return. The spawned agent runs independently; the dispatch record
    captures its stable identity for lineage tracking.
- On dispatch completion, write `result_json` and transition to `status = completed` in a single
  transaction. On failure, write `error_json` and transition to `status = failed`. Neither
  transition must be a partial write — the JSON and the status must change atomically.
- Invalid provider bindings or missing agent definitions must produce a `status = failed` dispatch
  record with a descriptive `error_json`, not a panic or an unrecoverable runtime error.
- The dispatch integration must not bypass the `ProviderBinding` layer introduced in task 11.
  All provider-specific setup (session creation, model selection, config) goes through
  `ProviderBinding`; the dispatch layer only receives the resulting `SessionId` and records it.
</requirements>

## Subtasks

- [x] 29.1 Identify the exact call site in the workflow engine where a step's agent dispatch
      currently begins. In `crates/openfang-kernel/src/workflow.rs` and the surrounding step executor
      code, locate where `AgentManifest` is resolved and execution is handed to the agent loop. This
      is the injection point for dispatch record creation.

- [x] 29.2 Modify the step executor to create a `DispatchRecord` with `status = pending` before
      any provider interaction begins. The dispatch kind (`call`, `send`, or `spawn`) must be resolved
      from the workflow step definition. Store the `dispatch_id` in the step's execution context so it
      survives across the `await` boundary into provider setup.

- [x] 29.3 After the provider session is established via `ProviderBinding`, transition the dispatch
      record to `status = running` and write `provider_driver`, `session_id`, and
      `provider_resume_token` (if available from the provider) to the record. For Arky-backed
      providers, extract the `SessionId` from `arky_session::SessionStore` after the session is
      created or resumed in `crates/arky-session/src/sqlite.rs`.

- [x] 29.4 Implement the `call` execution path: await the agent loop result, write `result_json`
      and transition to `completed` on success, write `error_json` and transition to `failed` on
      error. Both writes must be transactional.

- [x] 29.5 Implement the `send` execution path: spawn the agent execution as a background
      `tokio::task`, return the dispatch ID to the caller immediately, and wire the background task to
      write the final result or error when it completes. The dispatch record must not be left
      permanently in `running` state if the background task fails.

- [x] 29.6 Implement the `spawn` execution path: resolve or create the long-lived agent identity,
      write `spawned_agent_id` to the dispatch record, transition to `completed`, and return. The
      spawned agent's ongoing execution is tracked separately through its own future dispatches.

- [x] 29.7 Add integration tests covering all three dispatch modes and both provider paths (Arky
      and existing OpenFang LLM driver). Confirm that session identity is captured durably in the
      dispatch record and survives a simulated connection restart.

## Implementation Details

The current execution flow in the workflow engine is roughly:

1. `WorkflowEngine::advance_run` selects the current step from the workflow definition.
2. For `agent` kind steps, it resolves the `AgentManifest` from the kernel registry.
3. It invokes `agent_loop::run` (in `crates/openfang-runtime/src/agent_loop.rs`) with the
   manifest, session, and input.
4. The result is stored as a workflow variable and the run advances to the next step.

After this task, the flow becomes:

1. `WorkflowEngine::advance_run` selects the current step.
2. It creates a `DispatchRecord` with `status = pending` via `DispatchRepository::create`.
3. It resolves the `ProviderBinding` from the compiled agent definition (task 11).
4. It establishes or resumes the provider session; extracts and persists the `SessionId`.
5. It transitions the dispatch record to `status = running`.
6. For `call`: awaits completion, writes result, transitions to `completed`.
   For `send`: spawns background task, returns `dispatch_id` immediately.
   For `spawn`: registers long-lived agent, writes `spawned_agent_id`, transitions to `completed`.

The provider session identity is the key piece for task 30's HITL resume. When a dispatch
transitions to `waiting_hitl`, the runtime must later re-enter the provider session at exactly
the same point. For Arky-backed providers, the `SessionId` from `arky_session` is stable and
survives process restarts when backed by `SqliteSessionStore` (see
`crates/arky-session/src/sqlite.rs`). For the existing OpenFang LLM driver, the session in
`openfang_memory::session::Session` provides continuity within a canonical session.

The `provider_resume_token` column is a nullable escape hatch for providers that require an
opaque token (distinct from the session ID) to resume mid-stream. Claude Code's
`continue_conversation` flag and Codex's `resume_last` config are examples of provider-specific
resume signals. The dispatch record stores whatever the provider returns as a resume hint; the
runtime in task 30 reads it back when constructing the continuation request.

The step executor must be careful not to hold any lock across the provider `await` — this is the
existing rule in `CLAUDE.md` (never hold locks across `.await` points). The `dispatch_id` must
be captured before the await and used after it. The `DispatchRepository` must be `Arc`-wrapped
so it can be shared across the async step execution context.

For the `send` dispatch mode, the background task must be tracked. A `JoinSet` stored on the
workflow engine or kernel (per `CLAUDE.md`'s async conventions) is the right container. If the
process shuts down while `send` tasks are in flight, they must be cancelled cooperatively via
`CancellationToken` rather than left as orphaned threads.

### Relevant Files

- `crates/openfang-kernel/src/workflow.rs` — step executor to be modified
- `crates/openfang-runtime/src/agent_loop.rs` — agent execution entry point
- `crates/arky-session/src/sqlite.rs` — SQLite-backed session store for Arky providers
- `crates/arky-session/src/store.rs` — `SessionStore` trait, `SessionId` type
- `crates/arky-provider/src/traits.rs` — `Provider` trait and `ProviderRequest`
- `crates/arky-provider/src/request.rs` — `SessionRef`, `TurnContext` types
- `crates/arky-claude-code/src/` — Claude Code provider; check for session resume fields
- `crates/arky-codex/src/` — Codex provider; check for `resume_last` config
- task 23 dispatch repository — `DispatchRepository` and `DispatchRecord`
- `tasks/prd-compozy/docs/API-SPEC.md` section 10 — dispatch detail shape showing
  `kind`, `status`, and provider-identity fields

### Dependent Files

- task 30: HITL pause/resume reads `session_id` and `provider_resume_token` from this record
- task 33: API handlers expose live dispatch state via `/api/v1/dispatches`
- task 19 recovery: `running` records left at restart are downgraded; this task's records will be
  caught by that recovery scan if it runs after the version including this task

## Deliverables

- Dispatch record creation integrated into the workflow step executor before any provider interaction
- Session identity (`provider_driver`, `session_id`, `provider_resume_token`) written to the
  dispatch record after session establishment
- All three dispatch mode execution paths (`call`, `send`, `spawn`) implemented with correct
  durable status transitions
- Atomic result/error writes on completion or failure
- Integration tests for all three modes and both provider paths
- Zero-warning `cargo fmt`, `cargo clippy`, and `cargo test` results

## Tests

### Unit Tests (Required)

- [x] `dispatch_record_should_have_session_id_after_provider_setup` — after the step executor
      establishes a provider session, the dispatch record in the database must have a non-null
      `session_id` and `provider_driver`.
- [x] `dispatch_call_should_complete_with_result_json` — run a `call` dispatch against a test
      double provider and verify the dispatch record ends in `status = completed` with `result_json`
      populated.
- [x] `dispatch_call_should_fail_with_error_json_on_provider_error` — simulate a provider error
      and verify the dispatch record ends in `status = failed` with a non-null `error_json`.
- [x] `dispatch_send_should_return_immediately_with_running_status` — invoke a `send` dispatch
      and verify the caller receives the `dispatch_id` before the background task completes; the
      record is `running` at that point.
- [x] `dispatch_spawn_should_write_spawned_agent_id` — invoke a `spawn` dispatch and verify the
      dispatch record has a non-null `spawned_agent_id` after completion.
- [x] `dispatch_invalid_provider_binding_should_fail_cleanly` — pass a malformed or missing
      agent binding and verify the dispatch record transitions to `failed` with a descriptive error,
      not a panic.

### Integration Tests (Required)

- [x] `dispatch_call_end_to_end_with_arky_provider` — run a full `call` dispatch against the
      Arky provider test infrastructure; verify the dispatch record is `completed` with session
      identity populated.
- [x] `dispatch_call_end_to_end_with_openfang_llm_driver` — run a full `call` dispatch against
      the existing OpenFang LLM driver; verify the dispatch record is `completed` with at least
      `provider_driver` populated.
- [x] `dispatch_session_identity_survives_reconnect` — write a dispatch record with `session_id`,
      simulate a connection drop and reconnect, and verify the session identity is still present and
      the Arky `SessionStore` can load the session by that ID.
- [x] `dispatch_send_background_task_should_complete_and_update_record` — invoke a `send`
      dispatch with a fast test provider, wait for the background task to finish, and verify the
      record transitions to `completed`.
- [x] `dispatch_concurrent_call_dispatches_should_not_interfere` — run two `call` dispatches for
      different agents concurrently from the same workflow run and verify both records end in
      `completed` with independent results.

### Regression and Anti-Pattern Guards

- [x] Do not route durable dispatch through raw provider calls that bypass `ProviderBinding` —
      the provider identity must come from the compiled binding, not from manifest fields read
      directly in the step executor.
- [x] Do not fake session resume by constructing a new session with a copied transcript instead
      of using the stored `SessionId` — task 30 depends on true session continuity.
- [x] Do not keep dispatch runtime state only in provider-local memory — every status transition
      must be written to `compozy.db` before execution resumes.
- [x] Do not hold a `Mutex` or `RwLock` across a `.await` point in the step executor — per
      `CLAUDE.md` and Rust async conventions.
- [x] Do not let `send` mode dispatches become orphaned on process shutdown — wire
      `CancellationToken` cancellation to the `JoinSet` tracking background tasks.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- Every workflow `agent` step creates a `DispatchRecord` in `compozy.db` before execution begins.
- The dispatch record contains non-null `provider_driver` and `session_id` after session setup.
- All three dispatch modes (`call`, `send`, `spawn`) execute with the correct runtime semantics
  and produce the correct terminal dispatch status.
- Result and error writes are transactional — no partial writes where `result_json` is present
  but `status` is still `running`.
- The Arky `SessionId` written to the dispatch record is loadable by `arky_session::SessionStore`
  after a simulated restart — confirming the identity is durable, not in-memory only.
- Task 30 can begin immediately without any additional schema or execution wiring.
- All `cargo fmt`, `cargo clippy`, and `cargo test` checks pass at zero warnings.

---

## Prior Implementation Reference

The old TypeScript codebase shows how provider sessions were managed during dispatch:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/` — Provider tool adapters showing session patterns
- `~/Dev/compozy/compozy-code/providers/runtime/src/session/` — Session management in the old OpenResponses runtime
- `~/Dev/compozy/compozy-code/providers/runtime/src/protocol/` — OpenResponses protocol handling

The old runtime kept session identity in the provider layer. The new model must persist session/provider
identifiers durably in `agent_dispatch` so that resume and HITL continuation work across restarts.

## Notes

- This task is why provider work (tasks 12, 16, 18) must land before full dispatch/HITL integration.
- The `send` mode creates background tasks that outlive the step executor's call frame. Use
  `tokio::spawn` with a handle stored in a `JoinSet` on the kernel or workflow engine — do not
  use `tokio::spawn` with a fire-and-forget pattern that cannot be cancelled on shutdown.
- The `spawn` mode is the foundation for multi-agent compositions where one agent spawns a
  subordinate that has a lifetime independent of the triggering workflow run. The `spawned_agent_id`
  in the dispatch record is what makes those subordinate agents traceable.
