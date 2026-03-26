## markdown

## status: completed

<task_context>
<domain>engine/triggers/types</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task13</dependencies>
</task_context>

# Task 35.0: Trigger v2 Types And Definition CRUD

## Overview

Implement the trigger v2 definition model, the type system, and all file-backed CRUD and
compile endpoints. Triggers connect external events to actions (`agent_message`, `workflow_start`,
`workflow_signal`). Trigger definitions are file-backed under `~/.compozy/triggers/` following the
config-first storage model and support CRUD, validate, compile, fork, enable/disable, runtime
inspection, and test operations.

This task implements the type system and definition management surfaces defined in ADR-019
(trigger v2 with explicit targets), ADR-025 (trigger v2 public schema), ADR-033 (trigger API
definition and operational surfaces), and API-SPEC.md sections 5 (triggers). It also covers the
validate and compile endpoints per ADR-038.

The current codebase already registers basic `/api/triggers` routes in
`crates/openfang-api/src/server.rs` (lines 288-295), but they use the legacy agent-centric
`TriggerPattern` model from `crates/openfang-kernel/src/triggers.rs` (which only supports
`Lifecycle`, `AgentSpawned`, `System`, `ContentMatch`, etc.) and lack the `/api/v1` prefix. This
task introduces the trigger v2 type system with explicit match/target model and replaces those
routes with the new CRUD surface.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start -- **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement the trigger v2 type system per ADR-025 and DESIGN.md section 22. Top-level fields:
  `id`, `name`, `description`, `enabled`, `max_fires`, `cooldown_secs`, `match`, `target`.
  Match fields: `event`, `source`, `contains`, `filters`. Target kinds: `agent_message`,
  `workflow_start`, `workflow_signal`. `workflow_signal` must include an explicit `selector`
  for the destination run. This new type lives in a new module, distinct from the legacy
  `TriggerPattern` enum in `crates/openfang-kernel/src/triggers.rs`.
- Implement the complete trigger CRUD surface at `/api/v1/triggers`: `GET /api/v1/triggers`,
  `POST /api/v1/triggers`, `GET /api/v1/triggers/{id}`, `PUT /api/v1/triggers/{id}`, and
  `DELETE /api/v1/triggers/{id}`. Trigger definitions are file-backed under
  `~/.compozy/triggers/`. Writes go through validate-normalize-write-reload per ADR-040.
- Implement `POST /api/v1/triggers/validate` per ADR-038 and ADR-034: accepts
  `{ definition, strict, context }` and returns `{ valid, issues, normalized }`. Validation
  checks match field types, target kind validity, target-kind-specific required fields (e.g.,
  `workflow_signal` requires `selector.workflow_id`), and referenced workflow/agent IDs.
  No runtime boot or network calls during validation (ADR-041).
- Implement `POST /api/v1/triggers/compile` per ADR-038: accepts `{ definition, context }` and
  returns `{ definition_id, normalized, compiled: { trigger_ir } }`. Also implement
  `GET /api/v1/triggers/{id}/compiled` for persisted definitions.
- Implement `POST /api/v1/triggers/{id}/fork` per API-SPEC.md section 7 with correct `origin` and
  `forked_from` provenance metadata.
- Implement `GET /api/v1/triggers/{id}/runtime` returning `trigger_id`, `enabled`, `fire_count`,
  `max_fires`, `cooldown_secs`, and `last_fired_at`. Implement `POST /api/v1/triggers/{id}/enable`
  and `POST /api/v1/triggers/{id}/disable` as operational actions; a disabled trigger is never
  matched against incoming events, even when its match expression would otherwise match.
- Implement `POST /api/v1/triggers/{id}/test` per ADR-038: accepts a synthetic event payload and
  returns `{ matched, resolved_target, would_dispatch, explanation }` without executing the
  action. This is the safe simulation surface for trigger matching per ADR-033.
</requirements>

## Subtasks

- [x] 35.1 Define the trigger v2 types in a new module (e.g. `crates/openfang-kernel/src/trigger_v2.rs`
      or a dedicated `crates/openfang-types/src/trigger.rs`). The types must cover: `TriggerV2`
      (top-level resource with all fields from ADR-025), `TriggerMatch` (`event`, `source`, `contains`,
      `filters`), `TriggerTarget` (enum over `AgentMessage`, `WorkflowStart`, `WorkflowSignal`),
      and `TriggerRuntimeStatus` (`trigger_id`, `enabled`, `fire_count`, `max_fires`, `cooldown_secs`,
      `last_fired_at`). These types must be clearly isolated from the legacy `TriggerPattern` enum.

- [x] 35.2 Register the `/api/v1/triggers` router group in `crates/openfang-api/src/server.rs`,
      replacing the existing `/api/triggers` registration. Implement the full CRUD surface:
      `GET /api/v1/triggers` (paginated with `enabled`, `event`, `target_kind`, `q` filters),
      `POST /api/v1/triggers` (create, file-backed), `GET /api/v1/triggers/{id}` (full detail),
      `PUT /api/v1/triggers/{id}` (update with validate-normalize-write-reload), and
      `DELETE /api/v1/triggers/{id}`.

- [x] 35.3 Implement `POST /api/v1/triggers/validate`, `POST /api/v1/triggers/compile`, and
      `GET /api/v1/triggers/{id}/compiled` per ADR-038. Validation must check match field types,
      target kind enum validity, target-kind-specific required fields, and referenced agent/workflow
      IDs. Compile returns `{ definition_id, normalized, compiled: { trigger_ir } }`. Validation must
      not execute the event pipeline or contact external systems.

- [x] 35.4 Implement `POST /api/v1/triggers/{id}/fork` with correct provenance metadata, and
      `GET /api/v1/triggers/{id}/runtime` backed by persisted runtime state. Implement
      `POST /api/v1/triggers/{id}/enable` and `POST /api/v1/triggers/{id}/disable`. Enable/disable
      must update the in-memory matching set synchronously so a newly disabled trigger is excluded from
      the next event match cycle without a daemon restart.

- [x] 35.5 Implement `POST /api/v1/triggers/{id}/test`. The handler accepts a synthetic event
      payload `{ event: { event, source, payload } }` and evaluates all match fields against it using
      the trigger matching engine. The response carries `{ matched, resolved_target, would_dispatch,
      explanation }`. The handler must not execute any action or dispatch, even when `matched = true`
      and `would_dispatch = true`.

- [x] 35.6 Add route-level and type-level tests. See the Tests section below.

- [x] 35.7 Confirm that `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
      and `cargo test --workspace` all pass at zero warnings before marking this task complete.

## Implementation Details

Trigger definitions are file-backed under `~/.compozy/triggers/`. They follow the same
validate-normalize-write-reload write path used by agents, workflows, and schedules (ADR-040).
The trigger runtime state (fire counts, last fired, enabled flag) is persisted separately in
`runtime.db`.

The trigger v2 type system (ADR-025 and DESIGN.md section 22) is intentionally different from the
legacy `TriggerPattern` enum already in `crates/openfang-kernel/src/triggers.rs`. The legacy enum
must not be extended to carry the v2 model. The fork should keep both, with the legacy types
surviving as internal or adapter-level types if still needed by existing kernel systems.

The full trigger resource shape per API-SPEC.md section 5:

```
id, name, description, enabled, max_fires, cooldown_secs,
match: { event, source, contains, filters },
target: { kind: "workflow_start"|"workflow_signal"|"agent_message", ... },
created_at, updated_at
```

The list item shape includes `runtime_status: { enabled, fire_count, last_fired_at }`.

Validation payload conventions (ADR-034 and API-SPEC.md section 2):

- Request: `{ "definition": {}, "strict": true, "context": {} }`
- Response: `{ "valid": true, "issues": [], "normalized": {} }`
- Issue object: `{ "severity": "error"|"warning", "code": "...", "path": "target.workflow", "message": "..." }`

Compilation response: `{ "definition_id": "...", "normalized": {}, "compiled": { "trigger_ir": {} } }`

Trigger test request per API-SPEC.md section 5:

```
{ "event": { "event": "issue.created", "source": "api", "payload": { ... } } }
```

Trigger test response per API-SPEC.md section 5:

```
{
  "matched": true,
  "resolved_target": { "kind": "workflow_start", "workflow": "sdlc", "input": { ... } },
  "would_dispatch": true,
  "explanation": { "match": "...", "target_kind": "workflow_start" }
}
```

Operational action responses (enable, disable) use the accepted envelope:
`{ "accepted": true, "resource_id": "...", "status": "accepted" }`

### Relevant Files

- `crates/openfang-api/src/routes.rs` -- existing handler implementations; add v1 trigger handlers here
- `crates/openfang-api/src/server.rs` -- router registration; replace `/api/triggers` block with `/api/v1/triggers`
- `crates/openfang-kernel/src/triggers.rs` -- legacy `TriggerPattern` and `TriggerId` types; do not extend, keep as legacy adapter
- `crates/openfang-kernel/src/trigger_v2.rs` (new) -- v2 trigger types
- `tasks/prd-compozy/docs/API-SPEC.md` -- canonical payload contracts (section 5 for triggers, section 2 for common conventions)
- `tasks/prd-compozy/docs/DESIGN.md` -- sections 19 (trigger v2), 22 (trigger v2 public schema)
- `tasks/prd-compozy/docs/adrs/019-trigger-v2-with-explicit-targets.md`
- `tasks/prd-compozy/docs/adrs/025-trigger-v2-public-schema.md`
- `tasks/prd-compozy/docs/adrs/033-trigger-api-definition-and-operational-surfaces.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`
- `tasks/prd-compozy/docs/adrs/038-validate-compile-dry-run-and-explain-semantics.md`
- `tasks/prd-compozy/docs/adrs/041-bounded-layered-definition-validation.md`

### Dependent Files

- `crates/openfang-types/src/workflow.rs` -- workflow types referenced by `workflow_start` and `workflow_signal` targets
- Task 36 -- event ingress pipeline and match engine, depends on types defined here

## Deliverables

- Trigger v2 type system (`TriggerV2`, `TriggerMatch`, `TriggerTarget`, `TriggerRuntimeStatus`)
- All `/api/v1/triggers` CRUD endpoints
- `POST /api/v1/triggers/validate`, `POST /api/v1/triggers/compile`, `GET /api/v1/triggers/{id}/compiled`
- `POST /api/v1/triggers/{id}/fork`, `GET /api/v1/triggers/{id}/runtime`
- `POST /api/v1/triggers/{id}/enable`, `POST /api/v1/triggers/{id}/disable`
- `POST /api/v1/triggers/{id}/test`
- Tests for all trigger type operations and definition management scenarios

## Tests

### Unit Tests (Required)

- [x] `TriggerTarget::WorkflowSignal` requires a non-empty `selector.workflow_id`; a definition
      without it returns `valid: false` in validate with `path: "target.selector.workflow_id"`.
- [x] Target kind resolution correctly maps the three target kinds to their dispatch actions:
      `agent_message` -> agent message send, `workflow_start` -> run creation, `workflow_signal` ->
      signal dispatch.
- [x] Enable/disable state transitions: after `POST /api/v1/triggers/{id}/disable` the trigger is
      removed from the active matching set; after `POST /api/v1/triggers/{id}/enable` it is re-added.
      Both changes take effect before the endpoint returns.
- [x] Compile returns `{ definition_id, normalized, compiled: { trigger_ir } }` for a valid
      definition; `trigger_ir` must be non-null and non-empty.
- [x] Validate with `strict = true` reports warnings as errors.

### Integration Tests (Required)

- [x] Full CRUD round-trip: create a trigger (`POST`), read it back (`GET {id}`) with all fields,
      update its `max_fires` (`PUT {id}`), verify the change, delete it (`DELETE {id}`), confirm
      subsequent `GET {id}` returns 404.
- [x] `POST /api/v1/triggers/{id}/test` with a matching synthetic event returns `matched: true`,
      a non-null `resolved_target`, and `would_dispatch: true`, without creating any workflow run or
      agent message.
- [x] `POST /api/v1/triggers/{id}/test` with a non-matching synthetic event returns
      `matched: false` and `would_dispatch: false`.
- [x] `POST /api/v1/triggers/validate` for a `workflow_signal` target without a `selector` field
      returns `valid: false` with a structured issue at `path: "target.selector"`.
- [x] List endpoint returns `{ items, next_cursor }` with pagination; `target_kind=workflow_start`
      filter returns only triggers with that target kind.

### Regression and Anti-Pattern Guards

- [x] The legacy `TriggerPattern` enum in `crates/openfang-kernel/src/triggers.rs` must not be
      extended to carry v2 matching fields; the two type systems must remain isolated.
- [x] `POST /api/v1/triggers/{id}/test` must never dispatch an action, even when both
      `matched = true` and `would_dispatch = true`.
- [x] File-backed writes for trigger definitions are atomic: a failed normalization must not
      leave a partial file.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- All thirteen endpoints (`GET`, `POST`, `GET {id}`, `PUT {id}`, `DELETE {id}`, `POST validate`,
  `POST compile`, `GET {id}/compiled`, `POST {id}/fork`, `GET {id}/runtime`, `POST {id}/enable`,
  `POST {id}/disable`, `POST {id}/test`) are registered in the axum router and return correct
  status codes and payload shapes.
- The trigger v2 type system covers all match fields (`event`, `source`, `contains`, `filters`)
  and all target kinds (`agent_message`, `workflow_start`, `workflow_signal`), each clearly
  isolated from the legacy `TriggerPattern` enum.
- Validate endpoint returns `{ valid, issues, normalized }` with structured issues for every
  detected match or target field problem.
- Compile endpoint returns `{ definition_id, normalized, compiled: { trigger_ir } }` for valid
  definitions without triggering any execution.
- Test endpoint returns match result and resolved target without dispatching any action.
- Enable/disable takes effect in the active matching set before the endpoint returns.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- CLI commands for trigger management are deferred to future work (do not touch openfang-cli).
- The existing `/api/triggers` routes in `server.rs` (lines 288-295) must be migrated, not
  duplicated. Remove the old registration once the v1 surface is wired and tested.
- This task implements DESIGN.md sections 19 (trigger v2) and 22 (trigger v2 public schema) and
  API-SPEC.md section 5 (triggers).
- The event ingress pipeline (`POST /api/v1/events`) is handled by Task 36.
