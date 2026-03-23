## markdown

## status: pending

<task_context>
<domain>providers/arky/config</domain>
<type>implementation</type>
<scope>configuration</scope>
<complexity>high</complexity>
<dependencies>task4</dependencies>
</task_context>

# Task 10.0: Provider Layering For Workspace, Profiles, And Agent Config

## Overview

Implement the provider-layering model for Compozy using the internal Arky crates:
installation/workspace config, named profiles, and per-agent config.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Reflect the accepted provider layering in real code.
- Keep infrastructure-level provider plumbing out of normal agent documents.
</requirements>

## Subtasks

- [ ] 10.1 Identify which internal Arky config structures can be reused directly.
- [ ] 10.2 Introduce layering-aware config structures for workspace, profiles, and agent overrides.
- [ ] 10.3 Add validation around the allowed boundary between those layers.

## Implementation Details

The current `arky-config` layering is useful but not rich enough to represent
the full Compozy provider block without a new integration layer.

**Note:** The Arky crates referenced below (e.g. `arky-config`) are copied from the Arky workspace (`~/Dev/compozy/arky/crates/`) into `openfang/crates/` as part of task 4. The file paths below reference these local copies within the OpenFang workspace.

### Relevant Files

- `crates/arky-config/src/loader.rs`
- `crates/arky-config/src/merge.rs`
- `crates/arky-config/src/validate.rs`
- `tasks/prd-compozy/reset-2026-03-21/DESIGN.md`

### Dependent Files

- later `ProviderBinding` compile layer
- agent-definition validator and compiler

## Deliverables

- Provider-layering model in code
- Validation for layer boundaries
- Tests for precedence and override rules

## Tests

### Unit Tests (Required)

- [ ] Workspace, profile, and agent-level provider settings merge in the intended order.
- [ ] Infra-only settings are rejected at agent level where appropriate.
- [ ] Request-level settings remain bounded and typed.

### Integration Tests (Required)

- [ ] Example workspace config plus profile plus agent override yields expected effective provider config.
- [ ] Unsupported provider-layer combinations fail clearly.
- [ ] Existing provider setups remain mappable into the layered model.

### Regression and Anti-Pattern Guards

- [ ] Do not let `request_extra` become a bag for credentials or bootstrap config.
- [ ] Do not flatten typed provider blocks into generic top-level fields.
- [ ] Do not mix install-level secrets into file-backed agent definitions.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Provider layering exists in code, not just in docs.
- The boundary between workspace/profile/agent config is enforceable.
- Later agent/provider tasks can build on a stable model.

---

## Prior Implementation Reference

The old TypeScript codebase has provider layering patterns:

- `~/Dev/compozy/compozy-code/providers/core/src/` — Provider hooks, MCP bridges, tool bridges, token consumption
- `~/Dev/compozy/compozy-code/providers/runtime/src/` — OpenResponses protocol, AI SDK bridge, session/config management

The old model uses a Vercel AI SDK bridge with workspace-level and per-provider config. The new
Arky-based model is richer (three explicit tiers), but the old code shows how workspace vs per-agent
config was separated in practice.

## Notes

- This task should land before dispatch/HITL because provider/session semantics matter there.
