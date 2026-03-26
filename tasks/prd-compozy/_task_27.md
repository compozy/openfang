## markdown

## status: completed

<task_context>
<domain>api/skills</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>task26</dependencies>
</task_context>

# Task 27.0: Skills Listing Endpoint

## Overview

Add a read-only skills listing endpoint to the Compozy API under `/api/v1/skills`. Skills are
file-backed definitions stored under `~/.compozy/skills/` and loaded at boot. They are not managed
through the API (no CRUD); they are managed via the filesystem and packs. This task exposes them as
API-visible resources following the control-plane-first principle established in earlier tasks.

The endpoint follows the same patterns established by task 26 (Schedule Control-Plane Surfaces):
routes registered in `server.rs`, handlers implemented in `routes.rs`, pagination with
`{ items, next_cursor }` per ADR-034, and the standard error envelope for 404 responses.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Implement `GET /api/v1/skills` — paginated list of all loaded skills. Each item includes `id`,
  `name`, `description`, and `source` (the file path the skill was loaded from). Pagination uses
  `{ items, next_cursor }` with `limit` (default 50, max 200), `cursor`, and optional `q`
  full-text filter per ADR-034.
- Implement `GET /api/v1/skills/{id}` — single skill detail. Returns the full skill resource
  including `id`, `name`, `description`, `source`, `created_at`, and `updated_at`. Returns 404
  with the standard error envelope for unknown skill IDs.
- Skills are loaded from `~/.compozy/skills/` at boot time. The endpoint reads from the in-memory
  skill registry; it does not hit the filesystem on each request.
- The endpoint is strictly read-only. No POST, PUT, or DELETE routes. Skills are managed via the
  filesystem and packs (ADR-015, ADR-044).
- Wire routes in `crates/openfang-api/src/server.rs` under the `/api/v1/skills` prefix.
- Implement handlers in `crates/openfang-api/src/routes.rs` (or a dedicated `skills_routes.rs`
  module if the file is already large).
- Follow the same response shape conventions as task 26's schedule list endpoint.
</requirements>

## Subtasks

- [x] 27.1 Define the `SkillSummary` and `SkillDetail` response types in
      `crates/openfang-types/src/skill.rs` (or extend the existing skill types). `SkillSummary`
      carries `id`, `name`, `description`, `source`. `SkillDetail` adds `created_at` and
      `updated_at`. Both derive `Serialize` and `Deserialize`.
- [x] 27.2 Register the `/api/v1/skills` router group in `crates/openfang-api/src/server.rs` with
      two routes: `GET /` (list) and `GET /:id` (detail). Ensure the router is nested under the
      existing `/api/v1` prefix.
- [x] 27.3 Implement the `list_skills` handler. Read from the in-memory skill registry on
      `AppState`. Apply optional `q` filter (case-insensitive substring match on `name` and
      `description`). Apply cursor-based pagination with `limit` and `cursor` query params. Return
      `{ items: Vec<SkillSummary>, next_cursor: Option<String> }`.
- [x] 27.4 Implement the `get_skill` handler. Look up the skill by ID in the in-memory registry.
      Return `SkillDetail` on success, or 404 with the standard error envelope
      (`{ error: { code, message, details } }`) on miss.
- [x] 27.5 Ensure the kernel boot sequence populates the skill registry from `~/.compozy/skills/`
      and that `AppState` holds a reference to this registry. If the registry already exists from
      existing OpenFang code, wire it into `AppState` without duplicating it.
- [x] 27.6 Add route-level and handler-level tests. See the Tests section below.

## Implementation Details

Skills in OpenFang are file-backed definitions that the kernel loads at startup. The existing skill
infrastructure in the kernel already handles loading from disk. This task bridges that internal
registry to the public API surface.

### Skill Resource Shape

List item (`SkillSummary`):

```json
{
  "id": "writing",
  "name": "Writing",
  "description": "Skill for structured document authoring",
  "source": "/home/user/.compozy/skills/writing.toml"
}
```

Detail (`SkillDetail`):

```json
{
  "id": "writing",
  "name": "Writing",
  "description": "Skill for structured document authoring",
  "source": "/home/user/.compozy/skills/writing.toml",
  "created_at": "2026-03-21T12:00:00Z",
  "updated_at": "2026-03-21T14:00:00Z"
}
```

List response:

```json
{
  "items": [],
  "next_cursor": null
}
```

Error response (404):

```json
{
  "error": {
    "code": "not_found",
    "message": "skill 'unknown-skill' not found",
    "details": []
  }
}
```

### Relevant Files

- `crates/openfang-api/src/routes.rs` — add skill handlers here (or in a dedicated module)
- `crates/openfang-api/src/server.rs` — router registration
- `crates/openfang-types/src/skill.rs` — skill types (create or extend)
- `crates/openfang-kernel/src/` — existing skill loading infrastructure
- `tasks/prd-compozy/docs/API-SPEC.md` — section 2 for common conventions
- `tasks/prd-compozy/docs/adrs/034-canonical-control-plane-payload-conventions.md`
- `tasks/prd-compozy/docs/adrs/015-openfang-skills-and-schedules-remain-canonical.md`

### Dependent Files

- `crates/openfang-api/src/state.rs` or `server.rs` — `AppState` must hold skill registry reference
- `crates/openfang-kernel/src/kernel.rs` — kernel holding the skill registry

## Deliverables

- `GET /api/v1/skills` endpoint registered and implemented with pagination and `q` filter
- `GET /api/v1/skills/{id}` endpoint registered and implemented with 404 handling
- `SkillSummary` and `SkillDetail` types defined
- Skill registry wired into `AppState`
- Tests for all operations

## Tests

### Unit Tests (Required)

- [x] `SkillSummary` serialization: a summary with all fields populated produces the expected JSON
      shape with `id`, `name`, `description`, and `source` keys.
- [x] `SkillDetail` serialization: a detail with timestamps produces the expected JSON shape
      including `created_at` and `updated_at`.
- [x] Pagination logic: given 5 skills and `limit=2`, the first page returns 2 items with a
      non-null `next_cursor`; using that cursor returns the next 2; the final page returns 1 item
      with `next_cursor: null`.
- [x] The `q` filter matches case-insensitively on both `name` and `description` fields.

### Integration Tests (Required)

- [x] `GET /api/v1/skills` returns `{ items, next_cursor }` with loaded skills from the test
      registry. Verify status 200 and correct item count.
- [x] `GET /api/v1/skills/{id}` with a valid skill ID returns status 200 and the full skill detail
      including `source` path.
- [x] `GET /api/v1/skills/{id}` with an unknown ID returns status 404 with the standard error
      envelope containing `code: "not_found"`.
- [x] `GET /api/v1/skills?q=writing` returns only skills whose name or description matches the
      query string.
- [x] Pagination across multiple pages returns all skills without duplicates or omissions.

### Regression and Anti-Pattern Guards

- [x] No POST, PUT, or DELETE routes exist under `/api/v1/skills`. Attempting them returns 405
      Method Not Allowed.
- [x] The endpoint reads from the in-memory registry, not from disk on each request.
- [x] The skill registry is populated before the API server starts accepting requests.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- Both endpoints (`GET /api/v1/skills` and `GET /api/v1/skills/{id}`) are registered in the axum
  router and return correct status codes and payload shapes.
- List endpoint returns `{ items, next_cursor }` with pagination and optional `q` filter.
- Detail endpoint returns the full skill resource or 404 with the standard error envelope.
- Skills are served from the in-memory registry, not re-read from disk on each request.
- No mutation endpoints exist; skills remain filesystem-managed.
- All tests pass under `cargo test --workspace` with zero warnings under `cargo clippy`.

---

## Notes

- Use `tasks/prd-compozy/docs/` as the canonical reference baseline for this PRD.
- Keep the root of `tasks/prd-compozy/` reserved for `_task_<num>.md` execution files.
- ADR-015 confirms skills remain canonical OpenFang resources. This task only adds API visibility.
- Follow the same handler and response patterns established by task 26 for consistency.
