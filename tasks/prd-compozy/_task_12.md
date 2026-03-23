## markdown

## status: pending

<task_context>
<domain>providers/arky/adapters</domain>
<type>integration</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task11</dependencies>
</task_context>

# Task 12.0: Typed Provider Integration For Codex And Claude Code

## Overview

Connect `ProviderBinding` to the typed provider config and runtime paths for
Codex and Claude Code.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Support Codex and Claude Code as the first deep provider integrations.
- Keep wrapper/provider-specific semantics preserved.
</requirements>

## Subtasks

- [ ] 12.1 Implement binding -> Codex config mapping.
- [ ] 12.2 Implement binding -> Claude Code config mapping.
- [ ] 12.3 Add adapter tests for provider-specific behavior and invalid config.

## Implementation Details

This task is about provider-specific integration, not public agent surfaces yet.

**Note:** The Arky crates referenced below (e.g. `arky-codex`, `arky-claude-code`) are copied from the Arky workspace (`~/Dev/compozy/arky/crates/`) into `openfang/crates/` as part of task 4. The file paths below reference these local copies within the OpenFang workspace.

### Relevant Files

- `crates/arky-codex/src/config.rs`
- `crates/arky-codex/src/provider.rs`
- `crates/arky-claude-code/src/config.rs`
- `crates/arky-claude-code/src/profile.rs`
- `crates/arky-claude-code/src/provider.rs`

### Dependent Files

- agent-definition compiler
- runtime dispatch integration

## Deliverables

- Codex adapter from `ProviderBinding`
- Claude Code adapter from `ProviderBinding`
- Tests for typed provider behavior

## Tests

### Unit Tests (Required)

- [ ] Codex bindings resolve to the expected typed config.
- [ ] Claude Code bindings resolve to the expected typed config.
- [ ] Provider-specific unsupported fields fail in the correct layer.

### Integration Tests (Required)

- [ ] Example Codex agent config reaches a runnable provider binding.
- [ ] Example Claude Code agent config reaches a runnable provider binding.
- [ ] Provider-specific session settings survive binding compilation.

### Regression and Anti-Pattern Guards

- [ ] Do not reduce provider-specific config to generic stringly-typed flags.
- [ ] Do not introduce one-off mappings that bypass the typed config structs.
- [ ] Do not let provider wrappers silently ignore unsupported fields.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- Codex and Claude Code are both reachable through the new binding layer.
- Provider-specific typed config remains intact end-to-end.

---

## Prior Implementation Reference

The old TypeScript codebase has provider adapters and runtime integration that show session/config patterns:

- `~/Dev/compozy/compozy-code/packages/tools/src/implementations/` — Tool adapters for Codex and Claude Code
- `~/Dev/compozy/compozy-code/providers/core/src/` — Provider hooks, MCP bridges, tool bridges, token consumption
- `~/Dev/compozy/compozy-code/providers/runtime/src/` — OpenResponses protocol gateway, AI SDK bridge, session management

The old provider layer uses a Vercel AI SDK bridge. The new one uses Arky crates with typed bindings,
but the old adapters show how provider-specific config and session identity were handled.

## Notes

- These are the strategic providers for the first meaningful Compozy version.
