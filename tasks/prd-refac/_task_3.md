## markdown

## status: pending

<task_context>
<domain>engine/infra</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>medium</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 3: Config + OpenClaw Decomposition

## Overview

Split `crates/openfang-types/src/config.rs` (~4,321 lines) into a `config/` module structure and `crates/openfang-migrate/src/openclaw.rs` (~4,608 lines) into an `openclaw/` module structure. Both files combine unrelated concerns that should be separated.

<critical>
- **ALWAYS READ** @AGENTS.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-refac/techspec.md` and `tasks/prd-refac/analysis_supporting.md` before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Split `config.rs` into a `config/` module separating channel config types, web/integration/security config, and validation helpers
- Split `openclaw.rs` into an `openclaw/` module separating types, conversion helpers, and migration orchestration
- Preserve all existing public API surfaces via re-exports
- Do NOT include `openfang-channels` adapter-base redesign (deferred)
- Do NOT include `openfang-agent-definition` splitting (deferred)
- Do NOT include `arky-config` cleanup (deferred)
- Do NOT include dependency cleanup in `Cargo.toml` (deferred)
- Do NOT include global error-trait alignment (deferred)
</requirements>

## Subtasks

### Config decomposition

- [ ] 3.1 Audit `config.rs` to map all types, impls, validation, and test blocks by concern
- [ ] 3.2 Create `config/` module directory with `mod.rs` preserving public API via re-exports
- [ ] 3.3 Extract channel configuration types into `config/channels.rs`
- [ ] 3.4 Extract web, integration, and security config types into `config/server.rs`
- [ ] 3.5 Extract kernel/runtime config types into `config/kernel.rs`
- [ ] 3.6 Extract validation helpers into `config/validation.rs`
- [ ] 3.7 Move config tests to appropriate sub-modules

### OpenClaw decomposition

- [ ] 3.8 Audit `openclaw.rs` to map types, conversion logic, orchestration, and tests
- [ ] 3.9 Create `openclaw/` module directory with `mod.rs` preserving public API via re-exports
- [ ] 3.10 Extract OpenClaw type definitions into `openclaw/types.rs`
- [ ] 3.11 Extract conversion/mapping helpers into `openclaw/conversion.rs`
- [ ] 3.12 Extract migration orchestration logic into `openclaw/orchestration.rs`
- [ ] 3.13 Move openclaw tests to appropriate sub-modules

### Verification

- [ ] 3.14 Verify all config and migration tests pass without modifications
- [ ] 3.15 Verify no downstream crate breakage

## Implementation Details

### Approach

1. Read `config.rs` fully to understand the type groupings.
2. Identify natural boundaries: channel configs are distinct from web/security configs, which are distinct from validation.
3. Create `config/` dir, move types group by group, run tests after each move.
4. Repeat for `openclaw.rs`.
5. Ensure `mod.rs` re-exports everything that was previously public.

### Explicitly deferred (out of scope)

- `openfang-channels` adapter-base redesign
- `openfang-agent-definition` splitting
- `arky-config` cleanup
- Dependency cleanup in `Cargo.toml`
- Global error-trait alignment

### Relevant Files

- `crates/openfang-types/src/config.rs` (~4.3K lines)
- `crates/openfang-types/src/lib.rs`
- `crates/openfang-migrate/src/openclaw.rs` (~4.6K lines)
- `crates/openfang-migrate/src/lib.rs`

### Dependent Files

- `crates/openfang-kernel/` (imports config types)
- `crates/openfang-api/` (imports config types)
- `crates/openfang-cli/` (imports config types)

## Deliverables

- `config/` module directory with concern-separated files in openfang-types
- `openclaw/` module directory with concern-separated files in openfang-migrate
- `mod.rs` for each with re-exports preserving current public API
- All existing tests passing without modification

## Tests

### Unit Tests (Required)

- [ ] All existing config tests pass in their new module locations
- [ ] All existing openclaw tests pass in their new module locations
- [ ] Re-exports cover all previously public items

### Integration Tests (Required)

- [ ] Downstream crates compile without changes
- [ ] Config deserialization from TOML works identically

### Regression and Anti-Pattern Guards

- [ ] No behavioral changes to config parsing or validation
- [ ] No behavioral changes to migration logic
- [ ] No public API surface changes

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `config.rs` and `openclaw.rs` no longer exist as monolithic files
- Each concern area lives in its own module
- All public APIs unchanged
- Zero warnings, zero errors on `make fmt && make lint && make test`
