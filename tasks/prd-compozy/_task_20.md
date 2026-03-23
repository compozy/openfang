## markdown

## status: pending

<task_context>
<domain>api/agents/crud</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task18</dependencies>
</task_context>

# Task 20.0: Agent Definition CRUD And Compile Routes

## Overview

Wire the agent definition compile pipeline (task 18) into the public
`/api/v1/agents` surface for definition-management operations. This task
implements the CRUD endpoints (create, read, update, delete) and the
validate/compile endpoints for agent definitions, aligning all definition-facing
route handlers in `crates/openfang-api/src/routes.rs` with the new
definition-first model from ADR-029 and the payload conventions from ADR-034.

This task replaces the current "spawn from raw manifest TOML blob" model
(`POST /api/agents` with `manifest_toml: String`) with a structured
definition-first API that accepts and returns the `AgentDefinition` shape from
ADR-029, validates and compiles through the task 18 pipeline, and returns
structured responses per API-SPEC.md section 3.

The public API surface for definition management is fully specified in
`API-SPEC.md` section 3 and ADR-030. ADR-023 governs which existing internal
surfaces to reuse directly, which to wrap with an adapter, and which to replace.
ADR-038 governs the semantics of `validate`, `compile`, and `dry-run` endpoints.

Runtime operational sub-resources are handled in Task 21. Session and message
endpoints are handled in Task 22.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- All definition-facing endpoints (`GET`, `POST`, `PUT`, `DELETE` on `/api/v1/agents` and `/api/v1/agents/{id}`) must accept and return the `AgentDefinition` shape from ADR-029, not the legacy `manifest_toml`-based request shape (ADR-023, ADR-030).
- `POST /api/v1/agents/validate` must run the full four-stage validation pipeline from task 18 and return the `API-SPEC.md` validation response shape: `{ valid, issues, normalized }` (ADR-038, ADR-034).
- `POST /api/v1/agents/compile` and `GET /api/v1/agents/{id}/compiled` must run validation and compile via the task 18 pipeline and return `{ definition_id, normalized, compiled: { agent_manifest, provider_binding, product_metadata } }` (ADR-038, ADR-030).
- Definition endpoints must not implicitly start or stop a runtime — definition mutations and runtime actions must be clearly decoupled (ADR-023, ADR-030).
- All list and detail responses must include `origin` and `forked_from` fields per the `API-SPEC.md` definition origin metadata convention (ADR-034).
- All error responses must use the `{ error: { code, message, details } }` envelope from `API-SPEC.md`, with no unstructured plain-text error bodies (ADR-034).
- The internal `AppState` in `crates/openfang-api/src/routes.rs` must not hold a direct reference to the validation pipeline functions — route handlers must call the pipeline functions from the task 18 module by importing them; `AppState` provides only the kernel and shared state (ADR-023).
- All definition mutation responses (`create_agent`, `update_agent`) must return the full agent resource object, not just an ID.
</requirements>

## Subtasks

- [ ] 20.1 Add new request and response types to `crates/openfang-api/src/types.rs` for definition CRUD and compile endpoints: `CreateAgentRequest`, `UpdateAgentRequest`, `AgentResponse`, `AgentListItem`, `AgentValidateRequest`, `AgentValidateResponse`, `AgentCompileRequest`, `AgentCompileResponse`, `AgentCompiledResponse`. All structs must match the `API-SPEC.md` payload shapes exactly.
- [ ] 20.2 Implement the definition-management route handlers in `crates/openfang-api/src/routes.rs`: `list_agents`, `create_agent`, `get_agent`, `update_agent`, `delete_agent`. These handlers replace the existing `spawn_agent` handler for the new definition-first flow. The existing `spawn_agent` handler may be kept internally for backward-compatible agent boot but must not be promoted as the public definition API.
- [ ] 20.3 Implement `validate_agent_definition` and `compile_agent_definition` route handlers that call the task 18 pipeline functions (`stage1_schema_validate`, `stage2_reference_validate`, `stage3_semantic_validate`, `stage4_normalize`, `compile`). The `ValidationContext` passed to stage 2 must be constructed from live kernel state (agent registry snapshot, known skill names from the skill registry).
- [ ] 20.4 Implement `get_agent_compiled` route handler for `GET /api/v1/agents/{id}/compiled` that loads the stored definition, runs the full pipeline, and returns the compiled output without persisting it (compiled output is a derived artifact per ADR-040).
- [ ] 20.5 Register all definition CRUD and compile routes in `crates/openfang-api/src/server.rs` under the `/api/v1/agents` prefix. Verify that existing routes under `/api/agents` (the old non-versioned prefix) are either migrated or remain as legacy aliases. The new routes must live under `/api/v1/`.
- [ ] 20.6 Write unit and integration tests for all definition CRUD and compile endpoints.

## Implementation Details

### Current Codebase State

`crates/openfang-api/src/routes.rs` currently defines `AppState` with
`kernel: Arc<OpenFangKernel>`, `peer_registry`, `bridge_manager`,
`channels_config`, `shutdown_notify`, `clawhub_cache`, and
`provider_probe_cache`. The existing `spawn_agent` handler at `POST /api/agents`
accepts a `SpawnRequest { manifest_toml: String, template: Option<String> }`
and parses the TOML inline. This is the pattern that must be replaced for the
definition-first public surface.

`crates/openfang-api/src/types.rs` already defines `SpawnRequest` and a set of
response types for the existing routes. New types must be added here following
the same conventions (all types derive `Serialize`, `Deserialize`,
`Debug`, and use `#[serde(rename_all = "snake_case")]`).

`crates/openfang-api/src/server.rs` builds the Axum router via
`build_router(kernel, listen_addr)`. All new routes must be added to the router
inside `build_router`. The function returns `(Router<()>, Arc<AppState>)` so
the router is fully owned; callers must not need to add routes after
`build_router` returns.

### What Needs to Change

The public route surface must shift from:

```
POST /api/agents   { manifest_toml: "..." }   -> spawn from raw TOML
```

to:

```
POST   /api/v1/agents            { AgentDefinition }   -> create definition, validate, persist
GET    /api/v1/agents                                   -> list definitions
GET    /api/v1/agents/{id}                              -> get definition detail
PUT    /api/v1/agents/{id}       { AgentDefinition }   -> update definition
DELETE /api/v1/agents/{id}                              -> delete definition
POST   /api/v1/agents/validate   { AgentDefinition, strict, context }   -> validate only
POST   /api/v1/agents/compile    { AgentDefinition, context }            -> validate + compile
GET    /api/v1/agents/{id}/compiled                                       -> compile stored definition
```

The existing `/api/agents` route may remain as a legacy internal path for
backward compatibility during the transition but must not be documented as the
public product surface.

### Integration Points

- Task 18 pipeline functions (`stage1_schema_validate`, `stage2_reference_validate`, `stage3_semantic_validate`, `stage4_normalize`, `compile`): called directly from `validate_agent_definition` and `compile_agent_definition` handlers.
- `openfang_kernel::registry::AgentRegistry`: used by definition handlers for `register`, `get`, `find_by_name`, `remove`, `list`.
- `API-SPEC.md` section 3: canonical payload shapes for all agent endpoints.
- `ADR-030`: defines the full set of required route paths.
- `ADR-038`: defines the semantics of `validate`, `compile`, and `dry-run`.
- `ADR-023`: governs which internal surfaces are reused directly versus wrapped versus replaced.
- `ADR-034`: all list responses must use `{ items, next_cursor }`, all validation responses must use `{ valid, issues, normalized }`, all compile responses must use `{ definition_id, normalized, compiled }`, and all error responses must use `{ error: { code, message, details } }`.

### Relevant Files

- `crates/openfang-api/src/routes.rs` (existing — add new handlers, replace spawn_agent for public surface)
- `crates/openfang-api/src/server.rs` (existing — register new routes in build_router)
- `crates/openfang-api/src/types.rs` (existing — add new request/response types)
- `crates/openfang-kernel/src/registry.rs` (existing — read for state/mode operations)
- `tasks/prd-compozy/docs/API-SPEC.md` (section 3 — agent resource, all endpoint shapes)
- `tasks/prd-compozy/docs/adrs/030-agent-api-definition-and-operational-surfaces.md`
- `tasks/prd-compozy/docs/adrs/023-public-api-exposure-rules.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`

### Dependent Files

- `crates/openfang-cli/` (future work — CLI surfaces are out of scope for this task)
- `crates/openfang-desktop/` (reads from the API; no changes required here)

## Deliverables

- All definition-facing agent route handlers updated to use the `AgentDefinition` shape.
- `validate` and `compile` endpoint handlers wired to the task 18 pipeline.
- All routes registered in `server.rs` under `/api/v1/agents`.
- Request and response types in `crates/openfang-api/src/types.rs`.
- Full test suite covering definition CRUD flows and validation/compile flows.

## Tests

### Unit Tests (Required)

- [ ] `validate_agent_definition` handler returns `{ valid: true, issues: [], normalized: {...} }` when given a valid `AgentDefinition` JSON body matching the PRD writer example from `API-SPEC.md`.
- [ ] `validate_agent_definition` handler returns `{ valid: false, issues: [...] }` with at least one issue when given a definition with a missing required `provider.driver` field.
- [ ] `compile_agent_definition` handler returns a response with `compiled.agent_manifest`, `compiled.provider_binding`, and `compiled.product_metadata` all populated for a valid input.
- [ ] `get_agent_compiled` returns `404` for an unknown agent ID.
- [ ] All definition mutation responses (`create_agent`, `update_agent`) return the full agent resource object, not just an ID.

### Integration Tests (Required)

- [ ] Full create-validate-compile-get flow: `POST /api/v1/agents` creates a definition, `POST /api/v1/agents/validate` validates it, `POST /api/v1/agents/compile` compiles it, and `GET /api/v1/agents/{id}/compiled` returns the compiled form — all with consistent `definition_id`.
- [ ] `GET /api/v1/agents` returns a list response in `{ items: [...], next_cursor: null }` shape with at least the `id`, `name`, `enabled`, `group`, `tags`, `provider`, `origin`, `runtime_status`, `updated_at` fields on each list item.
- [ ] `DELETE /api/v1/agents/{id}` followed by `GET /api/v1/agents/{id}` returns `404`.
- [ ] `PUT /api/v1/agents/{id}` with a changed `name` field updates the agent and returns the updated definition with the new name.

### Regression and Anti-Pattern Guards

- [ ] The old `POST /api/agents` handler with `manifest_toml: String` must not be reachable under the new `/api/v1/agents` prefix — confirmed by asserting that a request with `manifest_toml` in the body to `POST /api/v1/agents` returns `422 Unprocessable Entity` or `400`.
- [ ] Definition endpoints (`POST`, `PUT`, `DELETE` on `/api/v1/agents`) must not have observable side effects on live runtime state (no implicit start/stop of agent loops) — confirmed by asserting that `get_agent_runtime` state is unchanged after a `PUT /api/v1/agents/{id}` call.
- [ ] No route handler may construct an `AgentDefinition` from a raw `manifest_toml` string — all routes must use typed JSON deserialization.
- [ ] All error responses must use the `{ error: { code, message, details } }` envelope — confirmed by asserting the shape of every `4xx` and `5xx` response in tests.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All definition CRUD routes and validate/compile routes are registered in `server.rs` and return non-`404` responses for valid inputs.
- `POST /api/v1/agents/validate` and `POST /api/v1/agents/compile` correctly call the task 18 pipeline and return the `API-SPEC.md` response shapes.
- Definition endpoints do not trigger runtime state changes.
- `GET /api/v1/agents/{id}/compiled` returns a response with all three compiled layers (`agent_manifest`, `provider_binding`, `product_metadata`) populated.
- All list responses include `next_cursor`.
- All error responses include `code`, `message`, and `details`.
- Zero clippy warnings, zero test failures.

---

## Notes

- This task covers only definition CRUD and compile routes. Runtime operational sub-resources are Task 21, and session/message endpoints are Task 22.
- The `POST /api/v1/agents/{id}/fork` endpoint is defined in `API-SPEC.md` but may be deferred to a follow-up task focused on pack management; note its absence explicitly in the implementation if deferred.
- Live integration testing (starting the daemon and curling real endpoints) is mandatory per `CLAUDE.md` before marking this task complete.
- CLI agent surfaces (`openfang-cli`) are out of scope and will be addressed as future work.
