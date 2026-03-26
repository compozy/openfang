## markdown

## status: completed

<task_context>
<domain>api/agents/streaming</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task21</dependencies>
</task_context>

# Task 22.0: Agent Sessions Messages And SSE Streaming

## Overview

Implement the message submission, SSE streaming, and dry-run endpoints for
agents under the `/api/v1/agents/{id}/` prefix. This task builds on the runtime
and session sub-resources from Task 21 and adds the message dispatch and
streaming capabilities specified in ADR-030 and API-SPEC.md section 3.

The message endpoints allow callers to submit messages to an agent (triggering
actual LLM dispatch), stream responses via Server-Sent Events (SSE), and
perform dry-run simulations without dispatching. These are the primary
interaction surfaces for agent conversations.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- `POST /api/v1/agents/{id}/messages` must accept a message request and dispatch it to the agent's execution loop, returning `{ accepted, session_id, message_id }`.
- `POST /api/v1/agents/{id}/messages/stream` must initiate an SSE connection and follow the event naming convention from `API-SPEC.md` section 2: `message.delta`, `message.completed`, `tool.started`, `tool.completed`, `error`, `keepalive`.
- `POST /api/v1/agents/{id}/messages/dry-run` must return `{ would_execute, resolved, effects, explanation }` without dispatching any actual message (ADR-038).
- The `stream` handler must use SSE (Server-Sent Events) and must not return a plain JSON response.
- All error responses must use the `{ error: { code, message, details } }` envelope from `API-SPEC.md` (ADR-034).
- Message handlers must interact with the agent execution loop in `crates/openfang-runtime/src/agent_loop.rs` for actual dispatch.
</requirements>

## Subtasks

- [x] 22.1 Add new request and response types to `crates/openfang-api/src/types.rs` for message endpoints: `MessageRequest`, `MessageResponse`, `MessageDryRunResponse`, `StreamEvent`. All structs must match the `API-SPEC.md` payload shapes exactly.
- [x] 22.2 Implement `submit_agent_message` route handler: accepts a message request, dispatches to the agent execution loop, and returns the accepted response with `session_id` and `message_id`.
- [x] 22.3 Implement `stream_agent_message` route handler using SSE: initiates an SSE connection, streams events (`message.delta`, `message.completed`, `tool.started`, `tool.completed`, `error`, `keepalive`) as the agent processes the message. Must use Axum's SSE support.
- [x] 22.4 Implement `dry_run_agent_message` route handler: resolves the definition and session context without dispatching a message, and returns `{ would_execute, resolved, effects, explanation }`.
- [x] 22.5 Register all message routes in `crates/openfang-api/src/server.rs` under the `/api/v1/agents/{id}/messages` prefix.
- [x] 22.6 Write unit and integration tests for all message endpoints including SSE streaming verification.

## Implementation Details

### Route Registration

All routes must be registered in `crates/openfang-api/src/server.rs` inside
the `build_router` function:

```
POST   /api/v1/agents/{id}/messages           -> submit_agent_message
POST   /api/v1/agents/{id}/messages/stream    -> stream_agent_message
POST   /api/v1/agents/{id}/messages/dry-run   -> dry_run_agent_message
```

### SSE Event Convention

The SSE stream must follow the event naming convention from API-SPEC.md
section 2:

- `message.delta` — incremental text chunks from the LLM response
- `message.completed` — final complete message with full content
- `tool.started` — a tool call has been initiated
- `tool.completed` — a tool call has completed with results
- `error` — an error occurred during processing
- `keepalive` — periodic heartbeat to keep the connection alive

Each SSE event must be formatted as:
```
event: <event_type>
data: <json_payload>

```

### Dry-Run Semantics

Per ADR-038, the `dry-run` handler must:
- Resolve the agent definition and compile it.
- Resolve the session context (active session, conversation history).
- Determine what would be executed (provider, model, tools).
- Return the resolution results without making any provider calls.
- The response shape is: `{ would_execute: bool, resolved: { provider, model, tools, session }, effects: { estimated_tokens, estimated_cost }, explanation: { steps } }`.

### Integration Points

- `openfang_runtime::agent_loop`: the agent execution loop that handles actual message dispatch. The `submit_agent_message` handler must dispatch into this loop. The `stream_agent_message` handler must connect to the loop's output stream.
- `openfang_kernel::registry::AgentRegistry`: used to look up agent state and validate that the agent exists and is in a runnable state.
- Session store from Task 21: used to retrieve and update session context for message dispatch.
- `API-SPEC.md` section 2: SSE event naming convention.
- `API-SPEC.md` section 3: message endpoint payload shapes.
- `ADR-030`: defines the full set of required route paths.
- `ADR-038`: defines the semantics of `dry-run`.
- `ADR-034`: payload conventions for all responses.

### Relevant Files

- `crates/openfang-api/src/routes.rs` (existing — add new handlers)
- `crates/openfang-api/src/server.rs` (existing — register new routes in build_router)
- `crates/openfang-api/src/types.rs` (existing — add new request/response types)
- `crates/openfang-runtime/src/agent_loop.rs` (existing — dispatch point for message submission)
- `tasks/prd-compozy/docs/API-SPEC.md` (sections 2 and 3 — SSE convention and agent message endpoints)
- `tasks/prd-compozy/docs/adrs/030-agent-api-definition-and-operational-surfaces.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`

### Dependent Files

- `crates/openfang-cli/` (future work — CLI surfaces are out of scope for this task)
- `crates/openfang-desktop/` (reads from the API; no changes required here)

## Deliverables

- `submit_agent_message` handler for synchronous message dispatch.
- `stream_agent_message` handler with full SSE support.
- `dry_run_agent_message` handler for simulation without dispatch.
- All routes registered in `server.rs` under `/api/v1/agents/{id}/messages`.
- Request and response types in `crates/openfang-api/src/types.rs`.
- Full test suite covering message submission, SSE streaming, and dry-run flows.

## Tests

### Unit Tests (Required)

- [x] `submit_agent_message` returns `{ accepted: true, session_id, message_id }` for a valid request.
- [x] `dry_run_agent_message` returns `{ would_execute: true, resolved: {...}, effects: {...}, explanation: {...} }` without dispatching any message.
- [x] `submit_agent_message` returns `404` for an unknown agent ID.
- [x] `submit_agent_message` returns an error when the agent runtime is not started.
- [x] `dry_run_agent_message` works even when the agent runtime is stopped (it only resolves, does not dispatch).

### Integration Tests (Required)

- [x] `POST /api/v1/agents/{id}/messages/stream` initiates an SSE response with at least a `keepalive` event and does not return a plain JSON response.
- [x] Full message lifecycle: submit a message, verify the response includes a `message_id`, then verify the session's message count has increased.
- [x] SSE stream includes `message.delta` and `message.completed` events for a real agent dispatch (requires a running agent with a configured provider).
- [x] Dry-run returns resolved provider and model information without triggering any actual LLM call.

### Regression and Anti-Pattern Guards

- [x] The `stream` handler must never return a plain JSON response — it must always use SSE content type (`text/event-stream`).
- [x] Message handlers must not modify the stored agent definition.
- [x] All error responses must use the `{ error: { code, message, details } }` envelope.
- [x] No message handler may bypass the session store to dispatch messages without session context.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- All message routes are registered in `server.rs` and return non-`404` responses for valid inputs.
- `POST /api/v1/agents/{id}/messages` correctly dispatches to the agent execution loop.
- `POST /api/v1/agents/{id}/messages/stream` produces a valid SSE stream with the correct event types.
- `POST /api/v1/agents/{id}/messages/dry-run` returns resolution results without dispatching.
- All error responses include `code`, `message`, and `details`.
- Zero clippy warnings, zero test failures.

---

## Notes

- This task should land after Task 21 (runtime operational sub-resources) since message dispatch depends on runtime state and session management.
- The SSE implementation should use Axum's built-in SSE support (`axum::response::sse::Event`, `axum::response::sse::Sse`).
- Live integration testing (starting the daemon and curling real endpoints) is mandatory per `CLAUDE.md` before marking this task complete.
- CLI agent surfaces (`openfang-cli`) are out of scope and will be addressed as future work.
