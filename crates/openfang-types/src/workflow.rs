//! Shared Workflow v2 definition types.
//!
//! These types define the public workflow definition schema shared by the
//! kernel, API, and future compile pipeline. They intentionally model the
//! Compozy-owned workflow surface rather than the legacy in-kernel workflow
//! structs.

use std::collections::BTreeMap;

use crate::contract::ContractNode;
use serde::{Deserialize, Serialize};

const DEFAULT_TIMEOUT_SECS: u64 = 120;

fn default_enabled() -> bool {
    true
}

/// Shared template binding map used by step `with` blocks and workflow
/// `outputs` projections.
pub type TemplateBindings = BTreeMap<String, String>;

/// Public Workflow v2 definition shared across the control plane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowV2Definition {
    /// Stable workflow definition identifier.
    pub id: String,
    /// Human-readable workflow name.
    pub name: String,
    /// Semantic workflow version.
    pub version: String,
    /// Human-readable description of the workflow.
    pub description: String,
    /// Whether the workflow is enabled for normal use.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Secondary workflow classification tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Accepted input contract for the workflow.
    pub input: ContractNode,
    /// Final output contract exposed by the workflow.
    pub output: ContractNode,
    /// Workflow-level runtime defaults.
    #[serde(default)]
    pub defaults: WorkflowDefaults,
    /// Ordered workflow steps.
    pub steps: Vec<WorkflowV2Step>,
    /// Projection from runtime symbols into the final output contract.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: TemplateBindings,
}

/// A single Workflow v2 step definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowV2Step {
    /// Stable step identifier within the workflow.
    pub id: String,
    /// Human-readable step name.
    pub name: String,
    /// Explicit step action kind.
    pub kind: StepKind,
    /// Target or action configuration for the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uses: Option<StepUses>,
    /// Step input bindings evaluated against the current namespaces.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub with: TemplateBindings,
    /// Optional symbol name bound into the `vars` namespace after the step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_as: Option<String>,
    /// Flow control metadata for the step.
    #[serde(default)]
    pub flow: FlowBlock,
    /// Optional runtime overrides for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeBlock>,
}

/// Supported workflow step kinds in the public workflow schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Dispatch to an agent definition.
    Agent,
    /// Invoke a domain primitive.
    Primitive,
    /// Invoke another workflow definition.
    Workflow,
    /// Suspend until a named signal arrives.
    WaitSignal,
    /// Start a looper run.
    StartLooper,
    /// Emit a named event into the system event pipeline.
    EmitEvent,
    /// Collect results from a preceding fan-out group.
    Collect,
    /// No-op placeholder step.
    Noop,
}

/// Step `uses` payloads keyed by the step kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepUses {
    /// Agent dispatch target.
    Agent(AgentUses),
    /// Primitive invocation target.
    Primitive(PrimitiveUses),
    /// Nested workflow target.
    Workflow(WorkflowUses),
    /// Workflow signal to wait on.
    WaitSignal(WaitSignalUses),
    /// Looper start configuration.
    StartLooper(StartLooperUses),
    /// Event emission configuration.
    EmitEvent(EmitEventUses),
}

/// Target payload for an `agent` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentUses {
    /// Target agent definition identifier.
    pub agent: String,
}

/// Target payload for a `primitive` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrimitiveUses {
    /// Primitive identifier, such as `issue.read`.
    pub primitive: String,
}

/// Target payload for a `workflow` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowUses {
    /// Referenced workflow definition identifier.
    pub workflow: String,
}

/// Target payload for a `wait_signal` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitSignalUses {
    /// Signal name that resumes the waiting workflow run.
    pub signal_name: String,
}

/// Target payload for a `start_looper` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartLooperUses {
    /// Static task reference for the looper run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_ref: Option<String>,
    /// Binding that resolves to the task identifier at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id_binding: Option<String>,
}

/// Target payload for an `emit_event` step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmitEventUses {
    /// Event name emitted into the system event pipeline.
    pub event: String,
    /// Optional payload template resolved at runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_template: Option<String>,
}

/// Flow control wrapper for a workflow step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawFlowBlock", into = "RawFlowBlock")]
pub struct FlowBlock {
    /// Flow mode and any mode-specific required fields.
    pub mode: FlowMode,
}

/// Supported step flow modes in the public workflow schema.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum FlowMode {
    /// Execute the step in order after the previous step completes.
    #[default]
    Sequential,
    /// Execute the step as part of a fan-out group.
    FanOut,
    /// Execute the step only when the condition is true.
    Conditional {
        /// Template expression evaluated against the current namespaces.
        when: String,
    },
    /// Repeat the step until the exit condition is met.
    Loop {
        /// Exit condition expression evaluated after each iteration.
        until: String,
        /// Maximum number of iterations allowed.
        max_iterations: u32,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct RawFlowBlock {
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<FlowModeTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    when: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    until: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_iterations: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FlowModeTag {
    Sequential,
    FanOut,
    Conditional,
    Loop,
}

impl TryFrom<RawFlowBlock> for FlowBlock {
    type Error = String;

    fn try_from(raw: RawFlowBlock) -> Result<Self, Self::Error> {
        let mode = match raw.mode.unwrap_or(FlowModeTag::Sequential) {
            FlowModeTag::Sequential => FlowMode::Sequential,
            FlowModeTag::FanOut => FlowMode::FanOut,
            FlowModeTag::Conditional => FlowMode::Conditional {
                when: raw.when.ok_or_else(|| "missing field `when`".to_string())?,
            },
            FlowModeTag::Loop => FlowMode::Loop {
                until: raw
                    .until
                    .ok_or_else(|| "missing field `until`".to_string())?,
                max_iterations: raw
                    .max_iterations
                    .ok_or_else(|| "missing field `max_iterations`".to_string())?,
            },
        };

        Ok(Self { mode })
    }
}

impl From<FlowBlock> for RawFlowBlock {
    fn from(flow: FlowBlock) -> Self {
        match flow.mode {
            FlowMode::Sequential => Self {
                mode: Some(FlowModeTag::Sequential),
                ..Self::default()
            },
            FlowMode::FanOut => Self {
                mode: Some(FlowModeTag::FanOut),
                ..Self::default()
            },
            FlowMode::Conditional { when } => Self {
                mode: Some(FlowModeTag::Conditional),
                when: Some(when),
                ..Self::default()
            },
            FlowMode::Loop {
                until,
                max_iterations,
            } => Self {
                mode: Some(FlowModeTag::Loop),
                until: Some(until),
                max_iterations: Some(max_iterations),
                ..Self::default()
            },
        }
    }
}

/// Shared workflow-level default runtime settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowDefaults {
    /// Default timeout applied to steps without an override.
    pub timeout_secs: u64,
    /// Default error handling behavior for steps without an override.
    pub error_mode: ErrorMode,
}

impl Default for WorkflowDefaults {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            error_mode: ErrorMode::default(),
        }
    }
}

/// Per-step runtime override block.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeBlock {
    /// Optional timeout override for the step.
    pub timeout_secs: Option<u64>,
    /// Optional error handling override for the step.
    pub error_mode: Option<ErrorMode>,
    /// Optional dispatch mode override for agent steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatch: Option<DispatchMode>,
}

/// Error handling behavior for a workflow step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorMode {
    /// Abort the workflow when the step fails.
    #[default]
    Fail,
    /// Skip the failed step and continue.
    Skip,
    /// Retry the step a fixed number of times before failing.
    Retry {
        /// Maximum number of retries before the step fails permanently.
        max_retries: u32,
    },
}

/// Durable dispatch mode for one workflow agent step.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchMode {
    /// Invoke the agent and wait for the result before advancing.
    #[default]
    Call,
    /// Start the agent asynchronously and advance immediately.
    Send,
    /// Resolve or create a long-lived runtime agent and return its identity.
    Spawn,
}

/// Severity for machine-readable workflow validation issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    /// The definition is invalid and compilation must stop.
    Error,
    /// The definition is accepted, but the author should review the issue.
    Warning,
}

impl ValidationSeverity {
    /// Returns `true` when the severity blocks compilation.
    #[must_use]
    pub const fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// Machine-readable validation issue returned by the workflow compile pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationIssue {
    /// Severity of the issue.
    pub severity: ValidationSeverity,
    /// Stable machine-readable issue code.
    pub code: String,
    /// Path to the invalid field or binding.
    pub path: String,
    /// Human-readable message explaining the problem.
    pub message: String,
}

impl ValidationIssue {
    /// Creates a validation issue.
    #[must_use]
    pub fn new(
        severity: ValidationSeverity,
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity,
            code: code.into(),
            path: path.into(),
            message: message.into(),
        }
    }

    /// Creates an error issue.
    #[must_use]
    pub fn error(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(ValidationSeverity::Error, code, path, message)
    }

    /// Creates a warning issue.
    #[must_use]
    pub fn warning(
        code: impl Into<String>,
        path: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::new(ValidationSeverity::Warning, code, path, message)
    }
}

/// Concrete runtime settings after workflow defaults and step overrides have
/// been merged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRuntimeSettings {
    /// Effective timeout for the workflow or step.
    pub timeout_secs: u64,
    /// Effective error handling behavior for the workflow or step.
    pub error_mode: ErrorMode,
    /// Effective dispatch mode for agent steps.
    pub dispatch: DispatchMode,
}

impl Default for ResolvedRuntimeSettings {
    fn default() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            error_mode: ErrorMode::default(),
            dispatch: DispatchMode::default(),
        }
    }
}

impl ResolvedRuntimeSettings {
    /// Resolves workflow defaults and an optional step-level override into a
    /// single concrete runtime configuration.
    #[must_use]
    pub fn from_workflow_defaults(
        defaults: &WorkflowDefaults,
        runtime: Option<&RuntimeBlock>,
    ) -> Self {
        Self {
            timeout_secs: runtime
                .and_then(|value| value.timeout_secs)
                .unwrap_or(defaults.timeout_secs),
            error_mode: runtime
                .and_then(|value| value.error_mode.clone())
                .unwrap_or_else(|| defaults.error_mode.clone()),
            dispatch: runtime.and_then(|value| value.dispatch).unwrap_or_default(),
        }
    }
}

impl From<WorkflowDefaults> for ResolvedRuntimeSettings {
    fn from(value: WorkflowDefaults) -> Self {
        Self {
            timeout_secs: value.timeout_secs,
            error_mode: value.error_mode,
            dispatch: DispatchMode::default(),
        }
    }
}

/// Stable workflow shape produced by the normalization phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedWorkflow {
    /// Stable workflow definition identifier.
    pub id: String,
    /// Human-readable workflow name.
    pub name: String,
    /// Semantic workflow version.
    pub version: String,
    /// Human-readable description of the workflow.
    pub description: String,
    /// Whether the workflow is enabled for normal use.
    pub enabled: bool,
    /// Secondary workflow classification tags.
    pub tags: Vec<String>,
    /// Normalized workflow input contract.
    pub input: ContractNode,
    /// Normalized workflow output contract.
    pub output: ContractNode,
    /// Workflow-level runtime defaults in concrete form.
    pub defaults: ResolvedRuntimeSettings,
    /// Ordered workflow steps with resolved runtime settings.
    pub steps: Vec<NormalizedWorkflowStep>,
    /// Normalized projection from runtime symbols into the final output contract.
    pub outputs: TemplateBindings,
}

/// Normalized workflow step produced by the normalization phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedWorkflowStep {
    /// Stable step identifier within the workflow.
    pub id: String,
    /// Human-readable step name.
    pub name: String,
    /// Explicit step action kind.
    pub kind: StepKind,
    /// Target or action configuration for the step.
    pub uses: Option<StepUses>,
    /// Step input bindings evaluated against the current namespaces.
    pub with: TemplateBindings,
    /// Optional symbol name bound into the `vars` namespace after the step.
    pub save_as: Option<String>,
    /// Flow control metadata for the step.
    pub flow: FlowBlock,
    /// Effective runtime settings for the step.
    pub runtime: ResolvedRuntimeSettings,
}

/// Namespace referenced by a compiled template expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateNamespace {
    /// Reference into the workflow input contract.
    Input,
    /// Reference into the incremental `vars` namespace.
    Vars,
}

/// Single reference inside a compiled template expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateReference {
    /// Namespace that owns the referenced symbol.
    pub namespace: TemplateNamespace,
    /// Path segments after the namespace, such as `["issue_id"]` for
    /// `{{ input.issue_id }}`.
    pub path: Vec<String>,
}

/// Tokenized template segment stored in compiled IR to avoid reparsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TemplateSegment {
    /// Literal text segment between template references.
    Text {
        /// Literal text value.
        value: String,
    },
    /// Symbol reference segment.
    Reference {
        /// Referenced namespace path.
        reference: TemplateReference,
    },
}

/// Template string after structural validation and tokenization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledTemplate {
    /// Original source string from the definition.
    pub source: String,
    /// Tokenized template segments.
    pub segments: Vec<TemplateSegment>,
}

/// IR step kind after the compile phase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkflowIrStepKind {
    /// Dispatch to the named agent definition.
    Agent {
        /// Agent definition identifier or name.
        agent: String,
    },
    /// Invoke a domain primitive.
    Primitive {
        /// Primitive identifier.
        primitive: String,
    },
    /// Invoke a nested workflow definition.
    Workflow {
        /// Nested workflow identifier.
        workflow: String,
    },
    /// Suspend until the named signal arrives.
    WaitSignal {
        /// Signal name that resumes the workflow.
        signal_name: String,
    },
    /// Launch a looper run.
    StartLooper {
        /// Static task reference for the looper run, if present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_ref: Option<String>,
        /// Binding that resolves to the task identifier at runtime, if present.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task_id_binding: Option<String>,
    },
    /// Emit an event into the system pipeline.
    EmitEvent {
        /// Event name to emit.
        event: String,
        /// Optional payload template source retained for the runtime.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        payload_template: Option<String>,
    },
    /// Collect results from a preceding fan-out group.
    Collect,
    /// No-op placeholder step.
    Noop,
}

/// Single compiled step inside `WorkflowIr`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowIrStep {
    /// Stable step identifier within the workflow.
    pub id: String,
    /// Human-readable step name.
    pub name: String,
    /// Compiled step action.
    pub kind: WorkflowIrStepKind,
    /// Flow control metadata for the step.
    pub flow: FlowBlock,
    /// Effective runtime settings for the step.
    pub runtime: ResolvedRuntimeSettings,
    /// Tokenized and validated step input bindings.
    pub with: BTreeMap<String, CompiledTemplate>,
    /// Optional symbol name introduced into the `vars` namespace after this step.
    pub save_as: Option<String>,
}

/// Stable compiled workflow IR used as the runtime execution boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowIr {
    /// Stable workflow definition identifier.
    pub workflow_id: String,
    /// Semantic workflow version.
    pub workflow_version: String,
    /// Workflow-level runtime defaults.
    pub defaults: ResolvedRuntimeSettings,
    /// Validated workflow input contract.
    pub input_contract: ContractNode,
    /// Validated workflow output contract.
    pub output_contract: ContractNode,
    /// Compiled workflow steps with validated bindings.
    pub steps: Vec<WorkflowIrStep>,
    /// Mapping from `save_as` symbol names to the step IDs that introduce them.
    pub symbol_table: BTreeMap<String, String>,
    /// Tokenized output projection mapping.
    pub outputs: BTreeMap<String, CompiledTemplate>,
}

#[cfg(test)]
mod tests {
    use super::{
        AgentUses, EmitEventUses, ErrorMode, FlowBlock, FlowMode, PrimitiveUses, RuntimeBlock,
        StartLooperUses, StepKind, StepUses, TemplateBindings, WaitSignalUses, WorkflowDefaults,
        WorkflowUses, WorkflowV2Definition, WorkflowV2Step,
    };
    use crate::contract::ContractNode;
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};

    fn parse_contract(value: Value) -> ContractNode {
        serde_json::from_value(value).expect("contract should deserialize")
    }

    #[test]
    fn workflow_v2_definition_round_trips_through_serde() {
        let payload = json!({
            "id": "sdlc",
            "name": "SDLC",
            "version": "1.0.0",
            "description": "First-party SDLC workflow",
            "enabled": true,
            "tags": ["sdlc", "planning"],
            "input": {
                "kind": "object",
                "required": ["issue_id"],
                "open": false,
                "fields": {
                    "issue_id": {
                        "kind": "string"
                    }
                }
            },
            "output": {
                "kind": "object",
                "required": ["prd"],
                "open": false,
                "fields": {
                    "prd": {
                        "kind": "artifact_ref",
                        "artifact_type": "prd"
                    }
                }
            },
            "defaults": {
                "timeout_secs": 120,
                "error_mode": "fail"
            },
            "steps": [
                {
                    "id": "load-issue",
                    "name": "Load Issue",
                    "kind": "primitive",
                    "uses": {
                        "primitive": "issue.read"
                    },
                    "with": {
                        "issue_id": "{{ input.issue_id }}"
                    },
                    "save_as": "issue",
                    "flow": {
                        "mode": "sequential"
                    }
                },
                {
                    "id": "write-prd",
                    "name": "Write PRD",
                    "kind": "agent",
                    "uses": {
                        "agent": "prd-writer"
                    },
                    "with": {
                        "issue": "{{ vars.issue }}"
                    },
                    "save_as": "prd",
                    "flow": {
                        "mode": "conditional",
                        "when": "{{ vars.issue }}"
                    },
                    "runtime": {
                        "timeout_secs": 300,
                        "error_mode": "fail"
                    }
                },
                {
                    "id": "call-subflow",
                    "name": "Call Subflow",
                    "kind": "workflow",
                    "uses": {
                        "workflow": "doc-review"
                    },
                    "flow": {
                        "mode": "sequential"
                    }
                },
                {
                    "id": "wait-approval",
                    "name": "Wait Approval",
                    "kind": "wait_signal",
                    "uses": {
                        "signal_name": "approval_received"
                    },
                    "flow": {
                        "mode": "sequential"
                    }
                },
                {
                    "id": "start-looper",
                    "name": "Start Looper",
                    "kind": "start_looper",
                    "uses": {
                        "task_ref": "task_123",
                        "task_id_binding": "{{ vars.task_id }}"
                    },
                    "flow": {
                        "mode": "sequential"
                    }
                },
                {
                    "id": "emit-event",
                    "name": "Emit Event",
                    "kind": "emit_event",
                    "uses": {
                        "event": "prd.created",
                        "payload_template": "{{ vars.prd }}"
                    },
                    "flow": {
                        "mode": "fan_out"
                    }
                },
                {
                    "id": "collect-results",
                    "name": "Collect Results",
                    "kind": "collect",
                    "flow": {
                        "mode": "sequential"
                    }
                },
                {
                    "id": "done",
                    "name": "Done",
                    "kind": "noop",
                    "flow": {
                        "mode": "loop",
                        "until": "{{ vars.prd }}",
                        "max_iterations": 2
                    }
                }
            ],
            "outputs": {
                "prd": "{{ vars.prd }}"
            }
        });

        let definition: WorkflowV2Definition =
            serde_json::from_value(payload.clone()).expect("definition should deserialize");
        let serialized = serde_json::to_value(&definition).expect("definition should serialize");
        let round_trip: WorkflowV2Definition =
            serde_json::from_value(serialized).expect("definition should round-trip");

        assert_eq!(
            serde_json::to_value(&round_trip).expect("round-trip should serialize"),
            payload
        );
        assert_eq!(round_trip, definition);
    }

    #[test]
    fn step_kind_agent_serializes_correctly() {
        let mut with = TemplateBindings::new();
        with.insert("issue".to_string(), "{{ vars.issue }}".to_string());

        let step = WorkflowV2Step {
            id: "write-prd".to_string(),
            name: "Write PRD".to_string(),
            kind: StepKind::Agent,
            uses: Some(StepUses::Agent(AgentUses {
                agent: "prd-writer".to_string(),
            })),
            with,
            save_as: Some("prd".to_string()),
            flow: FlowBlock::default(),
            runtime: Some(RuntimeBlock {
                timeout_secs: Some(300),
                error_mode: Some(ErrorMode::Fail),
                dispatch: None,
            }),
        };

        assert_eq!(
            serde_json::to_value(step).expect("step should serialize"),
            json!({
                "id": "write-prd",
                "name": "Write PRD",
                "kind": "agent",
                "uses": {
                    "agent": "prd-writer"
                },
                "with": {
                    "issue": "{{ vars.issue }}"
                },
                "save_as": "prd",
                "flow": {
                    "mode": "sequential"
                },
                "runtime": {
                    "timeout_secs": 300,
                    "error_mode": "fail"
                }
            })
        );
    }

    #[test]
    fn step_kind_all_variants_deserialize() {
        let cases = vec![
            (
                json!({
                    "id": "agent-step",
                    "name": "Agent Step",
                    "kind": "agent",
                    "uses": { "agent": "prd-writer" },
                    "flow": { "mode": "sequential" }
                }),
                StepKind::Agent,
                Some("agent"),
            ),
            (
                json!({
                    "id": "primitive-step",
                    "name": "Primitive Step",
                    "kind": "primitive",
                    "uses": { "primitive": "issue.read" },
                    "flow": { "mode": "sequential" }
                }),
                StepKind::Primitive,
                Some("primitive"),
            ),
            (
                json!({
                    "id": "workflow-step",
                    "name": "Workflow Step",
                    "kind": "workflow",
                    "uses": { "workflow": "doc-review" },
                    "flow": { "mode": "conditional", "when": "{{ vars.issue }}" }
                }),
                StepKind::Workflow,
                Some("workflow"),
            ),
            (
                json!({
                    "id": "wait-signal-step",
                    "name": "Wait Signal Step",
                    "kind": "wait_signal",
                    "uses": { "signal_name": "approval_received" },
                    "flow": { "mode": "sequential" }
                }),
                StepKind::WaitSignal,
                Some("wait_signal"),
            ),
            (
                json!({
                    "id": "start-looper-step",
                    "name": "Start Looper Step",
                    "kind": "start_looper",
                    "uses": { "task_id_binding": "{{ vars.task_id }}" },
                    "flow": { "mode": "sequential" }
                }),
                StepKind::StartLooper,
                Some("start_looper"),
            ),
            (
                json!({
                    "id": "emit-event-step",
                    "name": "Emit Event Step",
                    "kind": "emit_event",
                    "uses": { "event": "prd.created" },
                    "flow": { "mode": "fan_out" }
                }),
                StepKind::EmitEvent,
                Some("emit_event"),
            ),
            (
                json!({
                    "id": "collect-step",
                    "name": "Collect Step",
                    "kind": "collect",
                    "flow": { "mode": "sequential" }
                }),
                StepKind::Collect,
                None,
            ),
            (
                json!({
                    "id": "noop-step",
                    "name": "Noop Step",
                    "kind": "noop",
                    "flow": { "mode": "loop", "until": "{{ vars.done }}", "max_iterations": 1 }
                }),
                StepKind::Noop,
                None,
            ),
        ];

        for (value, expected_kind, expected_uses) in cases {
            let step: WorkflowV2Step =
                serde_json::from_value(value).expect("step should deserialize");

            assert_eq!(step.kind, expected_kind);

            match (step.uses, expected_uses) {
                (Some(StepUses::Agent(AgentUses { agent })), Some("agent")) => {
                    assert_eq!(agent, "prd-writer");
                }
                (Some(StepUses::Primitive(PrimitiveUses { primitive })), Some("primitive")) => {
                    assert_eq!(primitive, "issue.read");
                }
                (Some(StepUses::Workflow(WorkflowUses { workflow })), Some("workflow")) => {
                    assert_eq!(workflow, "doc-review");
                }
                (
                    Some(StepUses::WaitSignal(WaitSignalUses { signal_name })),
                    Some("wait_signal"),
                ) => {
                    assert_eq!(signal_name, "approval_received");
                }
                (
                    Some(StepUses::StartLooper(StartLooperUses {
                        task_ref,
                        task_id_binding,
                    })),
                    Some("start_looper"),
                ) => {
                    assert_eq!(task_ref, None);
                    assert_eq!(task_id_binding.as_deref(), Some("{{ vars.task_id }}"));
                }
                (
                    Some(StepUses::EmitEvent(EmitEventUses {
                        event,
                        payload_template,
                    })),
                    Some("emit_event"),
                ) => {
                    assert_eq!(event, "prd.created");
                    assert_eq!(payload_template, None);
                }
                (None, None) => {}
                (actual_uses, actual_kind) => {
                    panic!(
                        "unexpected uses payload for kind {:?}: {:?}",
                        actual_kind, actual_uses
                    );
                }
            }
        }
    }

    #[test]
    fn flow_mode_sequential_is_default() {
        let flow: FlowBlock = serde_json::from_value(json!({})).expect("flow should deserialize");

        assert_eq!(flow, FlowBlock::default());
    }

    #[test]
    fn flow_mode_conditional_requires_when() {
        let error = serde_json::from_value::<FlowBlock>(json!({
            "mode": "conditional"
        }))
        .expect_err("conditional flow without `when` should fail");

        assert!(
            error.to_string().contains("when"),
            "expected missing `when` field, got: {error}"
        );
    }

    #[test]
    fn flow_mode_loop_requires_until_and_max_iterations() {
        let missing_until = serde_json::from_value::<FlowBlock>(json!({
            "mode": "loop",
            "max_iterations": 3
        }))
        .expect_err("loop flow without `until` should fail");
        assert!(
            missing_until.to_string().contains("until"),
            "expected missing `until` field, got: {missing_until}"
        );

        let missing_max_iterations = serde_json::from_value::<FlowBlock>(json!({
            "mode": "loop",
            "until": "{{ vars.done }}"
        }))
        .expect_err("loop flow without `max_iterations` should fail");
        assert!(
            missing_max_iterations
                .to_string()
                .contains("max_iterations"),
            "expected missing `max_iterations` field, got: {missing_max_iterations}"
        );
    }

    #[test]
    fn workflow_defaults_apply_sensible_values() {
        assert_eq!(
            WorkflowDefaults::default(),
            WorkflowDefaults {
                timeout_secs: 120,
                error_mode: ErrorMode::Fail,
            }
        );
    }

    #[test]
    fn step_kind_enum_round_trips_through_strings() {
        let kinds = [
            StepKind::Agent,
            StepKind::Primitive,
            StepKind::Workflow,
            StepKind::WaitSignal,
            StepKind::StartLooper,
            StepKind::EmitEvent,
            StepKind::Collect,
            StepKind::Noop,
        ];

        for kind in kinds {
            let json = serde_json::to_string(&kind).expect("kind should serialize");
            let parsed: StepKind = serde_json::from_str(&json).expect("kind should deserialize");
            assert_eq!(parsed, kind);
        }
    }

    #[test]
    fn workflow_step_with_default_flow_deserializes_from_empty_flow_block() {
        let step: WorkflowV2Step = serde_json::from_value(json!({
            "id": "noop-step",
            "name": "Noop Step",
            "kind": "noop",
            "flow": {}
        }))
        .expect("step should deserialize");

        assert_eq!(step.flow.mode, FlowMode::Sequential);
    }

    #[test]
    fn runtime_block_round_trips_with_retry_mode() {
        let runtime = RuntimeBlock {
            timeout_secs: Some(45),
            error_mode: Some(ErrorMode::Retry { max_retries: 2 }),
            dispatch: None,
        };

        let round_trip: RuntimeBlock = serde_json::from_str(
            &serde_json::to_string(&runtime).expect("runtime should serialize"),
        )
        .expect("runtime should deserialize");

        assert_eq!(round_trip, runtime);
    }

    #[test]
    fn workflow_definition_supports_contract_nodes_on_input_and_output() {
        let definition = WorkflowV2Definition {
            id: "simple".to_string(),
            name: "Simple".to_string(),
            version: "1.0.0".to_string(),
            description: "Simple workflow".to_string(),
            enabled: true,
            tags: vec![],
            input: parse_contract(json!({
                "kind": "object",
                "fields": {
                    "issue_id": {
                        "kind": "string"
                    }
                }
            })),
            output: parse_contract(json!({
                "kind": "artifact_ref",
                "artifact_type": "prd"
            })),
            defaults: WorkflowDefaults::default(),
            steps: vec![WorkflowV2Step {
                id: "noop".to_string(),
                name: "Noop".to_string(),
                kind: StepKind::Noop,
                uses: None,
                with: TemplateBindings::new(),
                save_as: None,
                flow: FlowBlock::default(),
                runtime: None,
            }],
            outputs: TemplateBindings::new(),
        };

        let round_trip: WorkflowV2Definition = serde_json::from_str(
            &serde_json::to_string(&definition).expect("definition should serialize"),
        )
        .expect("definition should deserialize");

        assert_eq!(round_trip, definition);
    }
}
