## markdown

## status: pending

<task_context>
<domain>engine/runtime</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>_task_1,_task_2,_task_3,_task_4</dependencies>
</task_context>

# Task 5: Runtime + Memory Deduplication

## Overview

Remove local duplication in the runtime crate (`agent_loop.rs`, `tool_runner.rs`, `model_catalog.rs`) and extract shared helpers across memory store files. This is Phase 2 work — it preserves behavior but touches denser logic than the Phase 1 mechanical splits.

<critical>
- **ALWAYS READ** @AGENTS.md before start - **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-refac/techspec.md`, `tasks/prd-refac/analysis_runtime.md`, and `tasks/prd-refac/analysis_memory.md` before start
- **YOU CAN ONLY** finish when `make fmt`, `make lint`, and `make test` pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Unify the shared body of `run_agent_loop` and `run_agent_loop_streaming` behind a mode/context abstraction
- Introduce a context object for `execute_tool` to reduce oversized argument lists
- Split `tool_runner.rs` by domain or category
- Extract the model/provider catalog behind a dedicated loading boundary
- Extract shared connection-lock, JSON serialization, cursor, and SQL column helpers from memory stores
- Do NOT change storage semantics or query behavior
- Do NOT move `workflow_store::list_runs` filtering into SQL (deferred)
- Catalog extraction format is implementer's choice (JSON/TOML with include_str!, smaller Rust modules, or codegen) — pick the smallest safe diff
</requirements>

## Subtasks

### Runtime: agent_loop deduplication

- [ ] 5.1 Audit `agent_loop.rs` to map the shared vs divergent code between sync and streaming paths
- [ ] 5.2 Design a mode/context abstraction that captures the sync vs streaming difference
- [ ] 5.3 Unify `run_agent_loop` and `run_agent_loop_streaming` behind the abstraction
- [ ] 5.4 Verify both sync and streaming paths produce identical behavior to before

### Runtime: tool_runner refactoring

- [ ] 5.5 Audit `tool_runner.rs` to map `execute_tool` argument lists and tool dispatch domains
- [ ] 5.6 Introduce a `ToolExecutionContext` (or similar) struct to replace oversized argument lists
- [ ] 5.7 Split `tool_runner.rs` into domain/category modules (e.g., file tools, web tools, system tools)
- [ ] 5.8 Verify all tool execution paths work identically

### Runtime: model_catalog extraction

- [ ] 5.9 Audit `model_catalog.rs` to understand the inline provider/model data structure
- [ ] 5.10 Extract catalog data behind a loading boundary (choose format for smallest safe diff)
- [ ] 5.11 Verify model/provider resolution behavior is identical

### Memory: shared helper extraction

- [ ] 5.12 Audit memory store files to catalog all duplicated helpers:
  - `lock_conn` patterns across stores
  - JSON serialization/deserialization helpers
  - Cursor helpers for pagination
  - Repeated SQL column lists in SELECT statements
- [ ] 5.13 Create shared helper module(s) in openfang-memory (e.g., `helpers.rs` or `common/`)
- [ ] 5.14 Extract shared connection-lock helper
- [ ] 5.15 Extract shared JSON serialization helpers
- [ ] 5.16 Extract shared cursor helpers where representation is genuinely common
- [ ] 5.17 Extract shared SQL column constants for repeated select lists
- [ ] 5.18 Migrate all store files to use the shared helpers
- [ ] 5.19 Verify all store operations produce identical results

### Verification

- [ ] 5.20 Full test suite passes with no behavioral changes
- [ ] 5.21 No downstream crate breakage

## Implementation Details

### Approach

Work in this order (per techspec sequencing):
1. Runtime agent_loop deduplication first
2. Tool runner context extraction and domain split
3. Model catalog extraction behind loading boundary
4. Memory shared helper extraction last

Each sub-area should be verified independently before moving to the next.

### Important constraints

- **agent_loop**: The sync vs streaming unification should be a mode/context abstraction, not a full rewrite. The goal is to remove the duplicated body, not redesign the loop.
- **tool_runner**: The context object replaces long argument lists. The domain split follows existing tool categories.
- **model_catalog**: Do NOT hard-commit to JSON files. Pick whatever format produces the smallest safe diff (embedded JSON/TOML with `include_str!`, smaller Rust modules, or codegen).
- **memory**: Do NOT change how filtering works (e.g., moving `list_runs` to SQL). Only extract genuinely shared helpers.

### Relevant Files

Runtime:
- `crates/openfang-runtime/src/agent_loop.rs` (~4,556 lines)
- `crates/openfang-runtime/src/tool_runner.rs` (~3,988 lines)
- `crates/openfang-runtime/src/model_catalog.rs` (~4,250 lines)

Memory:
- `crates/openfang-memory/src/workflow_store.rs`
- `crates/openfang-memory/src/task.rs`
- `crates/openfang-memory/src/dispatch.rs`
- `crates/openfang-memory/src/hitl.rs`
- `crates/openfang-memory/src/looper.rs`
- `crates/openfang-memory/src/artifact.rs`
- `crates/openfang-memory/src/doc.rs`
- `crates/openfang-memory/src/runtime_store.rs`
- `crates/openfang-memory/src/pack.rs`

### Dependent Files

- `crates/openfang-kernel/` (calls runtime functions, uses memory stores)
- `crates/openfang-api/` (calls memory stores)

## Deliverables

- Unified agent loop with mode/context abstraction (no duplicated body)
- `ToolExecutionContext` struct replacing oversized argument lists
- `tool_runner/` module directory with domain-separated tool handlers
- Model catalog behind a loading boundary (format is implementer's choice)
- Shared helper module(s) in openfang-memory
- All store files using shared helpers instead of local duplicates
- All existing tests passing without modification

## Tests

### Unit Tests (Required)

- [ ] Agent loop sync path produces identical output to before
- [ ] Agent loop streaming path produces identical output to before
- [ ] All tool execution tests pass with new context object
- [ ] Model/provider resolution returns identical results
- [ ] All memory store operations produce identical results

### Integration Tests (Required)

- [ ] End-to-end agent message flow works (sync and streaming)
- [ ] Tool execution in real agent loops works correctly
- [ ] Memory store read/write operations are unchanged

### Regression and Anti-Pattern Guards

- [ ] No storage semantics changes
- [ ] No query behavior changes
- [ ] No new tool behaviors or removed tools
- [ ] No model/provider resolution changes

### Verification Commands

- [ ] `make fmt`
- [ ] `make lint`
- [ ] `make test`

## Success Criteria

- Agent loop no longer has two near-identical function bodies
- `execute_tool` uses a context struct instead of 10+ arguments
- Tool runner is split by domain, not a single 4K-line file
- Model catalog data is behind a loading boundary, not inline code
- Memory stores share common helpers instead of copy-pasting them
- Zero warnings, zero errors on `make fmt && make lint && make test`
