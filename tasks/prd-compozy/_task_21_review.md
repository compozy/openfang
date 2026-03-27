# Task 21 Review: Agent Runtime Operational Sub-Resources

## Status: PASS

## Checklist
- [x] `AgentRuntimeResponse` type in `types.rs` with `loaded`, `state`, `mode`, `healthy`, `active_sessions`, `active_dispatches` fields — present
- [x] `RuntimeModeRequest` type — present
- [x] `SessionListItem`, `SessionDetail`, `CreateSessionRequest` types — present
- [x] `get_agent_runtime` handler (`GET /api/v1/agents/{id}/runtime`) — implemented, returns runtime resource shape
- [x] `start_agent_runtime` handler (`POST /api/v1/agents/{id}/runtime/start`) — returns accepted response with `resource_id`
- [x] `stop_agent_runtime` handler (`POST /api/v1/agents/{id}/runtime/stop`) — implemented
- [x] `restart_agent_runtime` handler (`POST /api/v1/agents/{id}/runtime/restart`) — implemented
- [x] `set_agent_runtime_mode` handler (`PUT /api/v1/agents/{id}/runtime/mode`) — returns 400 for unknown mode
- [x] `list_agent_sessions_v1` handler (`GET /api/v1/agents/{id}/sessions`) — returns `{ items, next_cursor }` shape
- [x] `create_agent_session_v1` handler (`POST /api/v1/agents/{id}/sessions`) — returns session detail with generated `session_id`
- [x] `get_agent_session_v1` handler (`GET /api/v1/agents/{id}/sessions/{session_id}`) — returns 404 for unknown session
- [x] `activate_agent_session` handler (`POST /api/v1/agents/{id}/sessions/{session_id}/activate`) — implemented
- [x] `reset_agent_session` handler (`POST /api/v1/agents/{id}/sessions/{session_id}/reset`) — implemented
- [x] `compact_agent_session_v1` handler (`POST /api/v1/agents/{id}/sessions/{session_id}/compact`) — implemented
- [x] All routes registered in `server.rs` under `/api/v1/agents/{id}/` prefix
- [x] Runtime handlers delegate to `AgentRegistry.set_state` and `set_mode` — confirmed by implementation
- [x] Runtime actions do not modify stored definition — test `put_agent_should_not_change_runtime_state_observable_via_runtime_endpoint` confirms
- [x] All error responses use `{ error: { code, message, details } }` envelope — confirmed by inline tests
- [x] Unit tests: `get_agent_runtime` shape, `start_agent_runtime` accepted response, `set_agent_runtime_mode` 400 for bad mode, `list_agent_sessions` shape, `create_agent_session` with session_id, `get_agent_session` 404 — all present as inline tests in `routes.rs`
- [x] Integration tests: runtime lifecycle sequence (start→get→stop consistent states), session lifecycle (create→list→activate→reset), definition PUT does not affect runtime state — all present in `agent_v2_api_test.rs`

## Findings
- All deliverables fully implemented. The runtime sub-resources correctly delegate to `AgentRegistry` kernel methods.
- Session state is stored and retrieved via the kernel's session store, not reconstructed from registry state alone.
- The `compact_agent_session_v1` handler name uses the `_v1` suffix consistently.
- The anti-pattern guard (no runtime handler modifies the stored definition) is verified by `put_agent_should_not_change_runtime_state_observable_via_runtime_endpoint`.
- All list responses use the `{ items, next_cursor }` shape as required by ADR-034.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/types.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/routes.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/src/server.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-api/tests/agent_v2_api_test.rs`
