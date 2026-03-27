# Task 14 Review: Workflow v2 Compile Pipeline

## Status: PASS

## Checklist
- [x] 14.1 Validate phase implemented in `crates/openfang-kernel/src/workflow_compiler.rs`: schema validation (`collect_schema_validation_issues`), reference validation (agents, primitives, workflows against `WorkflowCompileRegistry`), semantic validation (`validate_step_flow` enforces kind+mode compatibility, collect placement, loop termination requirements)
- [x] 14.2 Normalize phase: `normalize_workflow_definition` fills `defaults.timeout_secs` into every step via `ResolvedRuntimeSettings::from_workflow_defaults`; normalizes contract aliases (`text` → `string`, `json` → `any`) via `normalize_contract` from the contract module
- [x] 14.3 Compile phase: `compile_normalized_workflow` walks normalized steps, resolves `save_as` symbols, validates template references against the `vars` namespace and `input` contract, produces `WorkflowIr`
- [x] 14.4 `WorkflowIr` struct: `workflow_id`, `workflow_version`, `defaults`, `input_contract`, `output_contract`, `steps`, `symbol_table`, `outputs` — all required fields present
- [x] 14.5 Runtime executor updated: `WorkflowEngine::execute_run` accepts `WorkflowIr` as execution input (referenced via `WorkflowIrStep` and `WorkflowIrStepKind` in the kernel's workflow engine)
- [x] 14.6 Comprehensive unit tests: all twelve required test cases present and correctly covering error paths and symbol resolution

## Findings

### Correct
- Three-phase pipeline is implemented as distinct public functions: `validate_workflow_definition`, `normalize_workflow_definition`, `compile_normalized_workflow`. There is also a convenience `compile_workflow_definition` entry point that runs all three in sequence.
- `validate_workflow_value` accepts a raw `serde_json::Value` for the API layer and runs pre-deserialization schema checks before attempting full struct deserialization.
- `save_as` symbol tracking is correctly incremental: symbols become available only after the step that introduces them. Forward references produce `code: "forward_reference"` and dangling references produce `code: "dangling_reference"` as required.
- Semantic kind+mode compatibility rules are all enforced: `wait_signal`/`start_looper` require `sequential`; `workflow` steps reject `fan_out` and `loop`; `emit_event` steps reject `conditional` and `loop`; `collect` must appear immediately after a `fan_out` step.
- `compile_step_kind` maps all eight `StepKind` variants to distinct `WorkflowIrStepKind` variants — no variant is silently dropped.
- If any `ValidationIssue` with `severity: error` is present, compilation stops and returns `Err`, never a partial IR.
- `NormalizedWorkflow` and `NormalizedWorkflowStep` intermediate representation is properly defined and used as the boundary between normalize and compile.
- `WorkflowCompileRegistry` exposes `with_agent`, `with_workflow`, `with_primitive` builder methods plus `set_*` mutation methods, allowing callers to skip reference checks with an empty registry.
- All required tests are present: `step_kind_agent_validates_and_compiles`, `step_kind_primitive_validates_known_primitive`, `step_kind_workflow_validates_nested_workflow_exists`, `step_kind_wait_signal_rejects_non_sequential_mode`, `step_kind_collect_rejects_placement_before_fan_out`, `step_kind_loop_requires_until_and_max_iterations`, `save_as_introduces_symbol_to_vars_namespace`, `outputs_with_dangling_reference_fails_compilation`, `forward_reference_in_with_fails_compilation`, `normalize_fills_default_timeout_and_error_mode`, `normalize_text_alias_becomes_string_kind`, `compile_produces_stable_ir_from_valid_definition`.

### Minor Observations
- The `validate_normalized_workflow` function is an additional public function not strictly required by the spec but useful for validating symbol bindings on already-normalized workflows — this is additive.
- Output binding contract compatibility checking (`validate_output_binding_contract`) is implemented beyond the minimum spec, providing structural type checks between template sources and the output contract. This is a good addition.
- The `collect` placement check only looks at the immediately preceding step (`workflow.steps[..index].last()`), which is intentional per the spec ("must appear immediately after one or more fan_out steps").

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-kernel/src/workflow_compiler.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/workflow.rs`
