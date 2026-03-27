# Task 13 Review: Workflow v2 Definition Types

## Status: PASS

## Checklist
- [x] 13.1 `WorkflowV2Definition` defined in `crates/openfang-types/src/workflow.rs` with all top-level fields: `id`, `name`, `version`, `description`, `enabled`, `tags`, `input`, `output`, `defaults`, `steps`, `outputs`
- [x] 13.2 `StepKind` enum with all eight variants: `Agent`, `Primitive`, `Workflow`, `WaitSignal`, `StartLooper`, `EmitEvent`, `Collect`, `Noop`; associated `uses` payload types (`AgentUses`, `PrimitiveUses`, `WorkflowUses`, `WaitSignalUses`, `StartLooperUses`, `EmitEventUses`) all present
- [x] 13.3 `FlowMode` enum with four variants: `Sequential`, `FanOut`, `Conditional { when }`, `Loop { until, max_iterations }`; `FlowBlock` wrapper present; deserialization enforces required mode-specific fields via `TryFrom<RawFlowBlock>`
- [x] 13.4 `WorkflowDefaults` with `timeout_secs` and `error_mode`; `RuntimeBlock` with optional per-step overrides; both present and correct
- [x] 13.5 Unit tests: all seven required test cases present and correctly named (`workflow_v2_definition_round_trips_through_serde`, `step_kind_agent_serializes_correctly`, `step_kind_all_variants_deserialize`, `flow_mode_sequential_is_default`, `flow_mode_conditional_requires_when`, `flow_mode_loop_requires_until_and_max_iterations`, `workflow_defaults_apply_sensible_values`)

## Findings

### Correct
- All eight step kinds are defined. `StepKind` and `StepUses` are separate enums, allowing the kind to be a `Copy` enum and the uses payload to carry data.
- `FlowBlock` uses a custom `TryFrom<RawFlowBlock>` conversion that enforces required fields at deserialization time, producing clean user-facing errors ("missing field `when`", "missing field `until`").
- `WorkflowDefaults::default()` produces `timeout_secs: 120` and `error_mode: Fail` as required.
- All types derive `Serialize + Deserialize`. The full round-trip test populates all eight step kinds plus all four flow modes in a single payload.
- `input` and `output` use `ContractNode` from `crates/openfang-types/src/contract.rs` (task 5 dependency met).
- Additional IR types (`NormalizedWorkflow`, `NormalizedWorkflowStep`, `WorkflowIr`, `WorkflowIrStep`, `WorkflowIrStepKind`, `CompiledTemplate`, `TemplateSegment`, `TemplateReference`) are co-located in the same file, which is acceptable since they are all part of the shared type layer consumed by the compile pipeline.
- `ValidationIssue` and `ValidationSeverity` types are defined here with `severity`, `code`, `path`, `message` fields as required.

### Minor Observations
- `StepUses` uses `#[serde(untagged)]` for JSON dispatch. This relies on structural field differences between the variants (`agent`, `primitive`, `workflow`, `signal_name`, `task_ref`/`task_id_binding`, `event`). The test suite covers all eight step kinds through deserialization and they pass, so this works in practice.
- The `Collect` and `Noop` step kinds have no `uses` struct (correctly modeled as `None`), which is consistent with the spec.
- `DispatchMode` and `ResolvedRuntimeSettings` extras beyond the spec requirements are present but do not conflict.

## Files Reviewed
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/workflow.rs`
- `/Users/pedronauck/Dev/compozy/openfang/crates/openfang-types/src/contract.rs` (referenced)
