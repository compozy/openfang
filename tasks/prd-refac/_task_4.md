## markdown

## status: pending

<task_context>
<domain>engine/cli</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 4: CLI/TUI Decomposition

## Overview

Split `crates/openfang-cli/src/main.rs` (~13,076 lines) into smaller command/domain modules and consolidate repeated `spawn_*` patterns in `crates/openfang-cli/src/tui/event.rs` (~2,786 lines).

<critical>
- **ALWAYS READ** @AGENTS.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-refac/techspec.md` and `tasks/prd-refac/analysis_cli.md` before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Break `main.rs` into smaller command/domain modules under `commands/`
- Extract repeated daemon-client and command-dispatch boilerplate
- Consolidate the repeated `spawn_*` pattern in the TUI event layer
- Preserve current CLI command names, flags, and output behavior
- Preserve current daemon-vs-in-process behavior
- Do NOT combine structural decomposition with UX redesign
</requirements>

## Subtasks

### CLI main.rs decomposition

- [ ] 4.1 Audit `main.rs` to map all command handlers, shared helpers, and daemon logic
- [ ] 4.2 Create `commands/` module directory
- [ ] 4.3 Extract CLI entrypoint and argument parsing into `cli.rs`
- [ ] 4.4 Extract daemon client/connection logic into `daemon.rs`
- [ ] 4.5 Extract shared command helpers into `helpers.rs`
- [ ] 4.6 Move init command handlers into `commands/init.rs`
- [ ] 4.7 Move agent command handlers into `commands/agent.rs`
- [ ] 4.8 Move workflow command handlers into `commands/workflow.rs`
- [ ] 4.9 Move task command handlers into `commands/task.rs`
- [ ] 4.10 Move config command handlers into `commands/config.rs`
- [ ] 4.11 Move system/misc command handlers into `commands/system.rs`
- [ ] 4.12 Update `main.rs` to be a thin entrypoint that dispatches to command modules

### TUI event.rs consolidation

- [ ] 4.13 Audit `tui/event.rs` to identify the repeated `spawn_*` / background-fetch pattern
- [ ] 4.14 Extract shared spawn/fetch logic into focused helpers or sibling modules
- [ ] 4.15 Refactor repeated instances to use the shared helpers

### Verification

- [ ] 4.16 Verify all CLI tests pass without modifications
- [ ] 4.17 Verify all CLI command names, flags, and output are preserved

## Implementation Details

### Approach

1. Read `main.rs` fully to understand the command structure and what each handler does.
2. Identify shared patterns: daemon connection, output formatting, error display.
3. Create `commands/` dir and move handlers one command group at a time.
4. After each move, run `make test` to catch breakage early.
5. Then tackle `tui/event.rs` — identify the repeated spawn pattern and extract a common helper.
6. Refactor each spawn site to use the helper.

### Coordination note

This crate is an active work surface. The implementation should be tightly scoped slices to minimize collisions with ongoing CLI work.

### Relevant Files

- `crates/openfang-cli/src/main.rs` (~13K lines)
- `crates/openfang-cli/src/tui/event.rs` (~2.8K lines)
- `crates/openfang-cli/src/lib.rs` (if exists)

### Dependent Files

- `crates/openfang-cli/src/tui/` (other TUI modules)
- Any integration tests for CLI commands

## Deliverables

- `commands/` module directory with command-separated handler files
- `cli.rs` for argument parsing / entrypoint logic
- `daemon.rs` for daemon client boilerplate
- `helpers.rs` for shared command utilities
- Slim `main.rs` acting as thin dispatcher
- Consolidated spawn helpers in TUI event layer
- All existing tests passing without modification

## Tests

### Unit Tests (Required)

- [ ] All existing CLI tests pass in their new module locations
- [ ] All existing TUI tests pass after event.rs refactoring

### Integration Tests (Required)

- [ ] CLI command names and flags are identical
- [ ] CLI output format is unchanged
- [ ] Daemon-vs-in-process behavior is preserved

### Regression and Anti-Pattern Guards

- [ ] No CLI UX changes (command names, flags, output)
- [ ] No daemon behavior changes
- [ ] No new dependencies introduced

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `main.rs` is a thin entrypoint (~100-200 lines), not a 13K monolith
- Command handlers live in domain-specific modules under `commands/`
- TUI event.rs spawn patterns are consolidated into shared helpers
- All CLI behavior is preserved
- Zero warnings, zero errors on `make fmt && make lint && make test`
