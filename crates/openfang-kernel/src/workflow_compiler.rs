//! Workflow v2 validate/normalize/compile pipeline.
//!
//! The compiler stays pure and synchronous so workflow definitions can be
//! checked structurally without booting providers, touching the network, or
//! executing templates.

use crate::template_renderer::TemplateRenderer;
use std::collections::{BTreeMap, BTreeSet};

use openfang_types::contract::{
    normalize as normalize_contract, validate as validate_contract, ContractKind, ContractNode,
};
use openfang_types::workflow::{
    CompiledTemplate, ErrorMode, FlowMode, NormalizedWorkflow, NormalizedWorkflowStep,
    ResolvedRuntimeSettings, StepKind, StepUses, TemplateNamespace, TemplateReference,
    TemplateSegment, ValidationIssue, WorkflowIr, WorkflowIrStep, WorkflowIrStepKind,
    WorkflowV2Definition, WorkflowV2Step,
};
use serde_json::Value;
use thiserror::Error;

const OUTPUT_OBJECT_REQUIRED_MESSAGE: &str =
    "`output` must be an object contract when `outputs` projections are used";

/// Named references available to the workflow compiler.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowCompileRegistry {
    agents: BTreeSet<String>,
    workflows: BTreeSet<String>,
    primitives: BTreeSet<String>,
}

impl WorkflowCompileRegistry {
    /// Creates an empty compile registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an agent reference to the registry.
    #[must_use]
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agents.insert(agent.into());
        self
    }

    /// Adds a workflow reference to the registry.
    #[must_use]
    pub fn with_workflow(mut self, workflow: impl Into<String>) -> Self {
        self.workflows.insert(workflow.into());
        self
    }

    /// Adds a primitive reference to the registry.
    #[must_use]
    pub fn with_primitive(mut self, primitive: impl Into<String>) -> Self {
        self.primitives.insert(primitive.into());
        self
    }

    /// Replaces the known agents.
    pub fn set_agents<I, S>(&mut self, agents: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.agents = agents.into_iter().map(Into::into).collect();
    }

    /// Replaces the known workflows.
    pub fn set_workflows<I, S>(&mut self, workflows: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.workflows = workflows.into_iter().map(Into::into).collect();
    }

    /// Replaces the known primitives.
    pub fn set_primitives<I, S>(&mut self, primitives: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.primitives = primitives.into_iter().map(Into::into).collect();
    }

    fn has_agent(&self, agent: &str) -> bool {
        self.agents.contains(agent)
    }

    fn has_workflow(&self, workflow: &str) -> bool {
        self.workflows.contains(workflow)
    }

    fn has_primitive(&self, primitive: &str) -> bool {
        self.primitives.contains(primitive)
    }
}

/// Structured workflow compilation failure with collected validation issues.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("workflow compilation failed with {issue_count} issue(s)")]
pub struct WorkflowCompileError {
    issue_count: usize,
    issues: Vec<ValidationIssue>,
}

impl WorkflowCompileError {
    fn new(mut issues: Vec<ValidationIssue>) -> Self {
        issues.sort_by(|left, right| {
            (
                left.path.as_str(),
                left.code.as_str(),
                left.message.as_str(),
                severity_rank(left),
            )
                .cmp(&(
                    right.path.as_str(),
                    right.code.as_str(),
                    right.message.as_str(),
                    severity_rank(right),
                ))
        });

        Self {
            issue_count: issues.len(),
            issues,
        }
    }

    /// Returns the collected validation issues.
    #[must_use]
    pub fn issues(&self) -> &[ValidationIssue] {
        &self.issues
    }
}

fn severity_rank(issue: &ValidationIssue) -> u8 {
    if issue.severity.is_error() {
        0
    } else {
        1
    }
}

/// Parses a JSON workflow document, validates it, normalizes it, and compiles
/// it into IR.
pub fn compile_workflow_value(
    value: &Value,
    registry: &WorkflowCompileRegistry,
) -> Result<WorkflowIr, WorkflowCompileError> {
    let definition = validate_workflow_value(value, registry)?;
    compile_workflow_definition(&definition, registry)
}

/// Parses a JSON workflow document and returns a typed definition when
/// validation succeeds.
pub fn validate_workflow_value(
    value: &Value,
    registry: &WorkflowCompileRegistry,
) -> Result<WorkflowV2Definition, WorkflowCompileError> {
    let mut issues = Vec::new();
    collect_schema_validation_issues(value, &mut issues);

    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(WorkflowCompileError::new(issues));
    }

    let definition = match serde_json::from_value::<WorkflowV2Definition>(value.clone()) {
        Ok(definition) => definition,
        Err(error) => {
            issues.push(ValidationIssue::error(
                "invalid_definition",
                "",
                format!("workflow definition could not be deserialized: {error}"),
            ));
            return Err(WorkflowCompileError::new(issues));
        }
    };

    issues.extend(validate_workflow_definition(&definition, registry));
    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(WorkflowCompileError::new(issues));
    }

    Ok(definition)
}

/// Runs the validate-normalize-compile pipeline for a typed definition.
pub fn compile_workflow_definition(
    definition: &WorkflowV2Definition,
    registry: &WorkflowCompileRegistry,
) -> Result<WorkflowIr, WorkflowCompileError> {
    let issues = validate_workflow_definition(definition, registry);
    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(WorkflowCompileError::new(issues));
    }

    let normalized = normalize_workflow_definition(definition);
    compile_normalized_workflow(&normalized)
}

/// Validates a typed workflow definition without running normalization or
/// compilation.
#[must_use]
pub fn validate_workflow_definition(
    definition: &WorkflowV2Definition,
    registry: &WorkflowCompileRegistry,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();

    validate_required_string(&definition.id, "id", &mut issues);
    validate_required_string(&definition.name, "name", &mut issues);
    validate_required_string(&definition.version, "version", &mut issues);
    validate_required_string(&definition.description, "description", &mut issues);

    if definition.defaults.timeout_secs == 0 {
        issues.push(ValidationIssue::error(
            "invalid_field_value",
            "defaults.timeout_secs",
            "`defaults.timeout_secs` must be greater than zero",
        ));
    }

    prefix_contract_issues("input", &definition.input, &mut issues);
    prefix_contract_issues("output", &definition.output, &mut issues);

    let mut step_ids = BTreeSet::new();
    let mut save_as_symbols = BTreeSet::new();

    for (index, step) in definition.steps.iter().enumerate() {
        validate_step_definition(
            step,
            index,
            definition,
            registry,
            &mut step_ids,
            &mut save_as_symbols,
            &mut issues,
        );
    }

    validate_duplicate_wait_signal_names(definition, &mut issues);
    validate_output_projection_contract(definition, &mut issues);

    issues
}

/// Produces a normalized workflow with canonical contracts and resolved runtime
/// defaults.
#[must_use]
pub fn normalize_workflow_definition(definition: &WorkflowV2Definition) -> NormalizedWorkflow {
    let defaults = ResolvedRuntimeSettings::from(definition.defaults.clone());
    let input = normalize_contract(definition.input.clone());
    let output = normalize_contract(definition.output.clone());
    let mut steps = Vec::with_capacity(definition.steps.len());

    for step in &definition.steps {
        steps.push(NormalizedWorkflowStep {
            id: step.id.clone(),
            name: step.name.clone(),
            kind: step.kind,
            uses: step.uses.clone(),
            with: step.with.clone(),
            save_as: step.save_as.clone(),
            flow: step.flow.clone(),
            runtime: ResolvedRuntimeSettings::from_workflow_defaults(
                &definition.defaults,
                step.runtime.as_ref(),
            ),
        });
    }

    NormalizedWorkflow {
        id: definition.id.clone(),
        name: definition.name.clone(),
        version: definition.version.clone(),
        description: definition.description.clone(),
        enabled: definition.enabled,
        tags: definition.tags.clone(),
        input,
        output,
        defaults,
        steps,
        outputs: definition.outputs.clone(),
    }
}

/// Validates normalized template bindings and output projections without
/// emitting workflow IR.
#[must_use]
pub fn validate_normalized_workflow(workflow: &NormalizedWorkflow) -> Vec<ValidationIssue> {
    let all_symbols = collect_all_symbols(workflow);
    let mut available_symbols = BTreeMap::new();
    let mut issues = Vec::new();

    for (index, step) in workflow.steps.iter().enumerate() {
        let step_path = format!("steps[{index}]");
        let _ = compile_template_bindings(
            &step.with,
            &step_path,
            &workflow.input,
            &available_symbols,
            &all_symbols,
            &mut issues,
        );

        if let Some(symbol) = step.save_as.as_ref() {
            if let Some(step_id) = all_symbols.get(symbol) {
                available_symbols.insert(symbol.clone(), step_id.clone());
            }
        }
    }

    let _ = compile_output_projection(
        &workflow.outputs,
        &workflow.output,
        &workflow.input,
        &all_symbols,
        &mut issues,
    );

    issues
}

/// Compiles a normalized workflow into stable IR.
pub fn compile_normalized_workflow(
    workflow: &NormalizedWorkflow,
) -> Result<WorkflowIr, WorkflowCompileError> {
    let all_symbols = collect_all_symbols(workflow);
    let mut available_symbols = BTreeMap::new();
    let mut steps = Vec::with_capacity(workflow.steps.len());
    let mut issues = Vec::new();

    for (index, step) in workflow.steps.iter().enumerate() {
        let step_path = format!("steps[{index}]");
        let with = compile_template_bindings(
            &step.with,
            &step_path,
            &workflow.input,
            &available_symbols,
            &all_symbols,
            &mut issues,
        );

        let kind = compile_step_kind(step, &step_path, &mut issues);
        if let Some(kind) = kind {
            steps.push(WorkflowIrStep {
                id: step.id.clone(),
                name: step.name.clone(),
                kind,
                flow: step.flow.clone(),
                runtime: step.runtime.clone(),
                with,
                save_as: step.save_as.clone(),
            });
        }

        if let Some(symbol) = step.save_as.as_ref() {
            if let Some(step_id) = all_symbols.get(symbol) {
                available_symbols.insert(symbol.clone(), step_id.clone());
            }
        }
    }

    let outputs = compile_output_projection(
        &workflow.outputs,
        &workflow.output,
        &workflow.input,
        &all_symbols,
        &mut issues,
    );

    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(WorkflowCompileError::new(issues));
    }

    Ok(WorkflowIr {
        workflow_id: workflow.id.clone(),
        workflow_version: workflow.version.clone(),
        defaults: workflow.defaults.clone(),
        input_contract: workflow.input.clone(),
        output_contract: workflow.output.clone(),
        steps,
        symbol_table: all_symbols,
        outputs,
    })
}

fn collect_schema_validation_issues(value: &Value, issues: &mut Vec<ValidationIssue>) {
    let Some(workflow) = value.as_object() else {
        issues.push(ValidationIssue::error(
            "invalid_type",
            "",
            "workflow definition must be a JSON object",
        ));
        return;
    };

    for field in [
        "id",
        "name",
        "version",
        "description",
        "input",
        "output",
        "steps",
    ] {
        if !workflow.contains_key(field) {
            issues.push(ValidationIssue::error(
                "missing_required_field",
                field,
                format!("`{field}` is required"),
            ));
        }
    }

    if let Some(steps) = workflow.get("steps") {
        let Some(step_list) = steps.as_array() else {
            issues.push(ValidationIssue::error(
                "invalid_type",
                "steps",
                "`steps` must be an array",
            ));
            return;
        };

        for (index, step) in step_list.iter().enumerate() {
            let step_path = format!("steps[{index}]");
            let Some(step) = step.as_object() else {
                issues.push(ValidationIssue::error(
                    "invalid_type",
                    step_path,
                    "workflow step must be an object",
                ));
                continue;
            };

            for field in ["id", "name", "kind"] {
                if !step.contains_key(field) {
                    issues.push(ValidationIssue::error(
                        "missing_required_field",
                        format!("{step_path}.{field}"),
                        format!("`{field}` is required"),
                    ));
                }
            }

            if let Some(kind) = step.get("kind").and_then(Value::as_str) {
                if !matches!(
                    kind,
                    "agent"
                        | "primitive"
                        | "workflow"
                        | "wait_signal"
                        | "start_looper"
                        | "emit_event"
                        | "collect"
                        | "noop"
                ) {
                    issues.push(ValidationIssue::error(
                        "invalid_enum_value",
                        format!("{step_path}.kind"),
                        format!("unknown step kind `{kind}`"),
                    ));
                }
            }

            if let Some(flow) = step.get("flow") {
                validate_flow_schema(flow, &step_path, issues);
            }
        }
    }
}

fn validate_flow_schema(flow: &Value, step_path: &str, issues: &mut Vec<ValidationIssue>) {
    let flow_path = format!("{step_path}.flow");
    let Some(flow) = flow.as_object() else {
        issues.push(ValidationIssue::error(
            "invalid_type",
            flow_path,
            "`flow` must be an object when provided",
        ));
        return;
    };

    let mode = flow
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("sequential");

    match mode {
        "sequential" | "fan_out" => {}
        "conditional" => {
            if !flow.contains_key("when") {
                issues.push(ValidationIssue::error(
                    "missing_required_field",
                    format!("{flow_path}.when"),
                    "`flow.when` is required when `flow.mode` is `conditional`",
                ));
            }
        }
        "loop" => {
            if !flow.contains_key("until") {
                issues.push(ValidationIssue::error(
                    "missing_required_field",
                    format!("{flow_path}.until"),
                    "`flow.until` is required when `flow.mode` is `loop`",
                ));
            }

            if !flow.contains_key("max_iterations") {
                issues.push(ValidationIssue::error(
                    "missing_required_field",
                    format!("{flow_path}.max_iterations"),
                    "`flow.max_iterations` is required when `flow.mode` is `loop`",
                ));
            }
        }
        other => {
            issues.push(ValidationIssue::error(
                "invalid_enum_value",
                format!("{flow_path}.mode"),
                format!("unknown flow mode `{other}`"),
            ));
        }
    }
}

fn validate_required_string(
    value: &str,
    path: impl Into<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let path = path.into();
    if value.trim().is_empty() {
        issues.push(ValidationIssue::error(
            "missing_required_field",
            path,
            "value must not be empty",
        ));
    }
}

fn prefix_contract_issues(
    prefix: &str,
    contract: &ContractNode,
    issues: &mut Vec<ValidationIssue>,
) {
    for issue in validate_contract(contract) {
        let path = if issue.path().is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}.{}", issue.path())
        };
        issues.push(ValidationIssue::error(
            "invalid_contract",
            path,
            issue.message().to_string(),
        ));
    }
}

fn validate_step_definition(
    step: &WorkflowV2Step,
    index: usize,
    workflow: &WorkflowV2Definition,
    registry: &WorkflowCompileRegistry,
    step_ids: &mut BTreeSet<String>,
    save_as_symbols: &mut BTreeSet<String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let step_path = format!("steps[{index}]");
    validate_required_string(&step.id, format!("{step_path}.id"), issues);
    validate_required_string(&step.name, format!("{step_path}.name"), issues);

    if !step_ids.insert(step.id.clone()) {
        issues.push(ValidationIssue::error(
            "duplicate_step_id",
            format!("{step_path}.id"),
            format!("step id `{}` is already defined", step.id),
        ));
    }

    if let Some(symbol) = step.save_as.as_deref() {
        validate_required_string(symbol, format!("{step_path}.save_as"), issues);
        if !symbol.trim().is_empty() && !save_as_symbols.insert(symbol.to_string()) {
            issues.push(ValidationIssue::error(
                "duplicate_symbol",
                format!("{step_path}.save_as"),
                format!("`vars.{symbol}` is already introduced by an earlier step"),
            ));
        }
    }

    if let Some(runtime) = step.runtime.as_ref() {
        if runtime.timeout_secs == Some(0) {
            issues.push(ValidationIssue::error(
                "invalid_field_value",
                format!("{step_path}.runtime.timeout_secs"),
                "`runtime.timeout_secs` must be greater than zero",
            ));
        }

        if matches!(
            runtime.error_mode,
            Some(ErrorMode::Retry { max_retries: 0 })
        ) {
            issues.push(ValidationIssue::warning(
                "retry_without_retries",
                format!("{step_path}.runtime.error_mode"),
                "`retry` with `max_retries = 0` behaves like `fail`",
            ));
        }

        if runtime.dispatch.is_some() && step.kind != StepKind::Agent {
            issues.push(ValidationIssue::error(
                "invalid_dispatch_target",
                format!("{step_path}.runtime.dispatch"),
                "`runtime.dispatch` is only supported on `agent` steps",
            ));
        }
    }

    validate_step_uses(step, &step_path, registry, issues);
    validate_step_flow(step, &step_path, workflow, index, issues);
}

fn validate_duplicate_wait_signal_names(
    workflow: &WorkflowV2Definition,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut seen = BTreeMap::<String, usize>::new();

    for (index, step) in workflow.steps.iter().enumerate() {
        let Some(StepUses::WaitSignal(uses)) = step.uses.as_ref() else {
            continue;
        };
        if step.kind != StepKind::WaitSignal || uses.signal_name.trim().is_empty() {
            continue;
        }

        if let Some(previous_index) = seen.insert(uses.signal_name.clone(), index) {
            issues.push(ValidationIssue::warning(
                "duplicate_wait_signal_name",
                format!("steps[{index}].uses.signal_name"),
                format!(
                    "signal name '{}' is also used by wait_signal step '{}' at steps[{previous_index}]",
                    uses.signal_name, workflow.steps[previous_index].id
                ),
            ));
        }
    }
}

fn validate_step_uses(
    step: &WorkflowV2Step,
    step_path: &str,
    registry: &WorkflowCompileRegistry,
    issues: &mut Vec<ValidationIssue>,
) {
    match step.kind {
        StepKind::Agent => match step.uses.as_ref() {
            Some(StepUses::Agent(uses)) => {
                validate_required_string(&uses.agent, format!("{step_path}.uses.agent"), issues);
                if !uses.agent.trim().is_empty() && !registry.has_agent(&uses.agent) {
                    issues.push(ValidationIssue::error(
                        "unknown_agent",
                        format!("{step_path}.uses.agent"),
                        format!("unknown agent `{}`", uses.agent),
                    ));
                }
            }
            Some(_) => issues.push(ValidationIssue::error(
                "invalid_uses_for_kind",
                format!("{step_path}.uses"),
                "`agent` steps must use `uses.agent`",
            )),
            None => issues.push(ValidationIssue::error(
                "missing_required_field",
                format!("{step_path}.uses.agent"),
                "`agent` steps require `uses.agent`",
            )),
        },
        StepKind::Primitive => match step.uses.as_ref() {
            Some(StepUses::Primitive(uses)) => {
                validate_required_string(
                    &uses.primitive,
                    format!("{step_path}.uses.primitive"),
                    issues,
                );
                if !uses.primitive.trim().is_empty() && !registry.has_primitive(&uses.primitive) {
                    issues.push(ValidationIssue::error(
                        "unknown_primitive",
                        format!("{step_path}.uses.primitive"),
                        format!("unknown primitive `{}`", uses.primitive),
                    ));
                }
            }
            Some(_) => issues.push(ValidationIssue::error(
                "invalid_uses_for_kind",
                format!("{step_path}.uses"),
                "`primitive` steps must use `uses.primitive`",
            )),
            None => issues.push(ValidationIssue::error(
                "missing_required_field",
                format!("{step_path}.uses.primitive"),
                "`primitive` steps require `uses.primitive`",
            )),
        },
        StepKind::Workflow => match step.uses.as_ref() {
            Some(StepUses::Workflow(uses)) => {
                validate_required_string(
                    &uses.workflow,
                    format!("{step_path}.uses.workflow"),
                    issues,
                );
                if !uses.workflow.trim().is_empty() && !registry.has_workflow(&uses.workflow) {
                    issues.push(ValidationIssue::error(
                        "unknown_workflow",
                        format!("{step_path}.uses.workflow"),
                        format!("unknown workflow `{}`", uses.workflow),
                    ));
                }
            }
            Some(_) => issues.push(ValidationIssue::error(
                "invalid_uses_for_kind",
                format!("{step_path}.uses"),
                "`workflow` steps must use `uses.workflow`",
            )),
            None => issues.push(ValidationIssue::error(
                "missing_required_field",
                format!("{step_path}.uses.workflow"),
                "`workflow` steps require `uses.workflow`",
            )),
        },
        StepKind::WaitSignal => match step.uses.as_ref() {
            Some(StepUses::WaitSignal(uses)) => validate_required_string(
                &uses.signal_name,
                format!("{step_path}.uses.signal_name"),
                issues,
            ),
            Some(_) => issues.push(ValidationIssue::error(
                "invalid_uses_for_kind",
                format!("{step_path}.uses"),
                "`wait_signal` steps must use `uses.signal_name`",
            )),
            None => issues.push(ValidationIssue::error(
                "missing_required_field",
                format!("{step_path}.uses.signal_name"),
                "`wait_signal` steps require `uses.signal_name`",
            )),
        },
        StepKind::StartLooper => match step.uses.as_ref() {
            Some(StepUses::StartLooper(uses)) => {
                let has_task_ref = uses
                    .task_ref
                    .as_ref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false);
                let has_task_binding = uses
                    .task_id_binding
                    .as_ref()
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false);

                if !has_task_ref && !has_task_binding {
                    issues.push(ValidationIssue::error(
                        "missing_required_field",
                        format!("{step_path}.uses"),
                        "`start_looper` steps require `uses.task_ref` or `uses.task_id_binding`",
                    ));
                }
            }
            Some(_) => issues.push(ValidationIssue::error(
                "invalid_uses_for_kind",
                format!("{step_path}.uses"),
                "`start_looper` steps must use looper target fields",
            )),
            None => issues.push(ValidationIssue::error(
                "missing_required_field",
                format!("{step_path}.uses"),
                "`start_looper` steps require `uses.task_ref` or `uses.task_id_binding`",
            )),
        },
        StepKind::EmitEvent => match step.uses.as_ref() {
            Some(StepUses::EmitEvent(uses)) => {
                validate_required_string(&uses.event, format!("{step_path}.uses.event"), issues);
            }
            Some(_) => issues.push(ValidationIssue::error(
                "invalid_uses_for_kind",
                format!("{step_path}.uses"),
                "`emit_event` steps must use `uses.event`",
            )),
            None => issues.push(ValidationIssue::error(
                "missing_required_field",
                format!("{step_path}.uses.event"),
                "`emit_event` steps require `uses.event`",
            )),
        },
        StepKind::Collect | StepKind::Noop => {
            if step.uses.is_some() {
                issues.push(ValidationIssue::error(
                    "invalid_uses_for_kind",
                    format!("{step_path}.uses"),
                    format!("`{:?}` steps must not define `uses`", step.kind),
                ));
            }
        }
    }
}

fn validate_step_flow(
    step: &WorkflowV2Step,
    step_path: &str,
    workflow: &WorkflowV2Definition,
    index: usize,
    issues: &mut Vec<ValidationIssue>,
) {
    match &step.flow.mode {
        FlowMode::Sequential | FlowMode::FanOut => {}
        FlowMode::Conditional { when } => {
            validate_required_string(when, format!("{step_path}.flow.when"), issues);
        }
        FlowMode::Loop {
            until,
            max_iterations,
        } => {
            validate_required_string(until, format!("{step_path}.flow.until"), issues);
            if *max_iterations == 0 {
                issues.push(ValidationIssue::error(
                    "invalid_field_value",
                    format!("{step_path}.flow.max_iterations"),
                    "`flow.max_iterations` must be greater than zero",
                ));
            }
        }
    }

    if matches!(step.kind, StepKind::WaitSignal | StepKind::StartLooper)
        && !matches!(step.flow.mode, FlowMode::Sequential)
    {
        issues.push(ValidationIssue::error(
            "invalid_mode_for_kind",
            format!("{step_path}.flow.mode"),
            format!("`{:?}` steps must use `sequential` flow mode", step.kind),
        ));
    }

    if step.kind == StepKind::Workflow
        && matches!(step.flow.mode, FlowMode::FanOut | FlowMode::Loop { .. })
    {
        issues.push(ValidationIssue::error(
            "invalid_mode_for_kind",
            format!("{step_path}.flow.mode"),
            "`workflow` steps only support `sequential` and `conditional` flow modes",
        ));
    }

    if step.kind == StepKind::EmitEvent
        && matches!(
            step.flow.mode,
            FlowMode::Conditional { .. } | FlowMode::Loop { .. }
        )
    {
        issues.push(ValidationIssue::error(
            "invalid_mode_for_kind",
            format!("{step_path}.flow.mode"),
            "`emit_event` steps only support `sequential` and `fan_out` flow modes",
        ));
    }

    if step.kind == StepKind::Collect && !matches!(step.flow.mode, FlowMode::Sequential) {
        issues.push(ValidationIssue::error(
            "invalid_mode_for_kind",
            format!("{step_path}.flow.mode"),
            "`collect` steps must use `sequential` flow mode",
        ));
    }

    if step.kind == StepKind::Collect {
        let has_immediate_fan_out = workflow.steps[..index]
            .last()
            .map(|previous| matches!(previous.flow.mode, FlowMode::FanOut))
            .unwrap_or(false);

        if !has_immediate_fan_out {
            issues.push(ValidationIssue::error(
                "collect_without_fan_out",
                format!("{step_path}.kind"),
                "`collect` steps must appear immediately after one or more `fan_out` steps",
            ));
        }
    }
}

fn validate_output_projection_contract(
    definition: &WorkflowV2Definition,
    issues: &mut Vec<ValidationIssue>,
) {
    if definition.outputs.is_empty() {
        return;
    }

    if definition.output.kind != ContractKind::Object {
        issues.push(ValidationIssue::error(
            "invalid_output_contract",
            "output",
            OUTPUT_OBJECT_REQUIRED_MESSAGE,
        ));
        return;
    }

    let Some(output_fields) = definition.output.fields.as_ref() else {
        issues.push(ValidationIssue::error(
            "invalid_output_contract",
            "output.fields",
            OUTPUT_OBJECT_REQUIRED_MESSAGE,
        ));
        return;
    };

    for field in definition.outputs.keys() {
        if !output_fields.contains_key(field) {
            issues.push(ValidationIssue::error(
                "unknown_output_field",
                format!("outputs.{field}"),
                format!("`{field}` is not defined by the `output` contract"),
            ));
        }
    }

    for field in &definition.output.required {
        if !definition.outputs.contains_key(field) {
            issues.push(ValidationIssue::error(
                "missing_output_binding",
                format!("outputs.{field}"),
                format!("required output field `{field}` has no binding"),
            ));
        }
    }
}

fn collect_all_symbols(workflow: &NormalizedWorkflow) -> BTreeMap<String, String> {
    let mut symbols = BTreeMap::new();
    for step in &workflow.steps {
        if let Some(symbol) = step.save_as.as_ref() {
            symbols.insert(symbol.clone(), step.id.clone());
        }
    }
    symbols
}

fn compile_template_bindings(
    bindings: &BTreeMap<String, String>,
    step_path: &str,
    input_contract: &ContractNode,
    available_symbols: &BTreeMap<String, String>,
    all_symbols: &BTreeMap<String, String>,
    issues: &mut Vec<ValidationIssue>,
) -> BTreeMap<String, CompiledTemplate> {
    let mut compiled = BTreeMap::new();
    for (key, template) in bindings {
        let path = format!("{step_path}.with.{key}");
        let compiled_template = compile_template(
            template,
            &path,
            input_contract,
            available_symbols,
            all_symbols,
            issues,
        );
        compiled.insert(key.clone(), compiled_template);
    }
    compiled
}

fn compile_output_projection(
    outputs: &BTreeMap<String, String>,
    output_contract: &ContractNode,
    input_contract: &ContractNode,
    symbols: &BTreeMap<String, String>,
    issues: &mut Vec<ValidationIssue>,
) -> BTreeMap<String, CompiledTemplate> {
    let mut compiled = BTreeMap::new();

    let output_fields = output_contract.fields.as_ref();

    for (field, template) in outputs {
        let path = format!("outputs.{field}");
        let compiled_template =
            compile_template(template, &path, input_contract, symbols, symbols, issues);

        if let Some(target_contract) = output_fields.and_then(|fields| fields.get(field)) {
            validate_output_binding_contract(
                target_contract,
                &compiled_template,
                input_contract,
                &path,
                issues,
            );
        }

        compiled.insert(field.clone(), compiled_template);
    }

    compiled
}

fn compile_template(
    source: &str,
    path: &str,
    input_contract: &ContractNode,
    available_symbols: &BTreeMap<String, String>,
    all_symbols: &BTreeMap<String, String>,
    issues: &mut Vec<ValidationIssue>,
) -> CompiledTemplate {
    validate_template_source(
        source,
        path,
        input_contract,
        available_symbols,
        all_symbols,
        issues,
    );

    CompiledTemplate {
        source: source.to_string(),
        segments: Vec::new(),
    }
}

fn validate_template_source(
    source: &str,
    path: &str,
    input_contract: &ContractNode,
    available_symbols: &BTreeMap<String, String>,
    all_symbols: &BTreeMap<String, String>,
    issues: &mut Vec<ValidationIssue>,
) {
    let references = match TemplateRenderer::shared().undeclared_variables(source) {
        Ok(references) => {
            let mut references = references.into_iter().collect::<Vec<_>>();
            references.sort();
            references
        }
        Err(error) => {
            issues.push(ValidationIssue::error(
                "invalid_template_syntax",
                path,
                format!("invalid template syntax: {error}"),
            ));
            return;
        }
    };

    for expression in references {
        let Some(reference) = parse_template_reference(&expression) else {
            issues.push(ValidationIssue::error(
                "unsupported_template_expression",
                path,
                format!("unsupported template expression `{expression}`"),
            ));
            continue;
        };

        match reference.namespace {
            TemplateNamespace::Input => {
                if resolve_contract_path(input_contract, &reference.path).is_none() {
                    issues.push(ValidationIssue::error(
                        "unknown_input_reference",
                        path,
                        format!(
                            "template references unknown input path `{}`",
                            display_reference(&reference)
                        ),
                    ));
                }
            }
            TemplateNamespace::Vars => {
                let Some(symbol) = reference.path.first() else {
                    issues.push(ValidationIssue::error(
                        "invalid_template_reference",
                        path,
                        "`vars` references must include a symbol name",
                    ));
                    continue;
                };

                if available_symbols.contains_key(symbol) {
                    continue;
                }

                let code = if all_symbols.contains_key(symbol) {
                    "forward_reference"
                } else {
                    "dangling_reference"
                };
                let message = if code == "forward_reference" {
                    format!("`vars.{symbol}` is referenced before the step that defines it")
                } else {
                    format!("`vars.{symbol}` is not defined by any step `save_as` binding")
                };

                issues.push(ValidationIssue::error(code, path, message));
            }
        }
    }
}

fn tokenize_template(
    source: &str,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Vec<TemplateSegment> {
    let mut segments = Vec::new();
    let mut cursor = 0usize;

    while let Some(relative_start) = source[cursor..].find("{{") {
        let start = cursor + relative_start;
        if start > cursor {
            segments.push(TemplateSegment::Text {
                value: source[cursor..start].to_string(),
            });
        }

        let Some(relative_end) = source[start + 2..].find("}}") else {
            issues.push(ValidationIssue::error(
                "invalid_template_syntax",
                path,
                "template reference is missing closing `}}`",
            ));
            return segments;
        };
        let end = start + 2 + relative_end;
        let expression = source[start + 2..end].trim();
        match parse_template_reference(expression) {
            Some(reference) => segments.push(TemplateSegment::Reference { reference }),
            None => issues.push(ValidationIssue::error(
                "unsupported_template_expression",
                path,
                format!("unsupported template expression `{expression}`"),
            )),
        }
        cursor = end + 2;
    }

    if cursor < source.len() {
        segments.push(TemplateSegment::Text {
            value: source[cursor..].to_string(),
        });
    }

    if segments.is_empty() {
        segments.push(TemplateSegment::Text {
            value: String::new(),
        });
    }

    segments
}

fn parse_template_reference(expression: &str) -> Option<TemplateReference> {
    if expression.is_empty() {
        return None;
    }

    let parts = expression.split('.').map(str::trim).collect::<Vec<_>>();
    if parts.iter().any(|part| !is_valid_template_identifier(part)) {
        return None;
    }

    let namespace = match parts.first().copied()? {
        "input" => TemplateNamespace::Input,
        "vars" => TemplateNamespace::Vars,
        _ => return None,
    };

    Some(TemplateReference {
        namespace,
        path: parts.into_iter().skip(1).map(ToOwned::to_owned).collect(),
    })
}

fn is_valid_template_identifier(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn resolve_contract_path<'a>(
    contract: &'a ContractNode,
    path: &[String],
) -> Option<&'a ContractNode> {
    if path.is_empty() {
        return Some(contract);
    }

    let mut current = contract;
    for segment in path {
        let fields = current.fields.as_ref()?;
        current = fields.get(segment)?;
    }

    Some(current)
}

fn display_reference(reference: &TemplateReference) -> String {
    let namespace = match reference.namespace {
        TemplateNamespace::Input => "input",
        TemplateNamespace::Vars => "vars",
    };

    if reference.path.is_empty() {
        namespace.to_string()
    } else {
        format!("{namespace}.{}", reference.path.join("."))
    }
}

fn validate_output_binding_contract(
    target_contract: &ContractNode,
    template: &CompiledTemplate,
    input_contract: &ContractNode,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    match infer_template_shape(template, input_contract) {
        InferredTemplateShape::KnownInput(contract) => {
            if !contracts_are_compatible(contract, target_contract) {
                issues.push(ValidationIssue::error(
                    "incompatible_output_binding",
                    path,
                    format!(
                        "template source contract `{}` is incompatible with output contract `{}`",
                        contract.kind.as_str(),
                        target_contract.kind.as_str()
                    ),
                ));
            }
        }
        InferredTemplateShape::LiteralString => {
            if !matches!(
                target_contract.kind,
                ContractKind::String | ContractKind::Any
            ) {
                issues.push(ValidationIssue::error(
                    "incompatible_output_binding",
                    path,
                    format!(
                        "literal string output is incompatible with contract kind `{}`",
                        target_contract.kind.as_str()
                    ),
                ));
            }
        }
        InferredTemplateShape::OpaqueVariable => {}
    }
}

enum InferredTemplateShape<'a> {
    KnownInput(&'a ContractNode),
    OpaqueVariable,
    LiteralString,
}

fn infer_template_shape<'a>(
    template: &CompiledTemplate,
    input_contract: &'a ContractNode,
) -> InferredTemplateShape<'a> {
    let Some(reference) = single_template_reference(template) else {
        return InferredTemplateShape::LiteralString;
    };

    match reference.namespace {
        TemplateNamespace::Input => resolve_contract_path(input_contract, &reference.path)
            .map(InferredTemplateShape::KnownInput)
            .unwrap_or(InferredTemplateShape::LiteralString),
        TemplateNamespace::Vars => InferredTemplateShape::OpaqueVariable,
    }
}

fn single_template_reference(template: &CompiledTemplate) -> Option<TemplateReference> {
    if !template.segments.is_empty() {
        if template.segments.len() != 1 {
            return None;
        }

        return match &template.segments[0] {
            TemplateSegment::Text { .. } => None,
            TemplateSegment::Reference { reference } => Some(reference.clone()),
        };
    }

    let mut issues = Vec::new();
    let segments = tokenize_template(&template.source, "", &mut issues);
    if !issues.is_empty() || segments.len() != 1 {
        return None;
    }

    match &segments[0] {
        TemplateSegment::Text { .. } => None,
        TemplateSegment::Reference { reference } => Some(reference.clone()),
    }
}

fn contracts_are_compatible(source: &ContractNode, target: &ContractNode) -> bool {
    if target.kind == ContractKind::Any {
        return true;
    }

    if source.kind != target.kind {
        return false;
    }

    if source.nullable && !target.nullable {
        return false;
    }

    match target.kind {
        ContractKind::ArtifactRef => metadata_matches(
            source.artifact_type.as_deref(),
            target.artifact_type.as_deref(),
        ),
        ContractKind::DocRef => {
            metadata_matches(source.doc_type.as_deref(), target.doc_type.as_deref())
        }
        ContractKind::Object => match (source.fields.as_ref(), target.fields.as_ref()) {
            (_, None) => true,
            (Some(source_fields), Some(target_fields)) => {
                target_fields.iter().all(|(field, target_field)| {
                    source_fields
                        .get(field)
                        .map(|source_field| contracts_are_compatible(source_field, target_field))
                        .unwrap_or(false)
                })
            }
            (None, Some(_)) => false,
        },
        ContractKind::Array => match (source.items.as_deref(), target.items.as_deref()) {
            (_, None) => true,
            (Some(source_items), Some(target_items)) => {
                contracts_are_compatible(source_items, target_items)
            }
            (None, Some(_)) => false,
        },
        _ => true,
    }
}

fn metadata_matches(source: Option<&str>, target: Option<&str>) -> bool {
    match target {
        Some(target) => source == Some(target),
        None => true,
    }
}

fn compile_step_kind(
    step: &NormalizedWorkflowStep,
    step_path: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Option<WorkflowIrStepKind> {
    match (step.kind, step.uses.as_ref()) {
        (StepKind::Agent, Some(StepUses::Agent(uses))) => Some(WorkflowIrStepKind::Agent {
            agent: uses.agent.clone(),
        }),
        (StepKind::Primitive, Some(StepUses::Primitive(uses))) => {
            Some(WorkflowIrStepKind::Primitive {
                primitive: uses.primitive.clone(),
            })
        }
        (StepKind::Workflow, Some(StepUses::Workflow(uses))) => {
            Some(WorkflowIrStepKind::Workflow {
                workflow: uses.workflow.clone(),
            })
        }
        (StepKind::WaitSignal, Some(StepUses::WaitSignal(uses))) => {
            Some(WorkflowIrStepKind::WaitSignal {
                signal_name: uses.signal_name.clone(),
            })
        }
        (StepKind::StartLooper, Some(StepUses::StartLooper(uses))) => {
            Some(WorkflowIrStepKind::StartLooper {
                task_ref: uses.task_ref.clone(),
                task_id_binding: uses.task_id_binding.clone(),
            })
        }
        (StepKind::EmitEvent, Some(StepUses::EmitEvent(uses))) => {
            Some(WorkflowIrStepKind::EmitEvent {
                event: uses.event.clone(),
                payload_template: uses.payload_template.clone(),
            })
        }
        (StepKind::Collect, None) => Some(WorkflowIrStepKind::Collect),
        (StepKind::Noop, None) => Some(WorkflowIrStepKind::Noop),
        _ => {
            issues.push(ValidationIssue::error(
                "invalid_uses_for_kind",
                format!("{step_path}.uses"),
                format!(
                    "`{:?}` step could not be compiled because its `uses` block is invalid",
                    step.kind
                ),
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compile_workflow_definition, compile_workflow_value, normalize_workflow_definition,
        validate_normalized_workflow, validate_workflow_definition, validate_workflow_value,
        WorkflowCompileRegistry,
    };
    use openfang_types::workflow::{
        FlowMode, ValidationSeverity, WorkflowIrStepKind, WorkflowV2Definition,
    };
    use pretty_assertions::assert_eq;
    use serde_json::{json, Value};

    fn registry() -> WorkflowCompileRegistry {
        WorkflowCompileRegistry::new()
            .with_agent("prd-writer")
            .with_agent("analyst")
            .with_workflow("nested-id")
            .with_primitive("issue.read")
    }

    fn parse_definition(value: Value) -> WorkflowV2Definition {
        serde_json::from_value(value).expect("workflow definition should deserialize")
    }

    fn valid_definition_json() -> Value {
        json!({
            "id": "sdlc",
            "name": "SDLC",
            "version": "1.0.0",
            "description": "First-party workflow",
            "enabled": true,
            "tags": ["sdlc"],
            "input": {
                "kind": "object",
                "required": ["issue_id"],
                "open": false,
                "fields": {
                    "issue_id": { "kind": "string" }
                }
            },
            "output": {
                "kind": "object",
                "required": ["result"],
                "open": false,
                "fields": {
                    "result": { "kind": "any" }
                }
            },
            "defaults": {
                "timeout_secs": 120,
                "error_mode": "fail"
            },
            "steps": [
                {
                    "id": "load",
                    "name": "Load",
                    "kind": "primitive",
                    "uses": { "primitive": "issue.read" },
                    "with": {
                        "issue_id": "{{ input.issue_id }}"
                    },
                    "save_as": "issue",
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "write",
                    "name": "Write",
                    "kind": "agent",
                    "uses": { "agent": "prd-writer" },
                    "with": {
                        "issue": "{{ vars.issue }}"
                    },
                    "save_as": "result",
                    "flow": { "mode": "sequential" }
                }
            ],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        })
    }

    fn definition_with_step(step: Value) -> WorkflowV2Definition {
        let mut definition = valid_definition_json();
        definition["steps"] = Value::Array(vec![step]);
        parse_definition(definition)
    }

    fn compile_ok(definition: &WorkflowV2Definition) -> openfang_types::workflow::WorkflowIr {
        compile_workflow_definition(definition, &registry()).expect("workflow should compile")
    }

    #[test]
    fn step_kind_agent_validates_and_compiles() {
        let definition = definition_with_step(json!({
            "id": "agent-step",
            "name": "Agent Step",
            "kind": "agent",
            "uses": { "agent": "prd-writer" },
            "with": { "issue_id": "{{ input.issue_id }}" },
            "save_as": "result",
            "flow": { "mode": "sequential" }
        }));

        let ir = compile_ok(&definition);
        assert_eq!(ir.steps.len(), 1);
        assert_eq!(
            ir.steps[0].kind,
            WorkflowIrStepKind::Agent {
                agent: "prd-writer".to_string()
            }
        );
    }

    #[test]
    fn step_kind_primitive_validates_known_primitive() {
        let definition = definition_with_step(json!({
            "id": "primitive-step",
            "name": "Primitive Step",
            "kind": "primitive",
            "uses": { "primitive": "issue.read" },
            "with": { "issue_id": "{{ input.issue_id }}" },
            "save_as": "result",
            "flow": { "mode": "sequential" }
        }));

        assert!(compile_workflow_definition(&definition, &registry()).is_ok());

        let missing_registry = WorkflowCompileRegistry::new().with_agent("prd-writer");
        let error = compile_workflow_definition(&definition, &missing_registry)
            .expect_err("unknown primitive should fail");
        assert_eq!(error.issues()[0].code, "unknown_primitive");
    }

    #[test]
    fn step_kind_workflow_validates_nested_workflow_exists() {
        let definition = definition_with_step(json!({
            "id": "workflow-step",
            "name": "Workflow Step",
            "kind": "workflow",
            "uses": { "workflow": "nested-id" },
            "flow": { "mode": "sequential" }
        }));

        let missing_registry = WorkflowCompileRegistry::new().with_agent("prd-writer");
        let error = compile_workflow_definition(&definition, &missing_registry)
            .expect_err("unknown nested workflow should fail");
        assert_eq!(error.issues()[0].code, "unknown_workflow");
    }

    #[test]
    fn step_kind_wait_signal_rejects_non_sequential_mode() {
        let definition = definition_with_step(json!({
            "id": "wait",
            "name": "Wait",
            "kind": "wait_signal",
            "uses": { "signal_name": "approval" },
            "flow": { "mode": "fan_out" }
        }));

        let issues = validate_workflow_definition(&definition, &registry());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "invalid_mode_for_kind");
        assert_eq!(issues[0].severity, ValidationSeverity::Error);
    }

    #[test]
    fn wait_signal_requires_explicit_signal_name() {
        let definition = definition_with_step(json!({
            "id": "wait",
            "name": "Wait",
            "kind": "wait_signal",
            "uses": { "signal_name": "" },
            "flow": { "mode": "sequential" }
        }));

        let issues = validate_workflow_definition(&definition, &registry());
        let issue = issues
            .iter()
            .find(|issue| issue.path == "steps[0].uses.signal_name")
            .expect("missing signal name issue should exist");

        assert_eq!(issue.code, "missing_required_field");
        assert_eq!(issue.severity, ValidationSeverity::Error);
    }

    #[test]
    fn duplicate_wait_signal_names_emit_warning() {
        let definition = parse_definition(json!({
            "id": "wait-duplicates",
            "name": "Wait Duplicates",
            "version": "1.0.0",
            "description": "Duplicate wait signal names",
            "input": { "kind": "any" },
            "output": {
                "kind": "object",
                "required": ["result"],
                "open": false,
                "fields": {
                    "result": { "kind": "any" }
                }
            },
            "steps": [
                {
                    "id": "wait-one",
                    "name": "Wait One",
                    "kind": "wait_signal",
                    "uses": { "signal_name": "approval" },
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "wait-two",
                    "name": "Wait Two",
                    "kind": "wait_signal",
                    "uses": { "signal_name": "approval" },
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "after",
                    "name": "After",
                    "kind": "noop",
                    "save_as": "result",
                    "flow": { "mode": "sequential" }
                }
            ],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        }));

        let issues = validate_workflow_definition(&definition, &registry());
        let issue = issues
            .iter()
            .find(|issue| issue.code == "duplicate_wait_signal_name")
            .expect("duplicate wait signal warning should exist");

        assert_eq!(issue.severity, ValidationSeverity::Warning);
        assert_eq!(issue.path, "steps[1].uses.signal_name");
    }

    #[test]
    fn step_kind_collect_rejects_placement_before_fan_out() {
        let definition = definition_with_step(json!({
            "id": "collect",
            "name": "Collect",
            "kind": "collect",
            "flow": { "mode": "sequential" }
        }));

        let issues = validate_workflow_definition(&definition, &registry());
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "collect_without_fan_out");
    }

    #[test]
    fn step_kind_loop_requires_until_and_max_iterations() {
        let workflow = json!({
            "id": "loop",
            "name": "Loop",
            "version": "1.0.0",
            "description": "Loop workflow",
            "input": { "kind": "object", "fields": {}, "required": [], "open": false },
            "output": { "kind": "object", "fields": {}, "required": [], "open": false },
            "steps": [
                {
                    "id": "loop-step",
                    "name": "Loop Step",
                    "kind": "agent",
                    "uses": { "agent": "prd-writer" },
                    "flow": { "mode": "loop", "until": "{{ vars.done }}" }
                }
            ]
        });

        let error =
            validate_workflow_value(&workflow, &registry()).expect_err("missing field should fail");
        let issue = error
            .issues()
            .iter()
            .find(|issue| issue.code == "missing_required_field")
            .expect("missing_required_field issue should be present");
        assert_eq!(issue.path, "steps[0].flow.max_iterations");
    }

    #[test]
    fn save_as_introduces_symbol_to_vars_namespace() {
        let definition = parse_definition(valid_definition_json());
        let ir = compile_ok(&definition);

        assert_eq!(ir.symbol_table.get("issue"), Some(&"load".to_string()));
    }

    #[test]
    fn outputs_with_dangling_reference_fails_compilation() {
        let mut definition = valid_definition_json();
        definition["outputs"] = json!({
            "result": "{{ vars.missing }}"
        });

        let error = compile_workflow_value(&definition, &registry())
            .expect_err("dangling output reference should fail");
        let issue = error
            .issues()
            .iter()
            .find(|issue| issue.code == "dangling_reference")
            .expect("dangling reference issue should exist");
        assert_eq!(issue.severity, ValidationSeverity::Error);
        assert_eq!(issue.path, "outputs.result");
    }

    #[test]
    fn normalized_validation_reports_dangling_output_reference() {
        let mut definition = valid_definition_json();
        definition["outputs"] = json!({
            "result": "{{ vars.missing }}"
        });

        let definition = parse_definition(definition);
        let normalized = normalize_workflow_definition(&definition);
        let issues = validate_normalized_workflow(&normalized);

        let issue = issues
            .iter()
            .find(|issue| issue.code == "dangling_reference")
            .expect("dangling reference issue should exist");
        assert_eq!(issue.severity, ValidationSeverity::Error);
        assert_eq!(issue.path, "outputs.result");
    }

    #[test]
    fn forward_reference_in_with_fails_compilation() {
        let mut definition = valid_definition_json();
        definition["steps"] = json!([
            {
                "id": "writer",
                "name": "Writer",
                "kind": "agent",
                "uses": { "agent": "prd-writer" },
                "with": {
                    "issue": "{{ vars.issue }}"
                },
                "flow": { "mode": "sequential" }
            },
            {
                "id": "loader",
                "name": "Loader",
                "kind": "primitive",
                "uses": { "primitive": "issue.read" },
                "with": {
                    "issue_id": "{{ input.issue_id }}"
                },
                "save_as": "issue",
                "flow": { "mode": "sequential" }
            }
        ]);

        let error = compile_workflow_value(&definition, &registry())
            .expect_err("forward reference should fail");
        let issue = error
            .issues()
            .iter()
            .find(|issue| issue.code == "forward_reference")
            .expect("forward reference issue should exist");
        assert_eq!(issue.path, "steps[0].with.issue");
    }

    #[test]
    fn normalize_fills_default_timeout_and_error_mode() {
        let definition = parse_definition(valid_definition_json());
        let normalized = normalize_workflow_definition(&definition);

        assert_eq!(normalized.steps[0].runtime.timeout_secs, 120);
        assert_eq!(
            normalized.steps[0].runtime.error_mode,
            definition.defaults.error_mode
        );
    }

    #[test]
    fn normalize_text_alias_becomes_string_kind() {
        let mut definition = valid_definition_json();
        definition["input"] = json!({
            "kind": "text"
        });

        let definition = parse_definition(definition);
        let normalized = normalize_workflow_definition(&definition);
        assert_eq!(
            normalized.input.kind,
            openfang_types::contract::ContractKind::String
        );
    }

    #[test]
    fn compile_produces_stable_ir_from_valid_definition() {
        let definition = parse_definition(valid_definition_json());
        let ir = compile_ok(&definition);

        let json = serde_json::to_string(&ir).expect("ir should serialize");
        let round_trip = serde_json::from_str(&json).expect("ir should deserialize");
        assert_eq!(ir, round_trip);
    }

    #[test]
    fn compile_produces_source_only_templates_for_new_definitions() {
        let definition = parse_definition(valid_definition_json());
        let ir = compile_ok(&definition);

        assert!(ir
            .steps
            .iter()
            .flat_map(|step| step.with.values())
            .all(|template| template.segments.is_empty()));
        assert!(ir
            .outputs
            .values()
            .all(|template| template.segments.is_empty()));
        assert_eq!(ir.steps[0].with["issue_id"].source, "{{ input.issue_id }}");
        assert_eq!(ir.outputs["result"].source, "{{ vars.result }}");
    }

    #[test]
    fn compile_accepts_advanced_minijinja_template_syntax() {
        let mut definition = valid_definition_json();
        definition["steps"][1]["with"]["issue"] = json!(
            r#"{% if vars.issue %}{{ vars.issue | upper }}{% else %}{{ input.issue_id | default("missing") }}{% endif %}"#
        );

        let ir = compile_workflow_value(&definition, &registry())
            .expect("advanced MiniJinja syntax should compile");

        assert!(ir.steps[1].with["issue"].segments.is_empty());
    }

    #[test]
    fn compile_accepts_iteration_template_syntax() {
        let mut definition = valid_definition_json();
        definition["steps"][1]["with"]["issue"] =
            json!(r#"{% for item in vars.issue %}{{ item }},{% endfor %}"#);

        let ir = compile_workflow_value(&definition, &registry())
            .expect("iteration syntax should compile");

        assert!(ir.steps[1].with["issue"].segments.is_empty());
    }

    #[test]
    fn all_step_kinds_produce_distinct_ir_variants() {
        let definition = parse_definition(json!({
            "id": "all-kinds",
            "name": "All Kinds",
            "version": "1.0.0",
            "description": "Covers all step kinds",
            "input": {
                "kind": "object",
                "required": ["issue_id"],
                "open": false,
                "fields": {
                    "issue_id": { "kind": "string" }
                }
            },
            "output": {
                "kind": "object",
                "required": ["result"],
                "open": false,
                "fields": {
                    "result": { "kind": "any" }
                }
            },
            "steps": [
                {
                    "id": "agent",
                    "name": "Agent",
                    "kind": "agent",
                    "uses": { "agent": "prd-writer" },
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "primitive",
                    "name": "Primitive",
                    "kind": "primitive",
                    "uses": { "primitive": "issue.read" },
                    "flow": { "mode": "fan_out" }
                },
                {
                    "id": "workflow",
                    "name": "Workflow",
                    "kind": "workflow",
                    "uses": { "workflow": "nested-id" },
                    "flow": { "mode": "conditional", "when": "ok" }
                },
                {
                    "id": "wait",
                    "name": "Wait",
                    "kind": "wait_signal",
                    "uses": { "signal_name": "approval" },
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "looper",
                    "name": "Looper",
                    "kind": "start_looper",
                    "uses": { "task_ref": "task_123" },
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "emit",
                    "name": "Emit",
                    "kind": "emit_event",
                    "uses": { "event": "task.updated" },
                    "flow": { "mode": "fan_out" }
                },
                {
                    "id": "collect",
                    "name": "Collect",
                    "kind": "collect",
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "noop",
                    "name": "Noop",
                    "kind": "noop",
                    "flow": { "mode": "loop", "until": "done", "max_iterations": 1 },
                    "save_as": "result"
                }
            ],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        }));

        let ir = compile_ok(&definition);
        let variants = ir
            .steps
            .iter()
            .map(|step| match step.kind {
                WorkflowIrStepKind::Agent { .. } => "agent",
                WorkflowIrStepKind::Primitive { .. } => "primitive",
                WorkflowIrStepKind::Workflow { .. } => "workflow",
                WorkflowIrStepKind::WaitSignal { .. } => "wait_signal",
                WorkflowIrStepKind::StartLooper { .. } => "start_looper",
                WorkflowIrStepKind::EmitEvent { .. } => "emit_event",
                WorkflowIrStepKind::Collect => "collect",
                WorkflowIrStepKind::Noop => "noop",
            })
            .collect::<Vec<_>>();

        assert_eq!(
            variants,
            vec![
                "agent",
                "primitive",
                "workflow",
                "wait_signal",
                "start_looper",
                "emit_event",
                "collect",
                "noop",
            ]
        );
    }

    #[test]
    fn compile_returns_error_instead_of_partial_ir_when_validation_fails() {
        let mut definition = valid_definition_json();
        definition["steps"][0]["uses"] = json!({ "primitive": "missing.primitive" });

        let error = compile_workflow_value(&definition, &registry())
            .expect_err("invalid definition should not compile");
        assert!(error
            .issues()
            .iter()
            .any(|issue| issue.code == "unknown_primitive"));
    }

    #[test]
    fn normalize_keeps_flow_modes_intact() {
        let definition = parse_definition(valid_definition_json());
        let normalized = normalize_workflow_definition(&definition);
        assert_eq!(normalized.steps[0].flow.mode, FlowMode::Sequential);
    }
}
