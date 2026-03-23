## markdown

## status: pending

<task_context>
<domain>api/agents/runtime</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task20</dependencies>
</task_context>

# Task 21.0: Agent Runtime Operational Sub-Resources

## Overview

Implement the runtime operational sub-resource route handlers for agents under
the `/api/v1/agents/{id}/` prefix. This task builds on the definition CRUD
surface from Task 20 and adds the runtime lifecycle and session management
endpoints specified in ADR-030 and API-SPEC.md section 3.

The operational sub-resources allow callers to manage an agent's runtime state
(start, stop, restart, change mode) and sessions (create, list, activate, reset,
compact) without modifying the stored definition. This decoupling is a core
principle from ADR-023 and ADR-030: definition mutations must not implicitly
start or stop a runtime, and runtime actions must not modify the stored
definition.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Operational sub-resources (`/runtime`, `/runtime/start`, `/runtime/stop`, `/runtime/restart`, `/runtime/mode`, `/sessions`, `/sessions/{session_id}`, `/sessions/{session_id}/activate`, `/sessions/{session_id}/reset`, `/sessions/{session_id}/compact`) must be implemented as distinct route handlers and registered in `crates/openfang-api/src/server.rs` under the `/api/v1/agents/{id}/` prefix (ADR-030).
- Runtime sub-resource handlers must delegate to the existing kernel agent lifecycle methods (`set_state`, `set_mode` on `AgentRegistry`) and return the `API-SPEC.md` runtime resource shape.
- Session sub-resource handlers must store and retrieve session state from the kernel or a dedicated session store, not reconstructed from agent registry state alone.
- Runtime actions must not modify the stored definition (ADR-023, ADR-030).
- All error responses must use the `{ error: { code, message, details } }` envelope from `API-SPEC.md` (ADR-034).
- All list responses must use `{ items, next_cursor }` shape (ADR-034).
</requirements>

## Subtasks

- [ ] 21.1 Add new request and response types to `crates/openfang-api/src/types.rs` for runtime and session sub-resources: `AgentRuntimeResponse`, `RuntimeModeRequest`, `SessionListItem`, `SessionDetail`, `CreateSessionRequest`. All structs must match the `API-SPEC.md` payload shapes exactly.
- [ ] 21.2 Implement runtime sub-resource route handlers: `get_agent_runtime`, `start_agent_runtime`, `stop_agent_runtime`, `restart_agent_runtime`, `set_agent_runtime_mode`. These must delegate to the existing kernel agent lifecycle methods (`set_state`, `set_mode` on `AgentRegistry`) and return the `API-SPEC.md` runtime resource shape.
- [ ] 21.3 Implement session sub-resource route handlers: `list_agent_sessions`, `create_agent_session`, `get_agent_session`, `activate_agent_session`, `reset_agent_session`, `compact_agent_session`. Session state must be stored in and retrieved from the kernel or a dedicated session store.
- [ ] 21.4 Register all runtime and session routes in `crates/openfang-api/src/server.rs` under the `/api/v1/agents/{id}/` prefix.
- [ ] 21.5 Write unit and integration tests for all runtime and session sub-resource endpoints.

## Implementation Details

### Route Registration

All routes must be registered in `crates/openfang-api/src/server.rs` inside
the `build_router` function under the `/api/v1/agents/{id}/` prefix:

```
GET    /api/v1/agents/{id}/runtime           -> get_agent_runtime
POST   /api/v1/agents/{id}/runtime/start     -> start_agent_runtime
POST   /api/v1/agents/{id}/runtime/stop      -> stop_agent_runtime
POST   /api/v1/agents/{id}/runtime/restart   -> restart_agent_runtime
PUT    /api/v1/agents/{id}/runtime/mode      -> set_agent_runtime_mode
GET    /api/v1/agents/{id}/sessions          -> list_agent_sessions
POST   /api/v1/agents/{id}/sessions          -> create_agent_session
GET    /api/v1/agents/{id}/sessions/{sid}    -> get_agent_session
POST   /api/v1/agents/{id}/sessions/{sid}/activate  -> activate_agent_session
POST   /api/v1/agents/{id}/sessions/{sid}/reset     -> reset_agent_session
POST   /api/v1/agents/{id}/sessions/{sid}/compact   -> compact_agent_session
```

### Runtime Resource Shape

The `get_agent_runtime` handler must return the runtime resource shape with
at minimum these fields:
- `loaded`: whether the agent definition is loaded into the runtime
- `state`: current runtime state (e.g., `running`, `stopped`, `error`)
- `mode`: current agent mode (e.g., `autonomous`, `supervised`)
- `healthy`: boolean health indicator
- `active_sessions`: count of active sessions
- `active_dispatches`: count of active dispatches

### Session Resource Shape

Session list items must include at minimum: `session_id`, `created_at`,
`updated_at`, `active`, `message_count`. Session detail must additionally
include the full conversation context if requested.

### Integration Points

- `openfang_kernel::registry::AgentRegistry`: used by runtime sub-resource handlers for `set_state`, `set_mode`, `list`, `get`.
- `API-SPEC.md` section 3: canonical payload shapes for runtime and session resources.
- `ADR-030`: defines the full set of required route paths.
- `ADR-023`: governs which internal surfaces are reused directly (`AgentRegistry.set_state`, `set_mode`).
- `ADR-034`: all list responses must use `{ items, next_cursor }`, all error responses must use `{ error: { code, message, details } }`.

### Relevant Files

- `crates/openfang-api/src/routes.rs` (existing — add new handlers)
- `crates/openfang-api/src/server.rs` (existing — register new routes in build_router)
- `crates/openfang-api/src/types.rs` (existing — add new request/response types)
- `crates/openfang-kernel/src/registry.rs` (existing — read for state/mode operations)
- `tasks/prd-compozy/docs/API-SPEC.md` (section 3 — agent resource, runtime and session sub-resources)
- `tasks/prd-compozy/docs/adrs/030-agent-api-definition-and-operational-surfaces.md`
- `tasks/prd-compozy/docs/adrs/023-public-api-exposure-rules.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`

## Deliverables

- All runtime sub-resource handlers (runtime status, start, stop, restart, mode).
- All session sub-resource handlers (list, create, get, activate, reset, compact).
- All routes registered in `server.rs` under `/api/v1/agents/{id}/`.
- Request and response types in `crates/openfang-api/src/types.rs`.
- Full test suite covering runtime lifecycle and session management flows.

## Tests

### Unit Tests (Required)

- [ ] `get_agent_runtime` returns the runtime resource shape with `loaded`, `state`, `mode`, `healthy`, `active_sessions`, `active_dispatches` fields.
- [ ] `start_agent_runtime` returns `{ accepted: true, resource_id: "...", status: "accepted" }` for a valid agent ID.
- [ ] `set_agent_runtime_mode` returns `400` when given an unknown mode string not in the `AgentMode` enum.
- [ ] `list_agent_sessions` returns `{ items: [...], next_cursor: null }` shape.
- [ ] `create_agent_session` returns a session detail with a generated `session_id`.
- [ ] `get_agent_session` returns `404` for an unknown session ID.

### Integration Tests (Required)

- [ ] An agent runtime lifecycle sequence — `start`, `get runtime`, `stop` — results in consistent `state` values in the runtime resource responses across the three calls.
- [ ] Session lifecycle: `create_session`, `list_sessions` includes the new session, `activate_session` changes the active flag, `reset_session` clears session state.
- [ ] Definition endpoints (`PUT /api/v1/agents/{id}` from Task 20) must not have observable side effects on live runtime state — confirmed by asserting that `get_agent_runtime` state is unchanged after a `PUT` call.

### Regression and Anti-Pattern Guards

- [ ] No runtime handler may modify the stored agent definition.
- [ ] All error responses must use the `{ error: { code, message, details } }` envelope.
- [ ] No internal agent side channels: runtime operations must not bypass the public API contract to apply privileged mutations not available to external callers (ADR-023).

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All runtime and session routes listed in ADR-030 are registered in `server.rs` and return non-`404` responses for valid inputs.
- Runtime lifecycle operations (start, stop, restart, mode change) correctly delegate to kernel methods.
- Session operations (create, list, get, activate, reset, compact) correctly manage session state.
- Runtime actions do not modify stored definitions.
- All error responses include `code`, `message`, and `details`.
- Zero clippy warnings, zero test failures.

---

## Notes

- This task should land after Task 20 (definition CRUD) and before Task 22 (messages and SSE streaming).
- CLI agent surfaces (`openfang-cli`) are out of scope and will be addressed as future work.
- Live integration testing (starting the daemon and curling real endpoints) is mandatory per `CLAUDE.md` before marking this task complete.
