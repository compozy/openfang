## markdown

## status: pending

<task_context>
<domain>providers/arky/integration</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task1</dependencies>
</task_context>

# Task 4.0: Copy Arky Crates Into OpenFang Workspace

## Overview

Copy the required Arky SDK crates from the `~/Dev/compozy/arky/` workspace (at
`/Users/pedronauck/Dev/compozy/arky/crates/`) into the OpenFang workspace under
`crates/`. The crates to copy are: `arky-config`, `arky-protocol`,
`arky-provider`, `arky-codex`, `arky-claude-code`, `arky-error`. After copying,
register them as workspace members in `openfang/Cargo.toml` and ensure they
compile within the OpenFang workspace.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/reset-2026-03-21/README.md` and the linked technical docs before start
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- The Arky crates must be first-class members of the OpenFang workspace.
- Later provider tasks (task 10-12) depend on these crates being available locally.
</requirements>

## Subtasks

- [ ] 4.1 Copy arky crate directories from `~/Dev/compozy/arky/crates/` into `openfang/crates/`.
- [ ] 4.2 Register copied crates as workspace members in `Cargo.toml` and resolve dependency conflicts.
- [ ] 4.3 Verify all copied crates compile and their existing tests pass within the OpenFang workspace.

## Implementation Details

This task brings the Arky SDK into the OpenFang workspace as local crates. The
copy must preserve each crate's internal structure while adapting workspace-level
configuration (member lists, shared dependency versions) to the OpenFang context.
Dependency conflicts between Arky and OpenFang crates must be resolved at this
stage so downstream provider tasks can depend on the Arky crates without extra
wiring.

### Relevant Files

- `Cargo.toml` (workspace root)
- `crates/` directory
- `~/Dev/compozy/arky/crates/arky-config/`
- `~/Dev/compozy/arky/crates/arky-protocol/`
- `~/Dev/compozy/arky/crates/arky-provider/`
- `~/Dev/compozy/arky/crates/arky-codex/`
- `~/Dev/compozy/arky/crates/arky-claude-code/`
- `~/Dev/compozy/arky/crates/arky-error/`

### Dependent Files

- `crates/openfang-kernel/Cargo.toml`
- `crates/openfang-types/Cargo.toml`

## Deliverables

- Arky crates copied and compiling in OpenFang workspace
- Workspace `Cargo.toml` updated with new members
- All existing tests still pass

## Tests

### Unit Tests (Required)

- [ ] Existing arky crate tests pass after copy.
- [ ] Each copied crate builds independently within the workspace.
- [ ] Crate metadata (name, version) is consistent with workspace conventions.

### Integration Tests (Required)

- [ ] OpenFang workspace builds with arky crates as members.
- [ ] Workspace dependency resolution succeeds without version conflicts.
- [ ] Existing OpenFang integration tests still pass with the expanded workspace.

### Regression and Anti-Pattern Guards

- [ ] No existing OpenFang crate is broken by the new workspace members.
- [ ] Do not introduce path dependencies that point outside the workspace.
- [ ] Do not silently disable arky crate tests to make the build pass.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- All six Arky crates are workspace members and compile cleanly.
- No existing OpenFang crate is broken.
- Provider tasks (10-12) can depend on the local Arky crates immediately.

---

## Notes

- This task is a prerequisite for all Arky-based provider integration work.
- Keep the copied crates as close to their upstream form as practical to simplify future syncs.
