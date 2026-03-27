## markdown

## status: pending

<task_context>
<domain>engine/api</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 1: API Route + Server Decomposition

## Overview

Split `crates/openfang-api/src/routes.rs` (~26,360 lines) into domain-specific route modules and refactor `server.rs` to compose per-domain sub-routers instead of a single monolithic router.

<critical>
- **ALWAYS READ** @AGENTS.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-refac/techspec.md` and `tasks/prd-refac/analysis_api.md` before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Split `routes.rs` by existing route families into a `routes/` module directory
- Move shared response helpers and test support into focused modules
- Break the router construction in `server.rs` into per-domain sub-routers merged at the top level
- Introduce compatibility re-exports so downstream code (tests, other crates) continues to compile without changes
- No path changes, no request/response shape changes, no auth/middleware/SSE contract changes
</requirements>

## Subtasks

- [ ] 1.1 Audit `routes.rs` to identify all route families and shared helpers
- [ ] 1.2 Create `routes/` module directory with `mod.rs` that re-exports the current public API
- [ ] 1.3 Extract shared response helpers and error types into `routes/helpers.rs`
- [ ] 1.4 Extract test support utilities into `routes/test_support.rs`
- [ ] 1.5 Move agent route handlers into `routes/agents_v1.rs`
- [ ] 1.6 Move workflow route handlers into `routes/workflows_v1.rs`
- [ ] 1.7 Move run route handlers into `routes/runs.rs`
- [ ] 1.8 Move trigger route handlers into `routes/triggers_v1.rs`
- [ ] 1.9 Move schedule route handlers into `routes/schedules_v1.rs`
- [ ] 1.10 Move task route handlers into `routes/tasks_v1.rs`
- [ ] 1.11 Move pack route handlers into `routes/packs_v1.rs`
- [ ] 1.12 Move channel route handlers into `routes/channels.rs`
- [ ] 1.13 Move system/health route handlers into `routes/system.rs`
- [ ] 1.14 Refactor `server.rs` to build per-domain sub-routers and merge at top level
- [ ] 1.15 Verify all existing API integration tests pass without modifications

## Implementation Details

The exact file list may differ from above. The requirement is domain separation, not a fixed count of files. Follow the natural route families that already exist in the code.

### Approach

1. Start by reading `routes.rs` end-to-end to identify all handler functions and which route family they belong to.
2. Identify shared types, helpers, and test utilities that are used across multiple route families.
3. Create the `routes/` directory, move shared code first, then move handlers family by family.
4. After each family move, run `make test` to catch breakage early.
5. Once all handlers are moved, refactor `server.rs` to use per-domain sub-router composition.

### Relevant Files

- `crates/openfang-api/src/routes.rs` (primary target, ~26K lines)
- `crates/openfang-api/src/server.rs` (router composition)
- `crates/openfang-api/src/lib.rs` (module declarations)

### Dependent Files

- Any file that imports from `openfang_api::routes`
- API integration tests

## Deliverables

- `routes/` module directory with domain-separated handler files
- `routes/mod.rs` with re-exports preserving the current public API surface
- Updated `server.rs` with per-domain sub-router composition
- All existing tests passing without modification

## Tests

### Unit Tests (Required)

- [ ] All existing route handler tests pass in their new module locations
- [ ] Re-exports in `routes/mod.rs` cover all previously public items

### Integration Tests (Required)

- [ ] All existing API integration tests pass without any test code changes
- [ ] Route paths remain identical (no 404 regressions)

### Regression and Anti-Pattern Guards

- [ ] No request/response shape changes
- [ ] No auth, middleware, or SSE contract changes
- [ ] No path changes to any endpoint

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `routes.rs` no longer exists as a monolithic file
- Each route family lives in its own module under `routes/`
- `server.rs` composes routers per domain instead of one flat builder
- All existing tests pass without modification
- Zero warnings, zero errors on `make fmt && make lint && make test`
