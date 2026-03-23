## markdown

## status: pending

<task_context>
<domain>engine/workflows/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task14,task19</dependencies>
</task_context>

# Task 25.0: Workflow Definition CRUD Control-Plane Surfaces

## Overview

Implement the full workflow definition CRUD API surfaces under `/api/v1/workflows`. Users can
create, read, update, delete, validate, compile, fork, and inspect runtime state for workflow
definitions through these endpoints. Workflow definitions are file-backed (following the
config-first storage model established in DESIGN.md section 6 and ADR-040), so writes go through a
validate-normalize-write-reload path. The compile endpoint returns the workflow IR produced by the
compiler from task 14. All payloads follow the canonical conventions from ADR-034 and API-SPEC.md
section 4.

The current codebase already has a basic `/api/workflows` surface registered in
`crates/openfang-api/src/server.rs` (lines 311-327), but it uses legacy route shapes, lacks the
`/api/v1` prefix, and is missing validate, compile, fork, and runtime sub-resources. This task
replaces that surface with the Compozy-owned definition-first contract described in ADR-032 and
ADR-023.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement the complete workflow definition CRUD surface at `/api/v1/workflows`, including
  `GET /api/v1/workflows`, `POST /api/v1/workflows`, `GET /api/v1/workflows/{id}`,
  `PUT /api/v1/workflows/{id}`, and `DELETE /api/v1/workflows/{id}`.
- Implement `POST /api/v1/workflows/validate` per ADR-038: accepts `{ definition, strict, context }`
  and returns `{ valid, issues, normalized }` where each issue carries `severity`, `code`, `path`,
  and `message` fields as specified in API-SPEC.md section 2.
- Implement `POST /api/v1/workflows/compile` per ADR-038: accepts `{ definition, context }` and
  returns `{ definition_id, normalized, compiled: { workflow_ir: {} } }` by delegating to the
  workflow IR compiler from task 14.
- Implement `GET /api/v1/workflows/{id}/compiled` to return the compiled form of a persisted
  workflow definition without re-supplying the definition in the request body.
- Implement `POST /api/v1/workflows/{id}/fork` per API-SPEC.md section 7: accepts `{ mode }` and
  returns the forked definition with `origin.kind = "user"` and a populated `forked_from` block
  tracking pack provenance. A fork must not silently shadow a managed pack object without going
  through the explicit fork operation (ADR-044).
- Implement `GET /api/v1/workflows/{id}/runtime` to return the workflow runtime status shape
  (`workflow_id`, `loaded`, `healthy`, `active_runs`, `waiting_runs`, `last_run_at`) and
  `POST /api/v1/workflows/{id}/runs` plus `POST /api/v1/workflows/{id}/runs/dry-run` per
  API-SPEC.md section 4.
- All list endpoints return `{ items, next_cursor }` with `limit` (default 50, max 200), `cursor`,
  `sort`, `order`, and filter parameters `enabled`, `tag`, and `q` per ADR-034.
- Writes (POST, PUT) go through the validate-normalize-write-reload path and return the full
  resulting resource. Write operations must be atomic at the file level; a failed normalize must not
  produce a partial file.
</requirements>

## Subtasks

- [ ] 25.1 Register the `/api/v1/workflows` router group in `crates/openfang-api/src/server.rs`,
      replacing the existing `/api/workflows` registration. Wire `AppState` to a new workflow handler
      module. Implement `GET /api/v1/workflows` (paginated list with `enabled`, `tag`, `q` filters),
      `POST /api/v1/workflows` (create with validate-normalize-write-reload), `GET /api/v1/workflows/{id}`
      (full detail), `PUT /api/v1/workflows/{id}` (update with same write path), and
      `DELETE /api/v1/workflows/{id}`.
- [ ] 25.2 Implement `POST /api/v1/workflows/validate` per ADR-038 and API-SPEC.md section 2.
      Validation must be layered: schema check, reference check (agent IDs, primitive names, workflow
      IDs in sub-workflow steps), semantic check (unique step IDs, legal `kind`/`uses`/`flow`
      combinations, binding reference resolution for `input`, `vars`, and `steps.<id>.output`,
      `save_as`, and `outputs`). Normalization fills defaults and canonicalizes aliases
      (`text` → `string`, `json` → `any`). Never invoke the runtime or make network calls during
      validation (ADR-041).
- [ ] 25.3 Implement `POST /api/v1/workflows/compile` and `GET /api/v1/workflows/{id}/compiled` per
      ADR-038. Compile delegates to the workflow IR compiler from task 14. The response carries
      `{ definition_id, normalized, compiled: { workflow_ir } }`. Compilation must succeed only on a
      valid, normalized definition; a compilation request for an invalid definition returns a structured
      error following the API-SPEC.md section 2 error envelope.
- [ ] 25.4 Implement `POST /api/v1/workflows/{id}/fork` per API-SPEC.md section 7. The fork writes
      a new file-backed user-owned definition with `origin.kind = "user"` and populates `forked_from`
      with the upstream pack provenance. Same-ID shadowing of managed pack objects is allowed only
      through this explicit endpoint; direct `POST /api/v1/workflows` with a conflicting ID must be
      rejected with a clear error.
- [ ] 25.5 Implement `GET /api/v1/workflows/{id}/runtime`, `POST /api/v1/workflows/{id}/runs`, and
      `POST /api/v1/workflows/{id}/runs/dry-run`. The dry-run response follows the ADR-034/API-SPEC.md
      dry-run envelope: `{ would_execute, resolved, effects, explanation }`. The explanation block must
      include the resolved `input_contract` and `output_contract` shapes.
- [ ] 25.6 Implement `GET /api/v1/workflows/{id}/runs` returning a paginated list of run summaries
      (`id`, `status`, `current_step_id`, `started_at`, `updated_at`) for that workflow.
- [ ] 25.7 Add route-level and handler-level tests. See the Tests section below.

## Implementation Details

Workflow definitions are file-backed under `~/.compozy/workflows/`. Writes go through a
validate-normalize-write-reload path to ensure consistency. The compile endpoint delegates to the
workflow IR compiler from task 14. The runtime and run sub-resources connect to the durable
workflow runtime from task 16.

The current server router (registered in `crates/openfang-api/src/server.rs`) uses
`axum::routing::{get, post, put, delete}` combinators. The new workflow routes should follow the
same axum patterns already used in that file. The `AppState` struct in
`crates/openfang-api/src/routes.rs` holds the `Arc<OpenFangKernel>` and should be extended with
any new workflow-related state, such as a workflow repository handle, if introduced by task 14/16.

The workflow definition resource shape is specified in API-SPEC.md section 4. The list item shape
omits the full `steps` array and includes a `steps` count integer, `runtime_status`, `origin`, and
`updated_at`. The full detail shape includes the complete `steps` array, `input`/`output` contracts,
`defaults`, `outputs` projection, and provenance metadata.

Validation payload conventions (ADR-034 and API-SPEC.md section 2):

- Request: `{ "definition": {}, "strict": true, "context": {} }`
- Response: `{ "valid": true, "issues": [], "normalized": {} }`
- Issue object: `{ "severity": "error"|"warning", "code": "...", "path": "steps[1].uses.agent", "message": "..." }`

Compilation payload conventions (ADR-034 and API-SPEC.md section 2):

- Request: `{ "definition": {}, "context": {} }`
- Response: `{ "definition_id": "sdlc", "normalized": {}, "compiled": { "workflow_ir": {} } }`

Dry-run payload conventions (ADR-034 and API-SPEC.md section 4):

- Request: same as `POST /api/v1/workflows/{id}/runs`
- Response: `{ "would_execute": true, "resolved": { "workflow_id", "workflow_version", "initial_step_id" }, "effects": { "run_create", "initial_dispatches" }, "explanation": { "input_contract", "output_contract" } }`

Operational action responses (enable, fork, run creation) use the accepted envelope:
`{ "accepted": true, "resource_id": "...", "status": "accepted", "run_id": "..." }`

Error responses use the stable envelope: `{ "error": { "code": "...", "message": "...", "details": [] } }`

### Relevant Files

- `crates/openfang-api/src/routes.rs` — existing handler implementations; add new v1 workflow handlers here
- `crates/openfang-api/src/server.rs` — router registration; replace `/api/workflows` block with `/api/v1/workflows`
- `crates/openfang-kernel/src/workflow.rs` — existing `Workflow`, `WorkflowId`, `WorkflowStep`, `StepMode`, `ErrorMode` types
- `tasks/prd-compozy/docs/API-SPEC.md` — canonical payload contracts (section 4 for workflows, section 2 for common conventions)
- `tasks/prd-compozy/docs/DESIGN.md` — sections 6 (config-first), 17 (workflow v2), 21 (workflow v2 public schema)
- `tasks/prd-compozy/docs/adrs/032-workflow-api-definition-and-operational-surfaces.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`
- `tasks/prd-compozy/docs/adrs/040-toml-authoring-json-transport-ir-execution.md`
- `tasks/prd-compozy/docs/adrs/041-bounded-layered-definition-validation.md`

### Dependent Files

- `crates/openfang-types/src/` — type definitions consumed by handlers
- `crates/openfang-kernel/src/workflow.rs` — kernel workflow state (from task 14)

## Deliverables

- All `/api/v1/workflows` CRUD endpoints registered and implemented
- `POST /api/v1/workflows/validate` and `POST /api/v1/workflows/compile` endpoints
- `GET /api/v1/workflows/{id}/compiled` endpoint
- `POST /api/v1/workflows/{id}/fork` endpoint
- `GET /api/v1/workflows/{id}/runtime` endpoint
- `POST /api/v1/workflows/{id}/runs` and `POST /api/v1/workflows/{id}/runs/dry-run` endpoints
- `GET /api/v1/workflows/{id}/runs` paginated list endpoint
- Tests for all operations

## Tests

### Unit Tests (Required)

- [ ] Workflow definition with valid steps serializes/deserializes correctly through the public
      resource shape (all top-level fields: `id`, `name`, `version`, `description`, `enabled`, `tags`,
      `input`, `output`, `defaults`, `steps`, `outputs`, `origin`, `forked_from`, `created_at`,
      `updated_at`).
- [ ] Validation returns `valid: false` with a structured issue list when a step references a
      non-existent agent ID; `path` must point to the offending field (e.g. `steps[1].uses.agent`).
- [ ] Validation accepts the convenience alias `"text"` for step input kinds and normalizes it to
      `"string"` in the `normalized` response block.
- [ ] Compile returns `{ definition_id, normalized, compiled: { workflow_ir } }` for a
      well-formed definition; the `workflow_ir` field must be non-null and non-empty.
- [ ] Compile returns a structured error (not a 500) when called on an invalid definition; the
      error envelope must carry `code` and `message`.
- [ ] The `POST /api/v1/workflows/{id}/runs/dry-run` response carries `would_execute: true` and
      an `explanation` block containing `input_contract` and `output_contract`.

### Integration Tests (Required)

- [ ] Full CRUD round-trip: create a workflow definition (`POST`), read it back (`GET {id}`),
      update its description (`PUT {id}`), verify the update is reflected, then delete it
      (`DELETE {id}`) and confirm a subsequent `GET {id}` returns 404.
- [ ] List endpoint returns `{ items, next_cursor }` with correct pagination: create three
      definitions, fetch with `limit=2`, assert `next_cursor` is non-null, fetch second page,
      assert `next_cursor` is null and both pages together contain all three.
- [ ] `POST /api/v1/workflows/validate` for a workflow with a missing required field returns
      `valid: false` and an `issues` array with at least one entry whose `severity` is `"error"`.
- [ ] `GET /api/v1/workflows/{id}/compiled` after a successful create returns the compiled form
      without needing to re-supply the definition body.
- [ ] `POST /api/v1/workflows/{id}/fork` creates a new file-backed definition with
      `origin.kind = "user"` and a populated `forked_from` block; a subsequent `GET` on the forked ID
      returns the correct provenance fields.
- [ ] `POST /api/v1/workflows` with an ID that collides with a managed pack definition without
      using the fork endpoint returns a 409 conflict error.
- [ ] `DELETE /api/v1/workflows/{id}` on a non-existent ID returns 404 with the stable error
      envelope.

### Regression and Anti-Pattern Guards

- [ ] No endpoint returns null or empty for a valid definition: `GET /api/v1/workflows/{id}` must
      return the full resource shape including all required fields.
- [ ] File-backed writes are atomic: a failed normalization during `POST` or `PUT` must not leave
      a partial file on disk; the previous definition file must remain intact.
- [ ] Validation never boots the runtime, makes network calls, or executes template expressions;
      these are forbidden per ADR-041.
- [ ] `POST /api/v1/workflows/compile` is definition-oriented and does not trigger a workflow run
      or any side-effecting operation (ADR-038).

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All eleven endpoints (`GET`, `POST`, `GET {id}`, `PUT {id}`, `DELETE {id}`, `POST validate`,
  `POST compile`, `GET {id}/compiled`, `POST {id}/fork`, `GET {id}/runtime`,
  `POST {id}/runs`, `POST {id}/runs/dry-run`, `GET {id}/runs`) are registered in the axum router
  and return correct status codes and payload shapes for both happy-path and error cases.
- Validate endpoint returns `{ valid, issues, normalized }` with structured issue objects that
  include `severity`, `code`, `path`, and `message` for every detected problem.
- Compile endpoint returns `{ definition_id, normalized, compiled: { workflow_ir } }` for any
  valid definition; it is never confused with running a workflow.
- Fork endpoint returns a user-owned definition with correct `forked_from` provenance; direct
  create with a conflicting managed-pack ID is rejected.
- List endpoint supports `limit`/`cursor` pagination and `enabled`/`tag`/`q` filters.
- All CRUD operations on file-backed definitions are atomic; no partial writes survive a failure.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- The existing `/api/workflows` routes in `server.rs` (lines 311-327) must be migrated, not
  duplicated. Remove the old registration once the v1 surface is wired and tested.
- The `explain` verb is not a standalone endpoint: explanation content is embedded inside compile
  and dry-run responses per ADR-038.
