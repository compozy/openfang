## markdown

## status: pending

<task_context>
<domain>engine/workflow/compile</domain>
<type>implementation</type>
<scope>core_feature</scope>
<complexity>critical</complexity>
<dependencies>task13</dependencies>
</task_context>

# Task 14.0: Workflow v2 Compile Pipeline

## Overview

Implement the three-phase validate-normalize-compile pipeline for Workflow v2
definitions. This task builds on the type definitions from Task 13 and produces
the `WorkflowIr` type that is the sole input to the runtime executor.

The pipeline is required by ADR-040 (TOML authoring, JSON transport, IR
execution) and ADR-041 (bounded layered definition validation). Per ADR-021
(runtime-first hardening), the compile pipeline is a Phase 1 foundation that
enables later run-durability work to operate on stable, pre-validated input.

The three phases are:

1. **Validate** — schema, reference, and semantic checks that return actionable
   `ValidationIssue` objects with `severity`, `code`, `path`, and `message`.
2. **Normalize** — fill defaults, canonicalize aliases, produce a stable
   `NormalizedWorkflow`.
3. **Compile** — walk the normalized form, resolve all `save_as` symbols,
   validate all template references, and produce `WorkflowIr`.

The runtime executor (`WorkflowEngine::execute_run` or its successor) must be
updated to accept only `WorkflowIr`, never raw definition structs.

<critical>
- **ALWAYS READ** `CLAUDE.md` before start — **MANDATORY SKILLS** must be checked for your domain
- **ALWAYS READ** `tasks/prd-compozy/docs/README.md` and the linked technical docs before start
- **ALWAYS READ** `tasks/prd-compozy/_techspec.md` for the full architecture specification summary
- **YOU CAN ONLY** finish when `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` all pass
- **IF YOU DON'T CHECK SKILLS** your task will be invalid
</critical>

<requirements>
- The compile pipeline must run in three phases: (1) validate — schema, reference, and semantic checks that return actionable `ValidationIssue` objects with `severity`, `code`, `path`, and `message`; (2) normalize — fill defaults, canonicalize aliases, produce a stable `NormalizedWorkflow`; (3) compile — produce a `WorkflowIr` that is the sole input to the runtime executor.
- `save_as` and `outputs` symbol resolution must be checked at compile time. A `save_as` value introduces a symbol into the `vars` namespace. An `outputs` entry that references `{{ vars.foo }}` must fail compilation if no step has `save_as: "foo"`. Dangling references must produce a `ValidationIssue` with `severity: error`.
- `input` and `output` on the workflow definition must use the shared definition contract schema from task 5 (`crates/openfang-types/src/` — the `ContractNode` or equivalent types introduced by task 5). The compile pipeline must validate that `outputs` bindings are structurally compatible with the `output` contract.
- The validate pipeline must not perform any network calls, provider boots, template execution, or full dry-runs per ADR-041. It must be pure and synchronous for schema and reference checks, and may be async only if reference resolution requires reading from the local file-backed definition store.
- Step kind and flow mode combinations must be validated for compatibility: `wait_signal` steps must use `sequential` mode only; `start_looper` steps must use `sequential` mode only; `collect` steps must only appear after one or more `fan_out` steps; `noop` steps are valid in any mode.
- The runtime executor must accept only `WorkflowIr` as its execution input after this task is complete.
</requirements>

## Subtasks

- [ ] 14.1 Implement the validate phase in `crates/openfang-kernel/src/workflow.rs` (or a new `crates/openfang-kernel/src/workflow_compiler.rs`): schema validation (required fields, known enums), reference validation (agents and primitives referenced by steps must exist in the definition registry), semantic validation (step kind + flow mode compatibility, loop termination requirements, conditional `when` field presence, `collect` placement rules).
- [ ] 14.2 Implement the normalize phase: fill `defaults.timeout_secs` (default: 120) and `defaults.error_mode` (default: `fail`) into steps that do not override them; canonicalize `text` → `string` and `json` → `any` kind aliases in the `input`/`output` contract; produce `NormalizedWorkflow`.
- [ ] 14.3 Implement the compile phase: walk the `NormalizedWorkflow`, resolve all `save_as` symbols, validate all `outputs` and `with` template references against the `vars` namespace and the `input` contract, and produce `WorkflowIr`. The `WorkflowIr` must carry all information needed by the runtime executor without re-parsing the definition.
- [ ] 14.4 Define the `WorkflowIr` struct with: resolved step sequence with all defaults filled in, symbol table mapping `save_as` names to their originating step IDs, resolved `outputs` projection mapping output field names to their source expressions, validated `input` and `output` contract nodes, workflow-level defaults, and `workflow_id`/`workflow_version` for durable run records.
- [ ] 14.5 Update the runtime executor (`WorkflowEngine::execute_run` or its successor) to accept `WorkflowIr` as its input rather than the raw `Workflow` struct. The old `Workflow` struct may be retained as a legacy internal representation during the migration but must not be the execution input.
- [ ] 14.6 Write comprehensive unit tests for all compile error paths and symbol resolution.

## Implementation Details

### Validation Rules

Schema validation:
- All required fields present on `WorkflowV2Definition` and `WorkflowV2Step`.
- All enum values are known (step kinds, flow modes, error modes).

Reference validation:
- Agents referenced by `uses.agent` must exist in the definition registry.
- Primitives referenced by `uses.primitive` must be known.
- Workflows referenced by `uses.workflow` must exist in the definition registry.

Semantic validation:
- `wait_signal` steps must use `sequential` mode only.
- `start_looper` steps must use `sequential` mode only.
- `collect` steps must only appear after one or more `fan_out` steps.
- `workflow` steps accept `sequential` and `conditional` modes only.
- `loop` steps must have `flow.until` and `flow.max_iterations` (> 0).
- `conditional` steps must have `flow.when`.

### Symbol Resolution And `save_as`/`outputs`

The `vars` namespace is built incrementally as the compile phase walks steps:

1. `input` fields are added to the symbol table as `input.<field_name>`.
2. For each step with `save_as: "foo"`, the symbol `vars.foo` is added to the table after that step.
3. Template expressions in `with` and `outputs` using `{{ vars.foo }}` or `{{ input.bar }}` are checked against the symbol table at the point where they are used.
4. A forward reference (using `{{ vars.foo }}` before the step with `save_as: "foo"`) must produce a `ValidationIssue` with `code: "forward_reference"` and `severity: error`.
5. An `outputs` entry referencing a symbol not present in the final symbol table must produce `code: "dangling_reference"` and `severity: error`.

### Compile Pipeline Output: `WorkflowIr`

The `WorkflowIr` struct must carry:

- Resolved step sequence with all defaults filled in.
- Symbol table mapping `save_as` names to their originating step IDs.
- Resolved `outputs` projection mapping output field names to their source expressions.
- Validated `input` and `output` contract nodes (from the shared contract schema).
- Workflow-level defaults (`timeout_secs`, `error_mode`).
- The `workflow_id` and `workflow_version` for use in durable run records.

The IR must be `Serialize + Deserialize` so it can be cached to disk or stored as a compiled projection (per ADR-040). It must not embed file paths or runtime-local state.

### Current Codebase Starting Point

The `WorkflowEngine` continues to own the in-memory registry of `WorkflowV2Definition`
objects. The compile pipeline produces `WorkflowIr` objects that are cached alongside
the definitions. The executor only ever calls into the IR.

### Relevant Files

- `crates/openfang-types/src/workflow.rs` — v2 definition types from Task 13, plus `WorkflowIr`
- `crates/openfang-kernel/src/workflow.rs` — `WorkflowEngine` and the compile pipeline implementation
- `crates/openfang-types/src/` — shared contract types from task 5 (`ContractNode` or equivalent)
- `tasks/prd-compozy/docs/DESIGN.md` — section 17 (Workflow v2), section 21 (Workflow v2 Public Schema)
- `tasks/prd-compozy/docs/adrs/040-toml-authoring-json-transport-ir-execution.md`
- `tasks/prd-compozy/docs/adrs/041-bounded-layered-definition-validation.md`

### Dependent Files

- `crates/openfang-types/src/contract.rs` (or equivalent) from task 5 — shared contract types for `input`/`output`
- `crates/openfang-kernel/src/kernel.rs` — `run_workflow` method that must be updated to use `WorkflowIr`

## Deliverables

- Three-phase compile pipeline: `validate`, `normalize`, `compile` producing `WorkflowIr`.
- `WorkflowIr` struct in `crates/openfang-types/src/workflow.rs` with all required fields.
- `NormalizedWorkflow` intermediate representation.
- `ValidationIssue` type with `severity`, `code`, `path`, `message` fields.
- Updated runtime executor accepting `WorkflowIr` as its sole input.
- Tests covering all eight step kinds, all four flow modes, and all compile error paths.

## Tests

### Unit Tests (Required)

- [ ] `step_kind_agent_validates_and_compiles`: a step with `kind: agent`, valid `uses.agent`, and `flow.mode: sequential` must compile without errors and produce an IR step with the correct target agent reference.
- [ ] `step_kind_primitive_validates_known_primitive`: a step with `kind: primitive` and `uses.primitive: "issue.read"` must pass reference validation when the primitive is registered; must fail with `code: "unknown_primitive"` when it is not.
- [ ] `step_kind_workflow_validates_nested_workflow_exists`: a step with `kind: workflow` and `uses.workflow: "nested-id"` must fail compilation with `code: "unknown_workflow"` when no workflow with that ID is registered.
- [ ] `step_kind_wait_signal_rejects_non_sequential_mode`: a `wait_signal` step with `flow.mode: fan_out` must produce a `ValidationIssue` with `code: "invalid_mode_for_kind"` and `severity: error`.
- [ ] `step_kind_collect_rejects_placement_before_fan_out`: a `collect` step that appears before any `fan_out` step must produce `code: "collect_without_fan_out"`.
- [ ] `step_kind_loop_requires_until_and_max_iterations`: a loop step with `flow.max_iterations` missing must produce `code: "missing_required_field"` with `path: "steps[N].flow.max_iterations"`.
- [ ] `save_as_introduces_symbol_to_vars_namespace`: after compiling a step with `save_as: "issue"`, the symbol `vars.issue` must be present in the compiled symbol table.
- [ ] `outputs_with_dangling_reference_fails_compilation`: an `outputs` entry using `{{ vars.result }}` when no step has `save_as: "result"` must produce `code: "dangling_reference"` with `severity: error`.
- [ ] `forward_reference_in_with_fails_compilation`: a step that uses `{{ vars.foo }}` in its `with` block before any step has `save_as: "foo"` must produce `code: "forward_reference"`.
- [ ] `normalize_fills_default_timeout_and_error_mode`: a step without explicit `runtime.timeout_secs` must have it set to `120` (from `defaults`) in the normalized form.
- [ ] `normalize_text_alias_becomes_string_kind`: an `input` contract with `kind: "text"` must normalize to `kind: "string"`.
- [ ] `compile_produces_stable_ir_from_valid_definition`: a fully valid two-step sequential definition must produce a `WorkflowIr` that round-trips through `serde_json::to_string` and `serde_json::from_str` without loss.

### Regression and Anti-Pattern Guards

- [ ] No step kind is silently ignored during compilation — all eight must produce distinct IR variants.
- [ ] The pipeline must not accept a partially valid definition: if any `ValidationIssue` with `severity: error` is present, `compile` must return an error, not a partial IR.
- [ ] The compile pipeline must not perform network calls, provider boots, or template execution — only structural symbol resolution.
- [ ] The runtime executor must not accept the raw `WorkflowV2Definition` struct as its execution input — only `WorkflowIr` is valid after this task.

### Verification Commands

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`

## Success Criteria

- The validate-normalize-compile pipeline produces a stable `WorkflowIr` from any valid definition. The same definition input produces the same IR output on every invocation (deterministic compilation).
- Invalid definitions are rejected with `severity: error` issues before any run is created. No invalid definition can reach the runtime executor.
- All eight step kinds produce correct IR variants through the pipeline.
- All four flow modes are validated for mode-specific required fields, and invalid kind+mode combinations produce actionable `ValidationIssue` objects.
- The runtime executor accepts only `WorkflowIr` as its execution input.
- `cargo test --workspace` passes with zero failures and `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.

---

## Notes

- This task bridges the definition layer (task 5 contracts, Task 13 types) and the runtime layer (Task 16 durable runs). The `WorkflowIr` produced here is the same object that Task 16's `workflow_run` records will reference as their compiled definition snapshot.
- Keep the IR shape stable so that runtime tasks do not need to re-parse definitions. The IR is a compiled artifact: it must not embed anything that changes at runtime (no live handles, no file paths, no Arc references).
