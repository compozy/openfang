## markdown

## status: completed

<task_context>
<domain>api/packs</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task39</dependencies>
</task_context>

# Task 40.0: Pack List Detail And CRUD Endpoints

## Overview

Add pack listing, detail, and fork endpoints to the Compozy API under `/api/v1/packs`. This task
provides the read and fork layer for packs before the full pack lifecycle system (task 41 handles
install, upgrade, uninstall). Packs are versioned distribution units whose metadata is derived from
filesystem scanning of `~/.compozy/packs/`. Each pack directory contains a `pack.toml` manifest
with id, version, description, and an objects list.

The API spec (section 7) defines the pack resource shape and endpoints. This task implements the
subset that does not require the install/upgrade/uninstall infrastructure: list, detail, objects,
and fork. The fork endpoint creates a user-owned shadow copy of a managed object in the top-level
definition directory, following ADR-044.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Define the `PackManifest` type in `crates/openfang-types/src/pack.rs` with fields: `id`,
  `name`, `version`, `description`, `source` (with kind: `bundled` or `external`), and `objects`
  (list of managed object references with `resource_type` and `resource_id`). Derive `Serialize`,
  `Deserialize`.
- Implement `GET /api/v1/packs` — paginated list of installed packs. Each item includes `id`,
  `name`, `version`, `source`, `installed`, `managed`, and `objects` (summary counts by resource
  type: agents, workflows, triggers, schedules, templates). Pagination with `{ items, next_cursor }`
  per ADR-034 with `limit` (default 50, max 200) and `cursor`.
- Implement `GET /api/v1/packs/{id}` — pack detail with the full manifest and managed objects
  list. Returns the complete `PackManifest` plus `installed`, `managed`, `updated_at`. Returns 404
  with standard error envelope for unknown pack IDs.
- Implement `GET /api/v1/packs/{id}/objects` — paginated list of managed objects within a pack.
  Each item includes `resource_type`, `resource_id`, `forked` (boolean indicating whether a
  user-owned fork exists for this object).
- Implement `POST /api/v1/packs/{id}/fork` — fork a specific managed object to user-owned. The
  request body specifies `{ resource_type, resource_id, mode: "shadow" }`. The handler copies the
  managed definition file to the top-level directory (e.g., `~/.compozy/agents/` for an agent),
  sets `origin.kind = "user"`, and populates `forked_from` with the pack provenance. Returns the
  resulting resource metadata with origin and forked_from fields.
- Pack metadata is derived at boot from scanning `~/.compozy/packs/` directories. Each pack
  directory must contain a `pack.toml` manifest. Invalid or missing manifests are logged as
  warnings and skipped.
- Wire routes in `crates/openfang-api/src/server.rs` under the `/api/v1/packs` prefix.
</requirements>

## Subtasks

- [x] 40.1 Define `PackManifest`, `PackObjectRef`, `PackSummary`, `PackDetail`, and
      `PackObjectSummary` types in `crates/openfang-types/src/pack.rs`. `PackManifest` is the
      deserialized `pack.toml` shape. `PackSummary` is the API list item. `PackDetail` is the API
      detail. `PackObjectRef` represents a managed object (`resource_type`, `resource_id`).
      `PackObjectSummary` extends `PackObjectRef` with a `forked` boolean. All derive `Serialize`
      and `Deserialize`.
- [x] 40.2 Implement a `PackRegistry` that scans `~/.compozy/packs/` at boot, parses each
      `pack.toml`, and holds the parsed manifests in memory. Place in
      `crates/openfang-kernel/src/pack_registry.rs` (or a module under the kernel). The registry
      provides `list_packs()`, `get_pack(id)`, and `list_objects(pack_id)` methods.
- [x] 40.3 Wire `PackRegistry` into `AppState` so API handlers can access it. Ensure it is
      populated during kernel boot before the API server starts.
- [x] 40.4 Register the `/api/v1/packs` router group in `server.rs` with four routes:
      `GET /` (list), `GET /:id` (detail), `GET /:id/objects` (objects list),
      `POST /:id/fork` (fork).
- [x] 40.5 Implement the `list_packs` handler. Read from the in-memory `PackRegistry`. Compute
      object counts by resource type for each pack summary. Apply cursor-based pagination.
- [x] 40.6 Implement the `get_pack` handler. Return the full pack detail or 404 with the standard
      error envelope.
- [x] 40.7 Implement the `list_pack_objects` handler. Return the paginated list of managed objects
      for a pack. For each object, check whether a user-owned fork exists in the top-level
      definition directory and set the `forked` boolean accordingly.
- [x] 40.8 Implement the `fork_pack_object` handler. Validate that the specified object exists in
      the pack manifest. Copy the managed definition file to the appropriate top-level directory.
      Set `origin.kind = "user"` and populate `forked_from` with pack provenance. Return the
      fork metadata. Return 404 if the pack or object is not found. Return 409 Conflict if a
      user-owned fork already exists.
- [x] 40.9 Add route-level and handler-level tests. See the Tests section below.

## Implementation Details

Packs are filesystem-backed distribution units. Each installed pack lives in a directory under
`~/.compozy/packs/<pack_id>/`. The directory contains a `pack.toml` manifest and the managed
definition files organized by resource type.

### pack.toml Format

```toml
id = "sdlc"
name = "SDLC"
version = "1.2.0"
description = "First-party SDLC workflow package"

[source]
kind = "bundled"

[[objects]]
resource_type = "agent"
resource_id = "prd-writer"

[[objects]]
resource_type = "workflow"
resource_id = "sdlc"

[[objects]]
resource_type = "trigger"
resource_id = "issue-created-start-sdlc"
```

### Pack List Item Shape (API)

```json
{
  "id": "sdlc",
  "name": "SDLC",
  "version": "1.2.0",
  "source": {
    "kind": "bundled"
  },
  "installed": true,
  "managed": true,
  "objects": {
    "agents": 5,
    "workflows": 2,
    "triggers": 3,
    "schedules": 1,
    "templates": 4
  },
  "updated_at": "2026-03-21T14:00:00Z"
}
```

### Pack Object Summary Shape

```json
{
  "resource_type": "workflow",
  "resource_id": "sdlc",
  "forked": false
}
```

### Fork Request

```json
{
  "resource_type": "workflow",
  "resource_id": "sdlc",
  "mode": "shadow"
}
```

### Fork Response

```json
{
  "id": "sdlc",
  "origin": {
    "kind": "user"
  },
  "forked_from": {
    "kind": "pack",
    "pack_id": "sdlc",
    "pack_version": "1.2.0",
    "resource_type": "workflow",
    "resource_id": "sdlc"
  }
}
```

### Relevant Files

- `crates/openfang-api/src/routes.rs` — add pack handlers
- `crates/openfang-api/src/server.rs` — router registration
- `crates/openfang-types/src/pack.rs` — pack types (create new module)
- `crates/openfang-kernel/src/pack_registry.rs` — pack scanning and registry (create new module)
- `tasks/prd-compozy/docs/API-SPEC.md` — section 7 (Packs)
- `tasks/prd-compozy/docs/adrs/044-versioned-packs-explicit-upgrades-and-safe-forks.md`
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`

### Dependent Files

- `crates/openfang-kernel/src/kernel.rs` — kernel boot sequence for pack scanning
- `crates/openfang-kernel/src/definition_store.rs` — file I/O for fork operations
- `crates/openfang-api/src/state.rs` or `server.rs` — `AppState` must hold `PackRegistry`

## Deliverables

- `PackManifest` type and related types in `openfang-types`
- `PackRegistry` with filesystem scanning in `openfang-kernel`
- `GET /api/v1/packs` endpoint with pagination and object counts
- `GET /api/v1/packs/{id}` endpoint with full detail
- `GET /api/v1/packs/{id}/objects` endpoint with fork status
- `POST /api/v1/packs/{id}/fork` endpoint with provenance metadata
- Tests for all operations

## Tests

### Unit Tests (Required)

- [x] `PackManifest` deserialization: a valid `pack.toml` string with id, version, objects list
      deserializes correctly.
- [x] `PackManifest` deserialization: a manifest with missing required fields produces a clear
      deserialization error.
- [x] `PackSummary` serialization: produces the expected JSON shape with `objects` as a count map
      (`agents: 5, workflows: 2`, etc.).
- [x] Object count computation: given a manifest with 3 agents, 2 workflows, and 1 trigger, the
      computed counts map is correct.

### Integration Tests (Required)

- [x] `GET /api/v1/packs` returns `{ items, next_cursor }` with status 200. Items match the packs
      loaded from the test fixtures directory.
- [x] `GET /api/v1/packs/{id}` with a valid pack ID returns status 200 and the full pack detail
      including managed objects list.
- [x] `GET /api/v1/packs/{id}` with an unknown pack ID returns status 404 with the standard error
      envelope.
- [x] `GET /api/v1/packs/{id}/objects` returns the managed objects list with correct `forked`
      boolean values.
- [x] `POST /api/v1/packs/{id}/fork` with a valid object creates a user-owned copy in the
      top-level directory and returns the fork metadata with `origin.kind = "user"` and populated
      `forked_from`.
- [x] `POST /api/v1/packs/{id}/fork` with an already-forked object returns 409 Conflict.
- [x] `POST /api/v1/packs/{id}/fork` with an unknown object in the pack returns 404.
- [x] After a successful fork, `GET /api/v1/packs/{id}/objects` shows the forked object with
      `forked: true`.

### Regression and Anti-Pattern Guards

- [x] The `PackRegistry` scan at boot logs warnings for directories without a valid `pack.toml`
      and continues loading other packs.
- [x] Fork operations use atomic file writes (write .tmp, rename) to avoid partial copies.
- [x] The fork endpoint does not modify the managed pack directory; it only writes to top-level
      definition directories.
- [x] Normal `POST /api/v1/<resource>` create operations cannot silently shadow managed pack
      objects. Only the explicit fork endpoint creates same-ID overrides.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- All four endpoints (`GET /api/v1/packs`, `GET /api/v1/packs/{id}`,
  `GET /api/v1/packs/{id}/objects`, `POST /api/v1/packs/{id}/fork`) are registered in the axum
  router and return correct status codes and payload shapes.
- `PackManifest` correctly deserializes `pack.toml` files from pack directories.
- List endpoint returns `{ items, next_cursor }` with object counts per resource type.
- Fork creates a user-owned shadow in the top-level definition directory with correct provenance
  metadata.
- 409 Conflict is returned when attempting to fork an already-forked object.
- Pack metadata is loaded from disk at boot; the API reads from the in-memory registry.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- Task 41 handles the full pack lifecycle (install, upgrade, uninstall). This task provides the
  read and fork layer that must exist first.
- ADR-044 is the primary authority for pack versioning, fork semantics, and upgrade safety.
- The `mode: "shadow"` fork mode is the only supported mode initially. Future modes may be added
  but are out of scope for this task.
