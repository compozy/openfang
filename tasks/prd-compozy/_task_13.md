## markdown

## status: completed

<task_context>
<domain>engine/workflow/types</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>high</complexity>
<dependencies>task5,task8</dependencies>
</task_context>

# Task 13.0: Workflow v2 Definition Types

## Overview

Define the Workflow v2 definition schema types as specified in ADR-024 (workflow
v2 public schema) and ADR-017 (workflow v2 as minimal evolution of OpenFang).
This task introduces the full Compozy-owned workflow definition model including
top-level definition fields, step kinds, flow modes, and all associated type
structures.

The current `Workflow` struct in `crates/openfang-kernel/src/workflow.rs` is a
shallow prompt-routing definition: it has `name`, `description`, a `Vec<WorkflowStep>`,
and each step is a `(StepAgent, prompt_template, StepMode, ErrorMode, timeout_secs,
output_var)` tuple. It has no:

- `version`, `enabled`, `tags`, `input`, `output`, `defaults`, or `outputs` fields
- explicit `kind` on steps (every step is implicitly an agent dispatch)
- `uses` block separating what a step targets from how it behaves
- `save_as` symbol binding or `vars` namespace
- `wait_signal`, `start_looper`, `emit_event`, `collect` (as an explicit step kind), or `noop` step kinds

The existing `StepMode` enum covers `Sequential`, `FanOut`, `Collect`, `Conditional`,
and `Loop` but mixes step kind semantics (collect is a step kind in v2) with
flow mode semantics (sequential, fan_out, conditional, loop are flow modes in v2).

This task defines the `WorkflowV2Definition`, `WorkflowV2Step`, `StepKind`, and
`FlowMode` types that form the foundation for the compile pipeline (Task 14) and
the workflow API endpoints (Task 15).

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- Workflow v2 definition types must cover all top-level fields specified in ADR-024: `id`, `name`, `version`, `description`, `enabled`, `tags`, `input`, `output`, `defaults`, `steps`, `outputs`.
- Each step must have an explicit `kind` field drawn from the eight supported step kinds: `agent`, `primitive`, `workflow`, `wait_signal`, `start_looper`, `emit_event`, `collect`, `noop`. The current implicit-agent-only model is not acceptable.
- Each step must have a `flow` block with an explicit `mode` drawn from the four supported flow modes: `sequential`, `fan_out`, `conditional`, `loop`. Mode-specific required fields (`flow.when` for conditional, `flow.until` and `flow.max_iterations` for loop) must be present in the type definitions.
- `input` and `output` on the workflow definition must use the shared definition contract schema from task 5 (`crates/openfang-types/src/` — the `ContractNode` or equivalent types introduced by task 5).
- All types must be `Serialize + Deserialize` for JSON transport per ADR-040.
- The `WorkflowV2Definition` type should live in `crates/openfang-types/` so it is accessible to both the kernel and the API crate without a circular dependency.
</requirements>

## Subtasks

- [x] 13.1 Define `WorkflowV2Definition` in `crates/openfang-types/src/workflow.rs` (new file or extension) covering all top-level fields from ADR-024, plus the full `WorkflowV2Step` struct with `id`, `name`, `kind`, `uses`, `with`, `save_as`, `flow`, and `runtime` sub-fields.
- [x] 13.2 Define `StepKind` enum with variants `Agent`, `Primitive`, `Workflow`, `WaitSignal`, `StartLooper`, `EmitEvent`, `Collect`, `Noop`, and their associated `uses` payloads (`AgentUses { agent: String }`, `PrimitiveUses { primitive: String }`, `WorkflowUses { workflow: String }`, etc.).
- [x] 13.3 Define `FlowMode` enum with variants `Sequential`, `FanOut`, `Conditional { when: String }`, `Loop { until: String, max_iterations: u32 }`, and a `FlowBlock` wrapper struct that `WorkflowV2Step.flow` uses.
- [x] 13.4 Define `WorkflowDefaults` struct with `timeout_secs` and `error_mode` fields, and the `RuntimeBlock` struct for per-step runtime overrides.
- [x] 13.5 Write unit tests validating that all types serialize and deserialize correctly through `serde_json` round-trips.

## Implementation Details

### Step Kind Semantics

Each step kind has distinct execution semantics and valid `uses` payload shapes:

- `agent`: dispatches to a named agent via `uses.agent`. Must produce a durable `agent_dispatch` record in Phase 1 (Task 16). Accepts `sequential`, `fan_out`, `conditional`, `loop` flow modes.
- `primitive`: calls a named domain primitive via `uses.primitive` (e.g., `issue.read`, `artifact.*`). Accepts all flow modes. `uses.primitive` must reference a known primitive at compile time.
- `workflow`: invokes a nested workflow via `uses.workflow`. The referenced workflow must exist in the definition registry at compile time. Accepts `sequential` and `conditional` modes only (not `fan_out` or `loop` — nested workflow fan-out is a future capability).
- `wait_signal`: suspends the run until a named signal arrives. `uses.signal_name` is required. Only `sequential` flow mode is valid. This is how the runtime implements durable pauses without polling.
- `start_looper`: launches a looper run against a task. `uses.task_ref` or `uses.task_id_binding` is required. Only `sequential` mode is valid. Produces a durable `looper_run` record.
- `emit_event`: fires a named event into the system event pipeline. `uses.event` (event name) and optional `uses.payload_template` are required. Accepts `sequential` and `fan_out` modes.
- `collect`: aggregates the outputs of preceding `fan_out` steps into the `vars` namespace. Has no `uses` block. Must only appear after at least one `fan_out` step in the same definition. Valid only with implicit sequential continuation.
- `noop`: does nothing. Used for placeholder steps during definition authoring or for conditional branches that should pass through. Accepts all flow modes.

### Flow Mode Semantics

- `sequential`: steps execute in order; each step's output becomes the next step's input context. Default mode when `flow` block is omitted (via normalize).
- `fan_out`: step executes in parallel with other consecutive `fan_out` steps. A `collect` step must follow the fan-out group.
- `conditional`: step executes only if `flow.when` evaluates to true. `flow.when` is a template expression evaluated against the current `vars` namespace.
- `loop`: step repeats until `flow.until` evaluates to true or `flow.max_iterations` is reached. Both `flow.until` and `flow.max_iterations` are required. `max_iterations` must be > 0.

### Current Codebase Starting Point

The existing types in `crates/openfang-kernel/src/workflow.rs` to migrate from:

- `Workflow` → `WorkflowV2Definition` (add all missing fields)
- `WorkflowStep` → `WorkflowV2Step` (add `id`, `kind`, `uses`, `with`, `save_as`, `flow` block)
- `StepMode` → `FlowMode` (rename and restructure; `Collect` becomes a `StepKind`, not a mode)
- `StepAgent` → part of `AgentUses` inside `StepKind::Agent`
- `ErrorMode` → retained but moved to `FlowBlock.runtime.error_mode` per step, or `WorkflowDefaults.error_mode` at definition level

### Relevant Files

- `crates/openfang-types/src/workflow.rs` — new or extended file for v2 definition types
- `crates/openfang-types/src/` — shared contract types from task 5 (`ContractNode` or equivalent)
- `tasks/prd-compozy/docs/DESIGN.md` — section 17 (Workflow v2), section 21 (Workflow v2 Public Schema)
- `tasks/prd-compozy/docs/adrs/017-workflow-v2-as-minimal-evolution-of-openfang.md`
- `tasks/prd-compozy/docs/adrs/024-workflow-v2-public-schema.md`
- `tasks/prd-compozy/docs/adrs/040-toml-authoring-json-transport-ir-execution.md`

### Dependent Files

- `crates/openfang-types/src/contract.rs` (or equivalent) from task 5 — shared contract types for `input`/`output`

## Deliverables

- `WorkflowV2Definition` and `WorkflowV2Step` types in `crates/openfang-types/src/workflow.rs` with all ADR-024 fields.
- `StepKind` enum with all eight variants and their `uses` payload types.
- `FlowMode` enum with all four variants and their mode-specific required fields.
- `WorkflowDefaults` and `RuntimeBlock` structs.
- Tests covering serialization round-trips for all types.

## Tests

### Unit Tests (Required)

- [x] `workflow_v2_definition_round_trips_through_serde`: a fully populated `WorkflowV2Definition` must round-trip through `serde_json::to_string` and `serde_json::from_str` without loss.
- [x] `step_kind_agent_serializes_correctly`: a step with `kind: agent` and `uses.agent` must serialize to the expected JSON shape.
- [x] `step_kind_all_variants_deserialize`: all eight `StepKind` variants must deserialize from their canonical JSON representations.
- [x] `flow_mode_sequential_is_default`: a `FlowBlock` with no mode specified must default to `Sequential`.
- [x] `flow_mode_conditional_requires_when`: a `Conditional` flow mode must include a `when` field.
- [x] `flow_mode_loop_requires_until_and_max_iterations`: a `Loop` flow mode must include both `until` and `max_iterations` fields.
- [x] `workflow_defaults_apply_sensible_values`: a `WorkflowDefaults` with default construction must have `timeout_secs: 120` and `error_mode: fail`.

### Verification Commands

- [x] `cargo fmt --all`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo test --workspace`

## Success Criteria

- All eight step kinds (`agent`, `primitive`, `workflow`, `wait_signal`, `start_looper`, `emit_event`, `collect`, `noop`) are representable in `WorkflowV2Definition`.
- All four flow modes (`sequential`, `fan_out`, `conditional`, `loop`) are defined with mode-specific required fields.
- All types are `Serialize + Deserialize` and round-trip correctly through JSON.
- `cargo test --workspace` passes with zero failures and `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.

---

## Prior Implementation Reference

The old TypeScript codebase has workflow/planning schemas and a prompt builder system:

- `~/Dev/compozy/compozy-code/packages/backend/src/db/schema/planning.ts` — Old planning schema (PRDs, techspecs) showing how "work items" were structured before the domain redesign
- `~/Dev/compozy/compozy-code/packages/prompts/` — Prompt builder (`builder.ts`, 52k lines) and formatter (`formatter.ts`, 15k lines) with built-in prompt categories for task execution, review, and subagents

The old model had no durable workflow runtime — the new Workflow v2 is greenfield. But the old
planning schema shows domain naming conventions, and the prompt system shows how step-like
constructs were expressed in the old product.

## Notes

- This task defines only the type layer. The compile pipeline is Task 14 and the API endpoints are Task 15.
- The `WorkflowV2Definition` type should live in `crates/openfang-types/` so it is accessible to both the kernel and the API crate without a circular dependency.
- Per ADR-017, `collect` is an explicit step kind in the public contract, while flow modes describe control behavior. The current `StepMode::Collect` variant mixes both concerns and must be cleaned up as part of this task.
