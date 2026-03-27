## markdown

## status: pending

<task_context>
<domain>engine/kernel</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>none</dependencies>
</task_context>

# Task 2: Kernel + Workflow Decomposition

## Overview

Split `crates/openfang-kernel/src/kernel.rs` (~11,229 lines) and `crates/openfang-kernel/src/workflow.rs` (~9,402 lines) into focused sub-modules while keeping `OpenFangKernel` and the current public workflow API stable.

<critical>
- **ALWAYS READ** @AGENTS.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-refac/techspec.md` and `tasks/prd-refac/analysis_kernel.md` before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Split large `impl` blocks and mixed concerns in `kernel.rs` into focused modules under `kernel/`
- Split `workflow.rs` into focused modules under `workflow/`
- Keep `OpenFangKernel` and the current public workflow API stable
- Introduce compatibility re-exports where needed
- Do NOT split `KernelHandle` into multiple traits (deferred)
- Do NOT reduce `OpenFangKernel` field count via facade structs (deferred)
- Do NOT consolidate `send_message*` entrypoints into a new shape (deferred)
</requirements>

## Subtasks

### Kernel decomposition

- [ ] 2.1 Audit `kernel.rs` to map all `impl` blocks, methods, and internal helpers by concern
- [ ] 2.2 Create `kernel/` module directory with `mod.rs` preserving public API via re-exports
- [ ] 2.3 Extract agent lifecycle methods (create, start, stop, restart) into `kernel/agent_lifecycle.rs`
- [ ] 2.4 Extract messaging methods (send_message variants, message routing) into `kernel/messaging.rs`
- [ ] 2.5 Extract session management into `kernel/session.rs`
- [ ] 2.6 Extract dispatch/orchestration logic into `kernel/dispatch.rs`
- [ ] 2.7 Extract HITL (human-in-the-loop) handling into `kernel/hitl.rs`
- [ ] 2.8 Extract MCP server/tool coordination into `kernel/mcp.rs`

### Workflow decomposition

- [ ] 2.9 Audit `workflow.rs` to map types, definition store, transition logic, and engine
- [ ] 2.10 Create `workflow/` module directory with `mod.rs` preserving public API via re-exports
- [ ] 2.11 Extract workflow type definitions into `workflow/types.rs`
- [ ] 2.12 Extract definition store logic into `workflow/definition_store.rs`
- [ ] 2.13 Extract transition writing/recording into `workflow/transition_writer.rs`
- [ ] 2.14 Extract workflow engine/execution into `workflow/engine.rs`

### Verification

- [ ] 2.15 Verify all kernel and workflow tests pass without modifications
- [ ] 2.16 Verify no downstream crate breakage (API, CLI, runtime imports)

## Implementation Details

The module boundaries listed above are targets, not mandatory exact structure. Follow the natural concern boundaries that exist in the code. If the code suggests different groupings, use those instead.

### Approach

1. Read `kernel.rs` fully to understand `impl OpenFangKernel` blocks and their concerns.
2. Identify method groupings that correspond to distinct domains (lifecycle, messaging, etc.).
3. Create `kernel/` dir and move methods group by group, running tests after each move.
4. Repeat for `workflow.rs`.
5. Ensure `mod.rs` re-exports everything that was previously public.

### Explicitly deferred (out of scope)

- Splitting `KernelHandle` into multiple traits
- Reducing `OpenFangKernel` field count via facade structs
- Consolidating all `send_message*` entrypoints into a new public shape

These are cross-cutting design changes, not file moves.

### Relevant Files

- `crates/openfang-kernel/src/kernel.rs` (~11K lines)
- `crates/openfang-kernel/src/workflow.rs` (~9.4K lines)
- `crates/openfang-kernel/src/lib.rs` (module declarations)

### Dependent Files

- `crates/openfang-api/src/routes.rs` (imports kernel types)
- `crates/openfang-runtime/` (uses KernelHandle)
- `crates/openfang-cli/` (kernel bootstrap)

## Deliverables

- `kernel/` module directory with concern-separated files
- `workflow/` module directory with concern-separated files
- `mod.rs` for each with re-exports preserving current public API
- All existing tests passing without modification

## Tests

### Unit Tests (Required)

- [ ] All existing kernel tests pass in their new module locations
- [ ] All existing workflow tests pass in their new module locations
- [ ] Re-exports cover all previously public items

### Integration Tests (Required)

- [ ] Downstream crates (API, runtime, CLI) compile without changes
- [ ] End-to-end agent lifecycle tests pass

### Regression and Anti-Pattern Guards

- [ ] No behavioral changes to any kernel method
- [ ] No public API surface changes
- [ ] No new cross-crate dependencies introduced

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- `kernel.rs` and `workflow.rs` no longer exist as monolithic files
- Each concern area lives in its own module
- `OpenFangKernel` public API is unchanged
- Workflow public API is unchanged
- Zero warnings, zero errors on `make fmt && make lint && make test`
