# Compozy Integration Gaps — Implementation Task Summary

## Relevant Files

### Core Implementation Files

- `crates/openfang-types/src/error.rs` - OpenFangError enum (ClassifiedError impl)
- `crates/openfang-types/src/taint.rs` - TaintSink, TaintViolation (serde derives)
- `crates/openfang-types/src/capability.rs` - CapabilityCheck (serde derives)
- `crates/openfang-types/src/agent.rs` - SessionId definition (unification)
- `crates/arky-protocol/src/id.rs` - Canonical SessionId definition
- `crates/openfang-cli/src/main.rs` - CLI command definitions and handlers
- `crates/openfang-kernel/src/workflow.rs` - Template rendering (minijinja migration)
- `crates/openfang-kernel/src/workflow_compiler.rs` - Template compilation phase
- `crates/openfang-types/src/workflow.rs` - CompiledTemplate, TemplateSegment types
- `crates/openfang-memory/src/hitl.rs` - HITL repository (timeout sweep)
- `crates/openfang-kernel/src/kernel.rs` - Background tasks (HITL timeout monitor)

### Integration Points

- `crates/openfang-api/src/routes.rs` - API handlers consumed by CLI commands
- `crates/openfang-api/src/server.rs` - Route registration and AppState
- `crates/arky-error/src/lib.rs` - ClassifiedError trait definition
- `crates/openfang-kernel/src/db_migration.rs` - Migration runner (no changes needed)

### Documentation Files

- `tasks/prd-gaps/techspec.md` - Full technical specification for all gaps

## Tasks

- [x] 44.0 Types Cleanup: Serde Derives + ClassifiedError (complexity: low)
- [x] 45.0 SessionId Unification (complexity: medium)
- [x] 46.0 CLI Commands: A2A, Peers, Budget (complexity: low)
- [x] 47.0 Minijinja Template Engine Migration (complexity: medium)
- [ ] 48.0 HITL Timeout Enforcement (complexity: low)

Notes on complexity:

- **low**: Simple, straightforward changes (configuration, text updates, single-file modifications)
- **medium**: Standard development work (new components, API endpoints, moderate integration)
- **high**: Complex implementations (multi-step features, architectural changes, complex data flows)
- **critical**: Mission-critical or blocking work (security, core architecture, major refactors)

## Task Design Rules

- Each parent task is a closed deliverable: independently shippable and reviewable
- Do not split one deliverable across multiple parent tasks; avoid cross-task coupling
- Each parent task must include unit test subtasks for this feature
- Each generated `/_task_<num>.md` must contain explicit Deliverables and Tests sections

## Execution Plan

- Critical Path: 44.0 → 45.0 → 46.0
- Parallel Track A (independent): 47.0 (Minijinja — no deps on other tasks)
- Parallel Track B (independent): 48.0 (HITL Timeout — no deps on other tasks)

Notes

- All Rust code MUST follow `rust-best-practices` skill
- Run `make fmt && make lint && make test` before marking any task as completed
- Use `cargo add` for new dependencies, never edit Cargo.toml by hand

## Batch Plan (Grouped Commits)

- [x] Batch 1 — Types & Foundations: 44.0, 45.0
- [x] Batch 2 — CLI Surface: 46.0
- [ ] Batch 3 — Engine & Runtime: 47.0, 48.0
