# Technical Specification: OpenFang Codebase Refactoring

## Status

Reviewed on 2026-03-27 against:

- `tasks/prd-refac/analysis_api.md`
- `tasks/prd-refac/analysis_kernel.md`
- `tasks/prd-refac/analysis_runtime.md`
- `tasks/prd-refac/analysis_memory.md`
- `tasks/prd-refac/analysis_cli.md`
- `tasks/prd-refac/analysis_supporting.md`

This document is the execution spec for the refactoring effort. The analysis files are the evidence base; this spec narrows that evidence into a sequence that is realistic to implement.

## Objective

OpenFang has several verified maintainability hotspots: oversized files, repeated helper code, and a few interfaces that have grown beyond a reasonable scope. The goal of this refactoring is to reduce those maintenance costs without mixing the work with product changes, protocol changes, or large architectural redesigns.

This spec replaces an earlier draft that mixed three different kinds of work under one umbrella:

1. Purely mechanical file decomposition.
2. Local deduplication with behavior preserved.
3. Cross-cutting redesigns that would change architecture, behavior, or rollout risk.

Those categories need different acceptance criteria and should not be planned as one undifferentiated stream.

## Verified Problem Statement

The analysis documents and spot checks on the current branch agree on the core problem areas:

| Area | Verified hotspot | Why it matters |
|------|------------------|----------------|
| API | `crates/openfang-api/src/routes.rs` is 26,360 lines | One file owns nearly every HTTP concern, plus helpers and inline tests. It is a review and merge bottleneck. |
| Kernel | `crates/openfang-kernel/src/kernel.rs` is 11,229 lines; `workflow.rs` is 9,402 lines | Core orchestration logic is concentrated in two files, which increases coupling and makes targeted changes harder to isolate. |
| Runtime | `agent_loop.rs` is 4,556 lines, `model_catalog.rs` is 4,250 lines, `tool_runner.rs` is 3,988 lines | Sync/streaming duplication, data-as-code, and oversized dispatch functions create high edit cost. |
| Memory | Several store files duplicate helpers such as `lock_conn`, JSON serialization helpers, cursor helpers, and repeated SQL column lists | The duplication is local, repetitive, and high-confidence to remove. |
| Types | `crates/openfang-types/src/config.rs` is 4,321 lines | Channel configs, kernel config, validation, helpers, and tests are mixed into one file. |
| Migrate | `crates/openfang-migrate/src/openclaw.rs` is 4,608 lines | Types, conversion logic, orchestration, and tests are interleaved in one module. |
| CLI/TUI | `crates/openfang-cli/src/main.rs` is 13,076 lines and `tui/event.rs` is 2,786 lines | This is one of the largest maintainability hotspots in the repo. It should be part of the refactoring plan, but it needs explicit coordination because the crate is high-churn. |

Notes:

- Exact line counts will drift. They are useful as a baseline, not as hard acceptance criteria.
- The earlier draft overstated some findings. For example, a 78-variant enum is a large coordination type, not a "God Object".

## Scope

### In Scope

- Split verified monolithic source files into module directories or smaller sibling modules.
- Remove high-confidence duplication inside a crate when the behavior can remain unchanged.
- Introduce compatibility re-exports or wrapper entrypoints where needed to keep downstream churn low.
- Land the work as incremental, independently verifiable slices.

### Out of Scope for This Refactoring Stream

- Feature work, UX changes, endpoint redesigns, or protocol changes.
- New storage semantics or query-behavior changes.
- Broad error-model unification across all `openfang-*` crates.
- Large trait redesigns that cut across crate boundaries.
- A full `openfang-channels` framework rewrite across all adapters.

## Refactoring Principles

1. Separate mechanical refactors from semantic changes.
2. Keep module boundaries aligned with existing domains instead of forcing an exact target file count.
3. Prefer one hotspot per PR or commit series.
4. Preserve current behavior first; pursue architectural cleanup later.
5. Do not bundle unrelated cleanup items just because they are easy.

## Planned Workstreams

### Workstream A: API Route Decomposition

**Targets**

- `crates/openfang-api/src/routes.rs`
- `crates/openfang-api/src/server.rs`

**Intent**

- Split `routes.rs` by existing route families.
- Move shared response helpers and test support into focused modules.
- Break the router construction in `server.rs` into per-domain sub-routers merged at the top level.

**Expected module shape**

The final split should follow the current route domains, for example:

- `routes/agents_v1.rs`
- `routes/workflows_v1.rs`
- `routes/runs.rs`
- `routes/triggers_v1.rs`
- `routes/schedules_v1.rs`
- `routes/tasks_v1.rs`
- `routes/packs_v1.rs`
- `routes/channels.rs`
- `routes/system.rs`
- `routes/test_support.rs`

The exact file list may differ. The requirement is domain separation, not a fixed count of files.

**Acceptance constraints**

- No path changes.
- No request or response shape changes.
- No auth, middleware, or SSE contract changes.
- Existing API integration tests should continue to pass without rewriting expected behavior.

### Workstream B: Kernel and Workflow Decomposition

**Targets**

- `crates/openfang-kernel/src/kernel.rs`
- `crates/openfang-kernel/src/workflow.rs`

**Intent**

- Split large `impl` blocks and mixed concerns into focused modules.
- Keep `OpenFangKernel` and the current public workflow API stable while moving code behind the module boundary.

**Expected module shape**

Examples of reasonable boundaries:

- `kernel/agent_lifecycle.rs`
- `kernel/messaging.rs`
- `kernel/session.rs`
- `kernel/dispatch.rs`
- `kernel/hitl.rs`
- `kernel/mcp.rs`
- `workflow/types.rs`
- `workflow/definition_store.rs`
- `workflow/transition_writer.rs`
- `workflow/engine.rs`

Again, these are target boundaries, not a mandatory exact structure.

**Important correction**

The earlier draft treated deeper architectural changes as if they were part of the same low-risk refactor. They are not. The following items are explicitly deferred:

- Splitting `KernelHandle` into multiple traits.
- Reducing `OpenFangKernel` field count via facade structs.
- Consolidating all `send_message*` entrypoints into a new public shape.

Those may be worthwhile later, but they are cross-cutting design changes, not just file moves.

### Workstream C: Runtime Deduplication

**Targets**

- `crates/openfang-runtime/src/agent_loop.rs`
- `crates/openfang-runtime/src/tool_runner.rs`
- `crates/openfang-runtime/src/model_catalog.rs`

**Intent**

- Remove obvious local duplication and oversized argument lists.
- Improve navigability without forcing a risky redesign.

**Planned changes**

- Unify the shared body of `run_agent_loop` and `run_agent_loop_streaming` behind a mode/context abstraction.
- Introduce a context object for `execute_tool`.
- Split `tool_runner.rs` by domain or category.
- Extract the model and provider catalog behind a dedicated loading boundary.

**Important correction**

The earlier draft hard-committed to JSON files for catalog data. That is a valid option, but it is more specific than the spec needs to be. The real requirement is to stop treating thousands of lines of provider/model entries as hand-maintained inline code.

Acceptable implementations include:

- Embedded JSON or TOML loaded with `include_str!`
- Smaller Rust data modules grouped by provider family
- Generated code if it remains deterministic and easy to review

The implementation should be chosen for the smallest safe diff, not because one format sounds cleaner in the abstract.

### Workstream D: Memory Deduplication

**Targets**

- `crates/openfang-memory/src/workflow_store.rs`
- `crates/openfang-memory/src/task.rs`
- `crates/openfang-memory/src/dispatch.rs`
- `crates/openfang-memory/src/hitl.rs`
- `crates/openfang-memory/src/looper.rs`
- `crates/openfang-memory/src/artifact.rs`
- `crates/openfang-memory/src/doc.rs`
- `crates/openfang-memory/src/runtime_store.rs`
- `crates/openfang-memory/src/pack.rs`

**Intent**

- Extract repeated helpers and constants into shared modules.
- Remove high-confidence duplication without changing storage semantics.

**Planned changes**

- Shared connection-lock helper.
- Shared JSON serialization helpers.
- Shared cursor helpers where the representation is genuinely common.
- Shared SQL column constants for repeated select lists.

**Important correction**

The previous draft included moving `workflow_store::list_runs` filtering into SQL as if it were part of the same mechanical cleanup. It is not. That change alters how filtering is executed and should be treated as a separate optimization or correctness task with its own validation. It is therefore out of scope for this spec.

### Workstream E: Supporting Crates

**Targets**

- `crates/openfang-types/src/config.rs`
- `crates/openfang-migrate/src/openclaw.rs`

**Intent**

- Split large files that combine unrelated concerns.

**Planned changes**

- Move `config.rs` into a `config/` module structure that separates channel config types, web/integration/security config, and validation helpers.
- Move `openclaw.rs` into an `openclaw/` module structure that separates types, conversion helpers, and migration orchestration.

**Important correction**

The earlier draft also tried to absorb:

- a full `openfang-channels` adapter-base redesign,
- `openfang-agent-definition` splitting,
- `arky-config` cleanup,
- dependency cleanup in `Cargo.toml`,
- global error-trait alignment.

Those are not part of the same execution slice. They should remain follow-on work, not gating work.

### Workstream F: CLI/TUI Decomposition

**Targets**

- `crates/openfang-cli/src/main.rs`
- `crates/openfang-cli/src/tui/event.rs`

**Intent**

- Break `main.rs` into smaller command/domain modules.
- Extract repeated daemon-client and command-dispatch boilerplate.
- Consolidate the repeated `spawn_*` pattern in the TUI event layer.

**Expected module shape**

Reasonable boundaries include:

- `cli.rs`
- `daemon.rs`
- `home.rs`
- `helpers.rs`
- `commands/init.rs`
- `commands/agent.rs`
- `commands/workflow.rs`
- `commands/task.rs`
- `commands/config.rs`
- `commands/system.rs`

For the TUI layer:

- `tui/event.rs` may stay as the entrypoint, but the repeated background-fetch logic should move into focused helpers or sibling modules.

**Acceptance constraints**

- Preserve current CLI command names, flags, and output behavior unless a separate spec says otherwise.
- Preserve current daemon-vs-in-process behavior.
- Do not combine structural decomposition with UX redesign in the same slice.

**Coordination note**

This crate is large enough to belong in the refactoring program, but it is also an active work surface. The implementation should therefore be split into smaller, tightly scoped slices and coordinated to minimize collisions with ongoing CLI work.

## Deferred Follow-On Topics

These are valid ideas from the analysis set, but they should not be presented as part of the initial low-risk refactoring stream:

| Topic | Why deferred |
|------|---------------|
| `openfang-channels` base abstractions for all adapters | High fan-out change touching many integrations; needs a pilot on a small adapter subset first. |
| `KernelHandle` trait decomposition | Cross-crate API redesign; not a file split. |
| `OpenFangKernel` facade extraction | Architecture change with ownership and lifecycle implications. |
| `send_message*` API consolidation | Public behavior and call-path change. |
| SQL-backed filtering for `list_runs` | Behavior/performance change; requires separate validation. |
| Deprecation headers for legacy endpoints | API contract change, not structural refactor. |
| Adopting `ClassifiedError` across all `openfang-*` crates | Cross-cutting error-model change; also partly incomplete in the current draft because some crates already implement it. |

## Sequencing

### Phase 0: Guardrails and Baseline

- Confirm hotspot files and current line counts.
- Preserve test coverage before moving code.
- For every slice, define whether the change is mechanical or behavioral before implementation starts.

### Phase 1: Mechanical File Splits

Recommended order:

1. API route decomposition.
2. Kernel decomposition.
3. Workflow decomposition.
4. Types config decomposition.
5. OpenClaw decomposition.
6. CLI main-file decomposition, when the active CLI workstream is coordinated.

These slices are high leverage and mostly structural. They should be implemented in separate PRs or commit groups.

### Phase 2: Local Deduplication

Recommended order:

1. Runtime agent-loop deduplication.
2. Tool runner context extraction and domain split.
3. Model catalog extraction behind a loading boundary.
4. Memory shared-helper extraction.

These slices are still intended to preserve behavior, but they touch denser logic than Phase 1 and deserve smaller review batches.

### Phase 3: Follow-On Design Work

Only after Phases 1 and 2 stabilize should the team evaluate:

- channel adapter base abstractions,
- kernel trait redesign,
- deeper kernel facade extraction,
- SQL-level query optimizations,
- broader CLI/TUI redesign beyond structural decomposition.

Those need separate specs or task docs.

## Verification Requirements

Every implementation slice derived from this spec must pass:

1. `make fmt`
2. `make lint`
3. `make test`

Additional requirements:

- Any API/router slice must keep existing route tests green.
- Any server wiring change must include live API smoke tests because route registration mistakes are easy to miss in unit tests.
- Large file moves should be reviewed with content-parity discipline: avoid mixing structural moves and logic rewrites unless the slice explicitly says so.

## Success Criteria

This refactoring effort is successful if it produces the following outcomes:

- The highest-risk hotspot files are no longer single-file ownership bottlenecks.
- Obvious local duplication in runtime and memory is removed without behavior drift.
- Future architectural work can be planned on top of clearer module boundaries instead of inside monolith files.
- Each merged slice is independently understandable, testable, and reversible.

## References

- [API Layer Analysis](analysis_api.md)
- [Kernel Analysis](analysis_kernel.md)
- [Runtime Analysis](analysis_runtime.md)
- [Memory Analysis](analysis_memory.md)
- [CLI/TUI Analysis](analysis_cli.md)
- [Supporting Crates Analysis](analysis_supporting.md)
