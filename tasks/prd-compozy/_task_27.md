## markdown

## status: pending

<task_context>
<domain>engine/dispatch/api</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task24,task25</dependencies>
</task_context>

# Task 27.0: Dispatch And HITL Control-Plane Surfaces

## Overview

Expose `/api/dispatches`, `/api/hitl-requests`, and matching API
surfaces on top of the durable runtime model.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Expose list/detail/action surfaces for dispatch and HITL.
- Keep them aligned with the durable storage and runtime semantics already implemented.
</requirements>

## Subtasks

- [ ] 27.1 Implement dispatch API list/detail/action surfaces.
- [ ] 27.2 Implement HITL API list/detail/answer surfaces.
- [ ] 27.3 Add end-to-end tests for operator and agent-driven use.

## Implementation Details

This is the public control-plane layer on top of tasks 20 to 25.

### Relevant Files

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`
- `tasks/prd-compozy/reset-2026-03-21/API-SPEC.md`

### Dependent Files

- SSE/watch surfaces
- later final hardening task

## Deliverables

- Durable dispatch control-plane API surfaces
- Durable HITL control-plane API surfaces
- End-to-end tests for API-driven operator and agent use

## Tests

### Unit Tests (Required)

- [ ] Dispatch list/detail payloads match the accepted schema.
- [ ] HITL answer flow updates durable state correctly.
- [ ] API responses align with the public contract in API-SPEC.md.

### Integration Tests (Required)

- [ ] Dispatch lifecycle is visible through the public API after runtime execution.
- [ ] HITL request and answer flows work end-to-end.
- [ ] Internal agentic control use cases can rely on the same public surfaces.

### Regression and Anti-Pattern Guards

- [ ] Do not create internal-only endpoints for control-plane actions.
- [ ] Do not expose stale in-memory state instead of durable runtime data.
- [ ] Do not hide response semantics behind ad hoc JSON blobs.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Dispatch and HITL are first-class public runtime resources.
- Humans and internal agents can operate them through the same control plane.

---

## Notes

- These surfaces complete the durable dispatch/HITL runtime slice.
- CLI commands for dispatch and HITL management are deferred to future work (do not touch openfang-cli).
