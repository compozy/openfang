//! Trigger v2 validate/normalize/compile pipeline and runtime registry.
//!
//! This module owns the Compozy-facing trigger control-plane model. It is
//! intentionally separate from the legacy `crate::triggers` engine.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use openfang_memory::{TriggerRuntimeRecord, TriggerRuntimeStore};
use openfang_types::error::OpenFangError;
use openfang_types::error::OpenFangResult;
use openfang_types::trigger::{
    NormalizedTrigger, TriggerDispatchAction, TriggerEvent, TriggerIr, TriggerRuntimeStatus,
    TriggerTarget, TriggerV2Definition, TriggerValidationIssue, TriggerWorkflowSignalSelector,
};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{Mutex, RwLock};

/// Compile-time registry of known trigger references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TriggerCompileRegistry {
    agent_refs: BTreeMap<String, String>,
    workflow_refs: BTreeMap<String, String>,
}

impl TriggerCompileRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one agent definition ID and optional name alias.
    pub fn insert_agent(&mut self, id: impl Into<String>, name: Option<impl Into<String>>) {
        let id = id.into();
        self.agent_refs.insert(id.clone(), id.clone());
        if let Some(name) = name {
            let name = name.into();
            if !name.trim().is_empty() {
                self.agent_refs.insert(name, id);
            }
        }
    }

    /// Registers one workflow definition ID and optional name alias.
    pub fn insert_workflow(&mut self, id: impl Into<String>, name: Option<impl Into<String>>) {
        let id = id.into();
        self.workflow_refs.insert(id.clone(), id.clone());
        if let Some(name) = name {
            let name = name.into();
            if !name.trim().is_empty() {
                self.workflow_refs.insert(name, id);
            }
        }
    }

    fn resolve_agent(&self, value: &str) -> Option<String> {
        self.agent_refs.get(value.trim()).cloned()
    }

    fn resolve_workflow(&self, value: &str) -> Option<String> {
        self.workflow_refs.get(value.trim()).cloned()
    }
}

/// Structured trigger compilation failure with collected validation issues.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("trigger compilation failed with {issue_count} issue(s)")]
pub struct TriggerCompileError {
    issue_count: usize,
    issues: Vec<TriggerValidationIssue>,
}

/// Errors surfaced by the trigger runtime registry.
#[derive(Debug, Error)]
pub enum TriggerEngineError {
    /// The definition failed trigger validation or compilation.
    #[error(transparent)]
    Compile(#[from] TriggerCompileError),
    /// The runtime projection could not be synchronized.
    #[error(transparent)]
    Runtime(#[from] OpenFangError),
}

impl TriggerCompileError {
    fn new(mut issues: Vec<TriggerValidationIssue>) -> Self {
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
    pub fn issues(&self) -> &[TriggerValidationIssue] {
        &self.issues
    }
}

fn severity_rank(issue: &TriggerValidationIssue) -> u8 {
    if issue.severity.is_error() {
        0
    } else {
        1
    }
}

/// Explanation returned by trigger dry-run evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerEvaluationExplanation {
    /// Human-readable summary of the match result.
    pub match_summary: String,
    /// Target kind selected by the compiled trigger.
    pub target_kind: String,
    /// Optional reason why dispatch would be blocked.
    pub blocked_by: Option<String>,
}

/// Result of evaluating one trigger against a synthetic event.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerEvaluation {
    /// Whether the event satisfied the trigger match expression.
    pub matched: bool,
    /// Canonical target that would be used for dispatch.
    pub resolved_target: Option<TriggerTarget>,
    /// Whether the runtime would dispatch after runtime guards are applied.
    pub would_dispatch: bool,
    /// Structured explanation for UI/API responses.
    pub explanation: TriggerEvaluationExplanation,
}

#[derive(Debug, Clone, PartialEq)]
struct TriggerMatchCandidate {
    compiled: TriggerIr,
    runtime: TriggerRuntimeStatus,
}

/// One per-trigger evaluation result from the snapshot match engine.
#[derive(Debug, Clone, PartialEq)]
pub struct TriggerMatchOutcome {
    /// Trigger definition identifier.
    pub trigger_id: String,
    /// Whether the trigger matched and passed runtime guards.
    pub matched: bool,
    /// Canonical target selected for the trigger.
    pub resolved_target: Option<TriggerTarget>,
    /// Dispatch action selected from the target kind.
    pub dispatch_action: TriggerDispatchAction,
    /// Whether the runtime should dispatch the target.
    pub would_dispatch: bool,
    /// Structured explanation for logs and dry-run responses.
    pub explanation: TriggerEvaluationExplanation,
}

/// Ordered event match report for a snapshot of active trigger definitions.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriggerMatchReport {
    /// Per-trigger evaluations in definition order.
    pub evaluations: Vec<TriggerMatchOutcome>,
}

impl TriggerMatchReport {
    /// Returns the trigger IDs that matched and should dispatch.
    #[must_use]
    pub fn matched_trigger_ids(&self) -> Vec<String> {
        self.evaluations
            .iter()
            .filter(|evaluation| evaluation.would_dispatch)
            .map(|evaluation| evaluation.trigger_id.clone())
            .collect()
    }

    /// Returns the dispatchable matches in definition order.
    #[must_use]
    pub fn dispatchable_matches(&self) -> Vec<TriggerMatchOutcome> {
        self.evaluations
            .iter()
            .filter(|evaluation| evaluation.would_dispatch)
            .cloned()
            .collect()
    }

    /// Returns `true` when at least one trigger would dispatch.
    #[must_use]
    pub fn would_dispatch(&self) -> bool {
        self.evaluations
            .iter()
            .any(|evaluation| evaluation.would_dispatch)
    }
}

/// Snapshot-based trigger matcher used by event ingress.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriggerMatchEngine {
    candidates: Vec<TriggerMatchCandidate>,
}

impl TriggerMatchEngine {
    /// Creates a match engine from a fixed ordered trigger snapshot.
    #[must_use]
    fn new(candidates: Vec<TriggerMatchCandidate>) -> Self {
        Self { candidates }
    }

    /// Evaluates every trigger in order and returns the full report.
    #[must_use]
    pub fn evaluate_event(&self, event: &TriggerEvent, now: DateTime<Utc>) -> TriggerMatchReport {
        let evaluations = self
            .candidates
            .iter()
            .map(|candidate| {
                let evaluation =
                    evaluate_compiled_trigger(&candidate.compiled, &candidate.runtime, event, now);
                TriggerMatchOutcome {
                    trigger_id: candidate.compiled.trigger_id.clone(),
                    matched: evaluation.matched,
                    resolved_target: evaluation.resolved_target,
                    dispatch_action: candidate.compiled.dispatch_action,
                    would_dispatch: evaluation.would_dispatch,
                    explanation: evaluation.explanation,
                }
            })
            .collect();

        TriggerMatchReport { evaluations }
    }

    /// Evaluates every trigger in order and returns only the dispatchable matches.
    #[must_use]
    pub fn match_event(
        &self,
        event: &TriggerEvent,
        now: DateTime<Utc>,
    ) -> Vec<TriggerMatchOutcome> {
        self.evaluate_event(event, now).dispatchable_matches()
    }
}

/// Durable trigger v2 registry and active matching set.
#[derive(Clone)]
pub struct TriggerV2Engine {
    definitions: Arc<RwLock<BTreeMap<String, TriggerV2Definition>>>,
    normalized: Arc<RwLock<BTreeMap<String, NormalizedTrigger>>>,
    compiled: Arc<RwLock<BTreeMap<String, TriggerIr>>>,
    active_compiled: Arc<RwLock<BTreeMap<String, TriggerIr>>>,
    runtime_store: TriggerRuntimeStore,
    definition_mutation_lock: Arc<Mutex<()>>,
}

type TriggerDefinitionMap = BTreeMap<String, TriggerV2Definition>;
type NormalizedTriggerMap = BTreeMap<String, NormalizedTrigger>;
type CompiledTriggerMap = BTreeMap<String, TriggerIr>;
type CompiledDefinitionSet = (
    TriggerDefinitionMap,
    NormalizedTriggerMap,
    CompiledTriggerMap,
);

impl TriggerV2Engine {
    /// Creates an empty trigger v2 engine backed by the shared runtime store.
    #[must_use]
    pub fn new(runtime_store: TriggerRuntimeStore) -> Self {
        Self {
            definitions: Arc::new(RwLock::new(BTreeMap::new())),
            normalized: Arc::new(RwLock::new(BTreeMap::new())),
            compiled: Arc::new(RwLock::new(BTreeMap::new())),
            active_compiled: Arc::new(RwLock::new(BTreeMap::new())),
            runtime_store,
            definition_mutation_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Replaces the full trigger definition set and compiled cache.
    pub async fn replace_definitions<I>(
        &self,
        definitions: I,
        registry: &TriggerCompileRegistry,
    ) -> Result<(), TriggerEngineError>
    where
        I: IntoIterator<Item = TriggerV2Definition>,
    {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let definitions = definitions.into_iter().collect::<Vec<_>>();
        let (definition_map, normalized_map, compiled_map) =
            compile_definition_set(&definitions, registry)?;

        *self.definitions.write().await = definition_map.clone();
        *self.normalized.write().await = normalized_map;
        *self.compiled.write().await = compiled_map.clone();
        *self.active_compiled.write().await = compiled_map
            .iter()
            .filter(|(_, compiled)| compiled.enabled)
            .map(|(id, compiled)| (id.clone(), compiled.clone()))
            .collect();

        sync_runtime_store_set(&self.runtime_store, definition_map.values())?;
        Ok(())
    }

    /// Upserts one trigger definition and synchronizes the active matching set.
    pub async fn upsert_definition(
        &self,
        definition: TriggerV2Definition,
        registry: &TriggerCompileRegistry,
    ) -> Result<(), TriggerEngineError> {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let normalized = normalize_trigger_definition(&definition, registry)?;
        let compiled = compile_normalized_trigger(&normalized);

        self.definitions
            .write()
            .await
            .insert(definition.id.clone(), definition.clone());
        self.normalized
            .write()
            .await
            .insert(normalized.id.clone(), normalized);
        self.compiled
            .write()
            .await
            .insert(compiled.trigger_id.clone(), compiled.clone());

        let mut active = self.active_compiled.write().await;
        if compiled.enabled {
            active.insert(compiled.trigger_id.clone(), compiled);
        } else {
            active.remove(&definition.id);
        }

        sync_runtime_record(&self.runtime_store, &definition)?;
        Ok(())
    }

    /// Removes one trigger definition and its runtime projection.
    pub async fn remove_definition(&self, trigger_id: &str) -> OpenFangResult<bool> {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let removed = self.definitions.write().await.remove(trigger_id).is_some();
        self.normalized.write().await.remove(trigger_id);
        self.compiled.write().await.remove(trigger_id);
        self.active_compiled.write().await.remove(trigger_id);
        self.runtime_store.remove_trigger_runtime(trigger_id)?;
        Ok(removed)
    }

    /// Returns one persisted trigger definition from the in-memory registry.
    pub async fn get_definition(&self, trigger_id: &str) -> Option<TriggerV2Definition> {
        self.definitions.read().await.get(trigger_id).cloned()
    }

    /// Returns the normalized trigger definition for one trigger.
    pub async fn get_normalized_trigger(&self, trigger_id: &str) -> Option<NormalizedTrigger> {
        self.normalized.read().await.get(trigger_id).cloned()
    }

    /// Returns the compiled IR for one trigger.
    pub async fn get_compiled_trigger(&self, trigger_id: &str) -> Option<TriggerIr> {
        self.compiled.read().await.get(trigger_id).cloned()
    }

    /// Lists all trigger definition IDs currently active for matching.
    pub async fn list_active_trigger_ids(&self) -> Vec<String> {
        self.active_compiled.read().await.keys().cloned().collect()
    }

    /// Loads runtime status for one trigger definition.
    pub async fn get_runtime_status(
        &self,
        trigger_id: &str,
    ) -> OpenFangResult<Option<TriggerRuntimeStatus>> {
        if let Some(record) = self.runtime_store.get_trigger_runtime(trigger_id)? {
            return Ok(Some(runtime_status_from_record(record)));
        }

        Ok(self
            .definitions
            .read()
            .await
            .get(trigger_id)
            .map(default_runtime_status))
    }

    /// Builds an ordered snapshot matcher from the current active trigger set.
    pub async fn match_engine_snapshot(&self) -> OpenFangResult<TriggerMatchEngine> {
        let compiled = self
            .active_compiled
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let runtime_records = self
            .runtime_store
            .list_trigger_runtimes()?
            .into_iter()
            .map(|record| (record.trigger_id.clone(), record))
            .collect::<BTreeMap<_, _>>();

        let candidates = compiled
            .into_iter()
            .map(|compiled_trigger| {
                let runtime = runtime_records
                    .get(&compiled_trigger.trigger_id)
                    .cloned()
                    .map(runtime_status_from_record)
                    .unwrap_or_else(|| default_runtime_status_from_compiled(&compiled_trigger));
                TriggerMatchCandidate {
                    compiled: compiled_trigger,
                    runtime,
                }
            })
            .collect();

        Ok(TriggerMatchEngine::new(candidates))
    }

    /// Records one successful trigger fire in the durable runtime projection.
    pub async fn record_successful_fire(
        &self,
        trigger_id: &str,
        fired_at: DateTime<Utc>,
    ) -> OpenFangResult<TriggerRuntimeStatus> {
        let mut runtime = match self.get_runtime_status(trigger_id).await? {
            Some(runtime) => runtime,
            None => {
                let Some(compiled) = self.get_compiled_trigger(trigger_id).await else {
                    return Err(OpenFangError::Internal(format!(
                        "Trigger runtime state was missing for unknown trigger '{trigger_id}'"
                    )));
                };
                default_runtime_status_from_compiled(&compiled)
            }
        };

        runtime.fire_count = runtime.fire_count.saturating_add(1);
        runtime.last_fired_at = Some(fired_at.to_rfc3339());
        self.runtime_store
            .upsert_trigger_runtime(&runtime_record_from_status(&runtime, fired_at.to_rfc3339()))?;

        Ok(runtime)
    }

    /// Evaluates one trigger definition against a synthetic event.
    pub async fn evaluate_trigger(
        &self,
        trigger_id: &str,
        event: &TriggerEvent,
    ) -> OpenFangResult<Option<TriggerEvaluation>> {
        let Some(compiled) = self.get_compiled_trigger(trigger_id).await else {
            return Ok(None);
        };
        let runtime = self
            .get_runtime_status(trigger_id)
            .await?
            .unwrap_or_else(|| TriggerRuntimeStatus {
                trigger_id: trigger_id.to_string(),
                enabled: compiled.enabled,
                fire_count: 0,
                max_fires: compiled.max_fires,
                cooldown_secs: compiled.cooldown_secs,
                last_fired_at: None,
            });

        Ok(Some(evaluate_compiled_trigger(
            &compiled,
            &runtime,
            event,
            Utc::now(),
        )))
    }
}

fn compile_definition_set(
    definitions: &[TriggerV2Definition],
    registry: &TriggerCompileRegistry,
) -> Result<CompiledDefinitionSet, TriggerCompileError> {
    let mut definition_map = BTreeMap::new();
    let mut normalized_map = BTreeMap::new();
    let mut compiled_map = BTreeMap::new();

    for definition in definitions {
        let normalized = normalize_trigger_definition(definition, registry)?;
        let compiled = compile_normalized_trigger(&normalized);
        definition_map.insert(definition.id.clone(), definition.clone());
        normalized_map.insert(normalized.id.clone(), normalized);
        compiled_map.insert(compiled.trigger_id.clone(), compiled);
    }

    Ok((definition_map, normalized_map, compiled_map))
}

fn sync_runtime_store_set<'a, I>(
    runtime_store: &TriggerRuntimeStore,
    definitions: I,
) -> OpenFangResult<()>
where
    I: IntoIterator<Item = &'a TriggerV2Definition>,
{
    let definitions = definitions.into_iter().collect::<Vec<_>>();
    let known_ids = definitions
        .iter()
        .map(|definition| definition.id.clone())
        .collect::<BTreeSet<_>>();

    for definition in definitions {
        sync_runtime_record(runtime_store, definition)?;
    }

    for record in runtime_store.list_trigger_runtimes()? {
        if !known_ids.contains(&record.trigger_id) {
            runtime_store.remove_trigger_runtime(&record.trigger_id)?;
        }
    }

    Ok(())
}

fn sync_runtime_record(
    runtime_store: &TriggerRuntimeStore,
    definition: &TriggerV2Definition,
) -> OpenFangResult<()> {
    let existing = runtime_store.get_trigger_runtime(&definition.id)?;
    let mut record = existing.unwrap_or_else(|| TriggerRuntimeRecord {
        trigger_id: definition.id.clone(),
        enabled: definition.enabled,
        fire_count: 0,
        max_fires: definition.max_fires,
        cooldown_secs: definition.cooldown_secs,
        last_fired_at: None,
        updated_at: Utc::now().to_rfc3339(),
    });

    record.enabled = definition.enabled;
    record.max_fires = definition.max_fires;
    record.cooldown_secs = definition.cooldown_secs;
    record.updated_at = Utc::now().to_rfc3339();
    runtime_store.upsert_trigger_runtime(&record)
}

fn runtime_status_from_record(record: TriggerRuntimeRecord) -> TriggerRuntimeStatus {
    TriggerRuntimeStatus {
        trigger_id: record.trigger_id,
        enabled: record.enabled,
        fire_count: record.fire_count,
        max_fires: record.max_fires,
        cooldown_secs: record.cooldown_secs,
        last_fired_at: record.last_fired_at,
    }
}

fn runtime_record_from_status(
    runtime: &TriggerRuntimeStatus,
    updated_at: String,
) -> TriggerRuntimeRecord {
    TriggerRuntimeRecord {
        trigger_id: runtime.trigger_id.clone(),
        enabled: runtime.enabled,
        fire_count: runtime.fire_count,
        max_fires: runtime.max_fires,
        cooldown_secs: runtime.cooldown_secs,
        last_fired_at: runtime.last_fired_at.clone(),
        updated_at,
    }
}

fn default_runtime_status(definition: &TriggerV2Definition) -> TriggerRuntimeStatus {
    TriggerRuntimeStatus {
        trigger_id: definition.id.clone(),
        enabled: definition.enabled,
        fire_count: 0,
        max_fires: definition.max_fires,
        cooldown_secs: definition.cooldown_secs,
        last_fired_at: None,
    }
}

fn default_runtime_status_from_compiled(compiled: &TriggerIr) -> TriggerRuntimeStatus {
    TriggerRuntimeStatus {
        trigger_id: compiled.trigger_id.clone(),
        enabled: compiled.enabled,
        fire_count: 0,
        max_fires: compiled.max_fires,
        cooldown_secs: compiled.cooldown_secs,
        last_fired_at: None,
    }
}

/// Parses a JSON trigger document and returns a typed definition when schema
/// and semantic validation succeed.
pub fn validate_trigger_value(
    value: &Value,
    registry: &TriggerCompileRegistry,
) -> Result<TriggerV2Definition, TriggerCompileError> {
    let mut issues = Vec::new();
    collect_schema_validation_issues(value, &mut issues);

    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(TriggerCompileError::new(issues));
    }

    let definition = match serde_json::from_value::<TriggerV2Definition>(value.clone()) {
        Ok(definition) => definition,
        Err(error) => {
            issues.push(TriggerValidationIssue::error(
                "invalid_definition",
                "",
                format!("trigger definition could not be deserialized: {error}"),
            ));
            return Err(TriggerCompileError::new(issues));
        }
    };

    issues.extend(validate_trigger_definition(&definition, registry));
    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(TriggerCompileError::new(issues));
    }

    Ok(definition)
}

/// Validates a typed trigger definition without compiling it.
#[must_use]
pub fn validate_trigger_definition(
    definition: &TriggerV2Definition,
    registry: &TriggerCompileRegistry,
) -> Vec<TriggerValidationIssue> {
    let mut issues = Vec::new();

    validate_required_string(&definition.id, "id", &mut issues);
    validate_required_string(&definition.name, "name", &mut issues);
    validate_required_string(&definition.description, "description", &mut issues);

    if definition
        .trigger_match
        .event
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        issues.push(TriggerValidationIssue::error(
            "invalid_field_value",
            "match.event",
            "`match.event` must not be empty",
        ));
    }
    if definition
        .trigger_match
        .source
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        issues.push(TriggerValidationIssue::error(
            "invalid_field_value",
            "match.source",
            "`match.source` must not be empty",
        ));
    }
    if definition
        .trigger_match
        .contains
        .as_deref()
        .is_some_and(|value| value.trim().is_empty())
    {
        issues.push(TriggerValidationIssue::error(
            "invalid_field_value",
            "match.contains",
            "`match.contains` must not be empty",
        ));
    }
    if definition.trigger_match.event.is_none()
        && definition.trigger_match.source.is_none()
        && definition.trigger_match.contains.is_none()
        && definition.trigger_match.filters.is_empty()
    {
        issues.push(TriggerValidationIssue::warning(
            "broad_match",
            "match",
            "trigger match is empty and will match every event",
        ));
    }

    for path in definition.trigger_match.filters.keys() {
        if path.trim().is_empty() {
            issues.push(TriggerValidationIssue::error(
                "invalid_field_value",
                "match.filters",
                "filter keys must not be empty",
            ));
        }
    }

    match &definition.target {
        TriggerTarget::AgentMessage { agent, input, .. } => {
            validate_required_string(agent, "target.agent", &mut issues);
            if !agent.trim().is_empty() && registry.resolve_agent(agent).is_none() {
                issues.push(TriggerValidationIssue::error(
                    "not_found",
                    "target.agent",
                    format!("agent definition not found: {}", agent.trim()),
                ));
            }
            for (index, item) in input.items.iter().enumerate() {
                if item.item_type.trim().is_empty() {
                    issues.push(TriggerValidationIssue::error(
                        "missing_field",
                        format!("target.input.items.{index}.type"),
                        "`target.input.items[].type` is required",
                    ));
                } else if item.item_type != "text" {
                    issues.push(TriggerValidationIssue::error(
                        "unsupported_input_item",
                        format!("target.input.items.{index}.type"),
                        format!("unsupported trigger input item type `{}`", item.item_type),
                    ));
                }
                if item.item_type == "text"
                    && item
                        .text
                        .as_deref()
                        .is_none_or(|text| text.trim().is_empty())
                {
                    issues.push(TriggerValidationIssue::error(
                        "missing_field",
                        format!("target.input.items.{index}.text"),
                        "`target.input.items[].text` is required for `text` items",
                    ));
                }
            }
        }
        TriggerTarget::WorkflowStart { workflow, .. } => {
            validate_required_string(workflow, "target.workflow", &mut issues);
            if !workflow.trim().is_empty() && registry.resolve_workflow(workflow).is_none() {
                issues.push(TriggerValidationIssue::error(
                    "not_found",
                    "target.workflow",
                    format!("workflow definition not found: {}", workflow.trim()),
                ));
            }
        }
        TriggerTarget::WorkflowSignal {
            signal, selector, ..
        } => {
            validate_required_string(signal, "target.signal", &mut issues);
            validate_required_string(
                &selector.workflow_id,
                "target.selector.workflow_id",
                &mut issues,
            );
            if !selector.workflow_id.trim().is_empty()
                && registry.resolve_workflow(&selector.workflow_id).is_none()
            {
                issues.push(TriggerValidationIssue::error(
                    "not_found",
                    "target.selector.workflow_id",
                    format!(
                        "workflow definition not found: {}",
                        selector.workflow_id.trim()
                    ),
                ));
            }
        }
    }

    issues
}

/// Normalizes a trigger definition into its canonical control-plane shape.
pub fn normalize_trigger_definition(
    definition: &TriggerV2Definition,
    registry: &TriggerCompileRegistry,
) -> Result<NormalizedTrigger, TriggerCompileError> {
    let issues = validate_trigger_definition(definition, registry);
    if issues.iter().any(|issue| issue.severity.is_error()) {
        return Err(TriggerCompileError::new(issues));
    }

    let target = match &definition.target {
        TriggerTarget::AgentMessage {
            agent,
            input,
            metadata,
        } => TriggerTarget::AgentMessage {
            agent: registry
                .resolve_agent(agent)
                .unwrap_or_else(|| agent.trim().to_string()),
            input: input.clone(),
            metadata: metadata.clone(),
        },
        TriggerTarget::WorkflowStart { workflow, input } => TriggerTarget::WorkflowStart {
            workflow: registry
                .resolve_workflow(workflow)
                .unwrap_or_else(|| workflow.trim().to_string()),
            input: input.clone(),
        },
        TriggerTarget::WorkflowSignal {
            signal,
            selector,
            payload,
        } => TriggerTarget::WorkflowSignal {
            signal: signal.trim().to_string(),
            selector: TriggerWorkflowSignalSelector {
                workflow_id: registry
                    .resolve_workflow(&selector.workflow_id)
                    .unwrap_or_else(|| selector.workflow_id.trim().to_string()),
            },
            payload: payload.clone(),
        },
    };

    Ok(NormalizedTrigger {
        id: definition.id.trim().to_string(),
        name: definition.name.trim().to_string(),
        description: definition.description.trim().to_string(),
        enabled: definition.enabled,
        max_fires: definition.max_fires,
        cooldown_secs: definition.cooldown_secs,
        trigger_match: openfang_types::trigger::TriggerMatch {
            event: definition
                .trigger_match
                .event
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            source: definition
                .trigger_match
                .source
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            contains: definition
                .trigger_match
                .contains
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
            filters: definition.trigger_match.filters.clone(),
        },
        target,
    })
}

/// Compiles a typed trigger definition into IR.
pub fn compile_trigger_definition(
    definition: &TriggerV2Definition,
    registry: &TriggerCompileRegistry,
) -> Result<TriggerIr, TriggerCompileError> {
    let normalized = normalize_trigger_definition(definition, registry)?;
    Ok(compile_normalized_trigger(&normalized))
}

/// Builds a compiled trigger IR from a normalized trigger definition.
#[must_use]
pub fn compile_normalized_trigger(normalized: &NormalizedTrigger) -> TriggerIr {
    TriggerIr {
        trigger_id: normalized.id.clone(),
        trigger_match: normalized.trigger_match.clone(),
        target: normalized.target.clone(),
        dispatch_action: normalized.target.dispatch_action(),
        enabled: normalized.enabled,
        max_fires: normalized.max_fires,
        cooldown_secs: normalized.cooldown_secs,
    }
}

/// Evaluates a compiled trigger against an event without dispatching it.
#[must_use]
pub fn evaluate_compiled_trigger(
    compiled: &TriggerIr,
    runtime: &TriggerRuntimeStatus,
    event: &TriggerEvent,
    now: DateTime<Utc>,
) -> TriggerEvaluation {
    if !runtime.enabled {
        return blocked_trigger_evaluation(compiled, "disabled", "trigger is disabled");
    }

    if runtime.max_fires > 0 && runtime.fire_count >= runtime.max_fires {
        return blocked_trigger_evaluation(
            compiled,
            "max_fires_reached",
            "trigger reached its max_fires limit",
        );
    }

    if runtime.cooldown_secs > 0
        && cooldown_active(runtime.last_fired_at.as_deref(), runtime.cooldown_secs, now)
    {
        return blocked_trigger_evaluation(
            compiled,
            "cooldown_active",
            "trigger is inside its cooldown window",
        );
    }

    let mut details = Vec::new();
    let matched = match_trigger(&compiled.trigger_match, event, &mut details);
    if matched {
        details.push("all configured match clauses satisfied".to_string());
    }

    TriggerEvaluation {
        matched,
        resolved_target: matched.then_some(compiled.target.clone()),
        would_dispatch: matched,
        explanation: TriggerEvaluationExplanation {
            match_summary: details.join("; "),
            target_kind: compiled.target.kind().to_string(),
            blocked_by: None,
        },
    }
}

fn blocked_trigger_evaluation(
    compiled: &TriggerIr,
    blocked_by: &str,
    summary: &str,
) -> TriggerEvaluation {
    TriggerEvaluation {
        matched: false,
        resolved_target: Some(compiled.target.clone()),
        would_dispatch: false,
        explanation: TriggerEvaluationExplanation {
            match_summary: summary.to_string(),
            target_kind: compiled.target.kind().to_string(),
            blocked_by: Some(blocked_by.to_string()),
        },
    }
}

fn cooldown_active(last_fired_at: Option<&str>, cooldown_secs: u64, now: DateTime<Utc>) -> bool {
    let Some(last_fired_at) = last_fired_at else {
        return false;
    };
    let Ok(last_fired_at) = DateTime::parse_from_rfc3339(last_fired_at) else {
        return false;
    };
    let Ok(cooldown_secs) = i64::try_from(cooldown_secs) else {
        return false;
    };

    last_fired_at.with_timezone(&Utc) + Duration::seconds(cooldown_secs) > now
}

fn match_trigger(
    trigger_match: &openfang_types::trigger::TriggerMatch,
    event: &TriggerEvent,
    details: &mut Vec<String>,
) -> bool {
    if let Some(expected) = trigger_match.event.as_deref() {
        if event.event != expected {
            details.push(format!(
                "event mismatch: expected `{expected}`, got `{}`",
                event.event
            ));
            return false;
        }
    }

    if let Some(expected) = trigger_match.source.as_deref() {
        if event.source != expected {
            details.push(format!(
                "source mismatch: expected `{expected}`, got `{}`",
                event.source
            ));
            return false;
        }
    }

    if let Some(needle) = trigger_match.contains.as_deref() {
        let payload = event.payload.to_string().to_lowercase();
        if !payload.contains(&needle.to_lowercase()) {
            details.push(format!(
                "payload did not contain case-insensitive substring `{needle}`"
            ));
            return false;
        }
    }

    for (path, expected) in &trigger_match.filters {
        let Some(actual) = resolve_json_path(&event.payload, path) else {
            details.push(format!("payload filter path `{path}` was missing"));
            return false;
        };
        if actual != expected {
            details.push(format!(
                "payload filter `{path}` mismatched: expected `{expected}`, got `{actual}`"
            ));
            return false;
        }
    }

    true
}

fn resolve_json_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    for segment in path.split('.') {
        current = current.get(segment)?;
    }
    Some(current)
}

fn validate_required_string(value: &str, path: &str, issues: &mut Vec<TriggerValidationIssue>) {
    if value.trim().is_empty() {
        issues.push(TriggerValidationIssue::error(
            "missing_field",
            path,
            format!("`{path}` is required"),
        ));
    }
}

fn collect_schema_validation_issues(value: &Value, issues: &mut Vec<TriggerValidationIssue>) {
    let Some(object) = value.as_object() else {
        issues.push(TriggerValidationIssue::error(
            "invalid_type",
            "",
            "trigger definition must be a JSON object",
        ));
        return;
    };

    validate_required_object_field(object, "match", issues);
    validate_required_object_field(object, "target", issues);
    validate_optional_string_field(object, "id", issues);
    validate_optional_string_field(object, "name", issues);
    validate_optional_string_field(object, "description", issues);
    validate_optional_bool_field(object, "enabled", issues);
    validate_optional_u64_field(object, "max_fires", issues);
    validate_optional_u64_field(object, "cooldown_secs", issues);

    if let Some(trigger_match) = object.get("match") {
        if let Some(trigger_match) = trigger_match.as_object() {
            validate_optional_string_field_prefixed(
                trigger_match,
                "event",
                issues_with_prefix("match", issues),
            );
            validate_optional_string_field_prefixed(
                trigger_match,
                "source",
                issues_with_prefix("match", issues),
            );
            validate_optional_string_field_prefixed(
                trigger_match,
                "contains",
                issues_with_prefix("match", issues),
            );
            validate_optional_object_field_prefixed(
                trigger_match,
                "filters",
                issues_with_prefix("match", issues),
            );
        } else {
            issues.push(TriggerValidationIssue::error(
                "invalid_type",
                "match",
                "`match` must be an object",
            ));
        }
    }

    if let Some(target) = object.get("target") {
        let Some(target) = target.as_object() else {
            issues.push(TriggerValidationIssue::error(
                "invalid_type",
                "target",
                "`target` must be an object",
            ));
            return;
        };
        let Some(kind) = target.get("kind").and_then(Value::as_str) else {
            issues.push(TriggerValidationIssue::error(
                "missing_field",
                "target.kind",
                "`target.kind` is required",
            ));
            return;
        };

        match kind {
            "agent_message" => {
                validate_required_string_value(target, "agent", "target.agent", issues);
                validate_optional_object_field_prefixed(
                    target,
                    "input",
                    issues_with_prefix("target", issues),
                );
            }
            "workflow_start" => {
                validate_required_string_value(target, "workflow", "target.workflow", issues);
            }
            "workflow_signal" => {
                validate_required_string_value(target, "signal", "target.signal", issues);
                let Some(selector) = target.get("selector") else {
                    issues.push(TriggerValidationIssue::error(
                        "missing_field",
                        "target.selector",
                        "`target.selector` is required",
                    ));
                    return;
                };
                let Some(selector) = selector.as_object() else {
                    issues.push(TriggerValidationIssue::error(
                        "invalid_type",
                        "target.selector",
                        "`target.selector` must be an object",
                    ));
                    return;
                };
                validate_required_string_value(
                    selector,
                    "workflow_id",
                    "target.selector.workflow_id",
                    issues,
                );
            }
            other => {
                issues.push(TriggerValidationIssue::error(
                    "unsupported_target_kind",
                    "target.kind",
                    format!("unsupported trigger target kind `{other}`"),
                ));
            }
        }
    }
}

fn validate_required_object_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<TriggerValidationIssue>,
) {
    let Some(value) = object.get(field) else {
        issues.push(TriggerValidationIssue::error(
            "missing_field",
            field,
            format!("`{field}` is required"),
        ));
        return;
    };
    if !value.is_object() {
        issues.push(TriggerValidationIssue::error(
            "invalid_type",
            field,
            format!("`{field}` must be an object"),
        ));
    }
}

fn validate_optional_string_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<TriggerValidationIssue>,
) {
    if let Some(value) = object.get(field) {
        if !value.is_string() {
            issues.push(TriggerValidationIssue::error(
                "invalid_type",
                field,
                format!("`{field}` must be a string"),
            ));
        }
    }
}

fn validate_optional_bool_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<TriggerValidationIssue>,
) {
    if let Some(value) = object.get(field) {
        if !value.is_boolean() {
            issues.push(TriggerValidationIssue::error(
                "invalid_type",
                field,
                format!("`{field}` must be a boolean"),
            ));
        }
    }
}

fn validate_optional_u64_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<TriggerValidationIssue>,
) {
    if let Some(value) = object.get(field) {
        if !value.is_u64() {
            issues.push(TriggerValidationIssue::error(
                "invalid_type",
                field,
                format!("`{field}` must be an unsigned integer"),
            ));
        }
    }
}

fn validate_required_string_value(
    object: &serde_json::Map<String, Value>,
    field: &str,
    path: &str,
    issues: &mut Vec<TriggerValidationIssue>,
) {
    let Some(value) = object.get(field) else {
        issues.push(TriggerValidationIssue::error(
            "missing_field",
            path,
            format!("`{path}` is required"),
        ));
        return;
    };
    let Some(value) = value.as_str() else {
        issues.push(TriggerValidationIssue::error(
            "invalid_type",
            path,
            format!("`{path}` must be a string"),
        ));
        return;
    };
    if value.trim().is_empty() {
        issues.push(TriggerValidationIssue::error(
            "missing_field",
            path,
            format!("`{path}` is required"),
        ));
    }
}

fn issues_with_prefix<'a>(
    prefix: &'a str,
    issues: &'a mut Vec<TriggerValidationIssue>,
) -> PrefixedIssues<'a> {
    PrefixedIssues { prefix, issues }
}

struct PrefixedIssues<'a> {
    prefix: &'a str,
    issues: &'a mut Vec<TriggerValidationIssue>,
}

impl PrefixedIssues<'_> {
    fn push(&mut self, issue: TriggerValidationIssue) {
        let path = if issue.path.is_empty() {
            self.prefix.to_string()
        } else {
            format!("{}.{}", self.prefix, issue.path)
        };
        self.issues.push(TriggerValidationIssue { path, ..issue });
    }
}

fn validate_optional_string_field_prefixed(
    object: &serde_json::Map<String, Value>,
    field: &str,
    mut issues: PrefixedIssues<'_>,
) {
    if let Some(value) = object.get(field) {
        if !value.is_string() {
            issues.push(TriggerValidationIssue::error(
                "invalid_type",
                field,
                format!("`{field}` must be a string"),
            ));
        }
    }
}

fn validate_optional_object_field_prefixed(
    object: &serde_json::Map<String, Value>,
    field: &str,
    mut issues: PrefixedIssues<'_>,
) {
    if let Some(value) = object.get(field) {
        if !value.is_object() {
            issues.push(TriggerValidationIssue::error(
                "invalid_type",
                field,
                format!("`{field}` must be an object"),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compile_trigger_definition, evaluate_compiled_trigger, validate_trigger_definition,
        validate_trigger_value, TriggerCompileRegistry, TriggerV2Engine,
    };
    use chrono::{Duration, Utc};
    use openfang_memory::TRIGGER_RUNTIME_CORE_MIGRATION_SQL;
    use openfang_types::trigger::{
        TriggerEvent, TriggerInputItem, TriggerInputPayload, TriggerTarget, TriggerV2Definition,
    };
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    fn registry() -> TriggerCompileRegistry {
        let mut registry = TriggerCompileRegistry::new();
        registry.insert_agent("reviewer", Some("Reviewer Agent"));
        registry.insert_workflow("sdlc", Some("SDLC"));
        registry
    }

    fn sample_definition() -> TriggerV2Definition {
        TriggerV2Definition {
            id: "issue-created".to_string(),
            name: "Issue Created".to_string(),
            description: "Start SDLC when an issue is created".to_string(),
            enabled: true,
            max_fires: 3,
            cooldown_secs: 30,
            trigger_match: openfang_types::trigger::TriggerMatch {
                event: Some("issue.created".to_string()),
                source: Some("api".to_string()),
                contains: None,
                filters: BTreeMap::from([("issue.priority".to_string(), json!("high"))]),
            },
            target: TriggerTarget::WorkflowStart {
                workflow: "sdlc".to_string(),
                input: json!({ "from": "trigger" }),
            },
        }
    }

    fn wildcard_definition() -> TriggerV2Definition {
        TriggerV2Definition {
            id: "wildcard-trigger".to_string(),
            name: "Wildcard".to_string(),
            description: "Matches any event".to_string(),
            enabled: true,
            max_fires: 0,
            cooldown_secs: 0,
            trigger_match: openfang_types::trigger::TriggerMatch {
                event: None,
                source: None,
                contains: None,
                filters: BTreeMap::new(),
            },
            target: TriggerTarget::WorkflowStart {
                workflow: "sdlc".to_string(),
                input: json!({}),
            },
        }
    }

    fn contains_definition() -> TriggerV2Definition {
        TriggerV2Definition {
            id: "contains-trigger".to_string(),
            name: "Contains".to_string(),
            description: "Matches payload text".to_string(),
            enabled: true,
            max_fires: 0,
            cooldown_secs: 0,
            trigger_match: openfang_types::trigger::TriggerMatch {
                event: Some("issue.created".to_string()),
                source: Some("api".to_string()),
                contains: Some("ISSUE-123".to_string()),
                filters: BTreeMap::new(),
            },
            target: TriggerTarget::WorkflowStart {
                workflow: "sdlc".to_string(),
                input: json!({}),
            },
        }
    }

    fn runtime_store() -> openfang_memory::TriggerRuntimeStore {
        let conn = Connection::open_in_memory().expect("open runtime.db");
        conn.execute_batch(TRIGGER_RUNTIME_CORE_MIGRATION_SQL)
            .expect("apply trigger runtime schema");
        openfang_memory::TriggerRuntimeStore::new(Arc::new(Mutex::new(conn)))
    }

    #[test]
    fn workflow_signal_should_require_selector_workflow_id() {
        let error = validate_trigger_value(
            &json!({
                "id": "waiter",
                "name": "Waiter",
                "description": "Signals a workflow",
                "enabled": true,
                "max_fires": 0,
                "cooldown_secs": 0,
                "match": {
                    "event": "ticket.updated"
                },
                "target": {
                    "kind": "workflow_signal",
                    "signal": "resume",
                    "selector": {}
                }
            }),
            &registry(),
        )
        .expect_err("selector.workflow_id should be required");

        assert_eq!(
            error.issues()[0].path,
            "target.selector.workflow_id".to_string()
        );
    }

    #[test]
    fn broad_match_should_emit_warning() {
        let definition = TriggerV2Definition {
            id: "catch-all".to_string(),
            name: "Catch All".to_string(),
            description: "Matches everything".to_string(),
            enabled: true,
            max_fires: 0,
            cooldown_secs: 0,
            trigger_match: openfang_types::trigger::TriggerMatch::default(),
            target: TriggerTarget::AgentMessage {
                agent: "reviewer".to_string(),
                input: TriggerInputPayload {
                    items: vec![TriggerInputItem {
                        item_type: "text".to_string(),
                        text: Some("hello".to_string()),
                    }],
                },
                metadata: None,
            },
        };

        let issues = validate_trigger_definition(&definition, &registry());
        assert_eq!(issues[0].path, "match".to_string());
        assert_eq!(issues[0].code, "broad_match".to_string());
    }

    #[test]
    fn compile_should_return_non_empty_ir() {
        let compiled = compile_trigger_definition(&sample_definition(), &registry())
            .expect("compile should succeed");

        assert_eq!(compiled.trigger_id, "issue-created".to_string());
        assert_eq!(
            compiled.dispatch_action,
            openfang_types::trigger::TriggerDispatchAction::WorkflowRunCreate
        );
    }

    #[tokio::test]
    async fn enable_disable_should_update_active_matching_set() {
        let engine = TriggerV2Engine::new(runtime_store());
        let registry = registry();
        let definition = sample_definition();

        engine
            .upsert_definition(definition.clone(), &registry)
            .await
            .expect("insert active definition");
        assert_eq!(
            engine.list_active_trigger_ids().await,
            vec!["issue-created".to_string()]
        );

        let mut disabled = definition.clone();
        disabled.enabled = false;
        engine
            .upsert_definition(disabled.clone(), &registry)
            .await
            .expect("disable definition");
        assert!(engine.list_active_trigger_ids().await.is_empty());

        let mut enabled_again = disabled;
        enabled_again.enabled = true;
        engine
            .upsert_definition(enabled_again, &registry)
            .await
            .expect("enable definition");
        assert_eq!(
            engine.list_active_trigger_ids().await,
            vec!["issue-created".to_string()]
        );
    }

    #[test]
    fn evaluation_should_respect_runtime_guards() {
        let compiled = compile_trigger_definition(&sample_definition(), &registry())
            .expect("compile should succeed");
        let event = TriggerEvent {
            event: "issue.created".to_string(),
            source: "api".to_string(),
            payload: json!({
                "issue": {
                    "priority": "high"
                }
            }),
        };
        let runtime = openfang_types::trigger::TriggerRuntimeStatus {
            trigger_id: "issue-created".to_string(),
            enabled: false,
            fire_count: 0,
            max_fires: 3,
            cooldown_secs: 30,
            last_fired_at: None,
        };

        let evaluation = evaluate_compiled_trigger(&compiled, &runtime, &event, Utc::now());

        assert!(!evaluation.matched);
        assert!(!evaluation.would_dispatch);
        assert_eq!(
            evaluation.explanation.blocked_by,
            Some("disabled".to_string())
        );
    }

    #[tokio::test]
    async fn match_engine_should_match_exact_event_name() {
        let engine = TriggerV2Engine::new(runtime_store());
        let registry = registry();
        let definition = sample_definition();
        engine
            .upsert_definition(definition, &registry)
            .await
            .expect("definition should load");
        let matcher = engine
            .match_engine_snapshot()
            .await
            .expect("match engine snapshot should build");

        let matching_event = TriggerEvent {
            event: "issue.created".to_string(),
            source: "api".to_string(),
            payload: json!({
                "issue": { "priority": "high" }
            }),
        };
        let non_matching_event = TriggerEvent {
            event: "issue.updated".to_string(),
            source: "api".to_string(),
            payload: json!({
                "issue": { "priority": "high" }
            }),
        };

        let matching = matcher.match_event(&matching_event, Utc::now());
        let non_matching = matcher.match_event(&non_matching_event, Utc::now());

        assert_eq!(matching.len(), 1);
        assert!(non_matching.is_empty());
    }

    #[tokio::test]
    async fn match_engine_should_match_contains_substring() {
        let engine = TriggerV2Engine::new(runtime_store());
        let registry = registry();
        engine
            .upsert_definition(contains_definition(), &registry)
            .await
            .expect("definition should load");
        let matcher = engine
            .match_engine_snapshot()
            .await
            .expect("match engine snapshot should build");

        let matching_event = TriggerEvent {
            event: "issue.created".to_string(),
            source: "api".to_string(),
            payload: json!({
                "issue_id": "ISSUE-123"
            }),
        };
        let non_matching_event = TriggerEvent {
            event: "issue.created".to_string(),
            source: "api".to_string(),
            payload: json!({
                "issue_id": "ISSUE-999"
            }),
        };

        let matching = matcher.match_event(&matching_event, Utc::now());
        let non_matching = matcher.match_event(&non_matching_event, Utc::now());

        assert_eq!(matching.len(), 1);
        assert!(non_matching.is_empty());
    }

    #[tokio::test]
    async fn match_engine_should_treat_missing_event_as_wildcard() {
        let engine = TriggerV2Engine::new(runtime_store());
        let registry = registry();
        engine
            .upsert_definition(wildcard_definition(), &registry)
            .await
            .expect("definition should load");
        let matcher = engine
            .match_engine_snapshot()
            .await
            .expect("match engine snapshot should build");

        let event = TriggerEvent {
            event: "anything.happened".to_string(),
            source: "worker".to_string(),
            payload: json!({
                "message": "wildcard"
            }),
        };

        let matches = matcher.match_event(&event, Utc::now());

        assert_eq!(matches.len(), 1);
    }

    #[tokio::test]
    async fn match_engine_should_skip_trigger_after_max_fires_reached() {
        let engine = TriggerV2Engine::new(runtime_store());
        let registry = registry();
        let definition = sample_definition();
        engine
            .upsert_definition(definition.clone(), &registry)
            .await
            .expect("definition should load");
        engine
            .runtime_store
            .upsert_trigger_runtime(&openfang_memory::TriggerRuntimeRecord {
                trigger_id: definition.id.clone(),
                enabled: true,
                fire_count: 3,
                max_fires: 3,
                cooldown_secs: 0,
                last_fired_at: None,
                updated_at: Utc::now().to_rfc3339(),
            })
            .expect("runtime should update");

        let matcher = engine
            .match_engine_snapshot()
            .await
            .expect("match engine snapshot should build");
        let event = TriggerEvent {
            event: "issue.created".to_string(),
            source: "api".to_string(),
            payload: json!({
                "issue": { "priority": "high" }
            }),
        };

        let report = matcher.evaluate_event(&event, Utc::now());

        assert!(report.dispatchable_matches().is_empty());
        assert_eq!(
            report.evaluations[0].explanation.blocked_by.as_deref(),
            Some("max_fires_reached")
        );
    }

    #[tokio::test]
    async fn match_engine_should_skip_trigger_with_active_cooldown() {
        let engine = TriggerV2Engine::new(runtime_store());
        let registry = registry();
        let definition = sample_definition();
        engine
            .upsert_definition(definition.clone(), &registry)
            .await
            .expect("definition should load");
        let last_fired_at = (Utc::now() - Duration::seconds(30)).to_rfc3339();
        engine
            .runtime_store
            .upsert_trigger_runtime(&openfang_memory::TriggerRuntimeRecord {
                trigger_id: definition.id.clone(),
                enabled: true,
                fire_count: 1,
                max_fires: 3,
                cooldown_secs: 60,
                last_fired_at: Some(last_fired_at),
                updated_at: Utc::now().to_rfc3339(),
            })
            .expect("runtime should update");

        let matcher = engine
            .match_engine_snapshot()
            .await
            .expect("match engine snapshot should build");
        let event = TriggerEvent {
            event: "issue.created".to_string(),
            source: "api".to_string(),
            payload: json!({
                "issue": { "priority": "high" }
            }),
        };

        let report = matcher.evaluate_event(&event, Utc::now());

        assert!(report.dispatchable_matches().is_empty());
        assert_eq!(
            report.evaluations[0].explanation.blocked_by.as_deref(),
            Some("cooldown_active")
        );
    }

    #[tokio::test]
    async fn record_successful_fire_should_increment_fire_count_and_timestamp() {
        let engine = TriggerV2Engine::new(runtime_store());
        let registry = registry();
        let definition = sample_definition();
        let trigger_id = definition.id.clone();
        engine
            .upsert_definition(definition, &registry)
            .await
            .expect("definition should load");
        let fired_at = Utc::now();

        let runtime = engine
            .record_successful_fire(&trigger_id, fired_at)
            .await
            .expect("fire state should persist");

        assert_eq!(runtime.fire_count, 1);
        assert_eq!(runtime.last_fired_at, Some(fired_at.to_rfc3339()));
    }
}
