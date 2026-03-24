//! Workflow engine — multi-step agent pipeline execution.
//!
//! A workflow defines a sequence of steps where each step routes
//! a task to a specific agent. Steps can:
//! - Pass their output as input to the next step
//! - Run in sequence (pipeline) or in parallel (fan-out)
//! - Conditionally skip based on previous output
//! - Loop until a condition is met
//! - Store outputs in named variables for later reference
//!
//! Workflows are defined as Rust structs or loaded from JSON.

use crate::workflow_compiler::{
    compile_workflow_definition, WorkflowCompileError, WorkflowCompileRegistry,
};
use chrono::{DateTime, Utc};
use openfang_memory::{
    now_timestamp, CheckpointKind as DurableCheckpointKind, SubmittedSignalResume,
    WorkflowCheckpointRecord, WorkflowRunRecord, WorkflowRunStatus as DurableWorkflowRunStatus,
    WorkflowStoreError, WorkflowStoreSet, WORKFLOW_CHECKPOINT_MIGRATION_SQL,
    WORKFLOW_RUNTIME_DURABILITY_MIGRATION_SQL, WORKFLOW_RUN_CONTROL_PLANE_MIGRATION_SQL,
    WORKFLOW_RUN_CORE_MIGRATION_SQL, WORKFLOW_SIGNAL_MIGRATION_SQL,
    WORKFLOW_SIGNAL_WAITING_STATE_MIGRATION_SQL,
};
use openfang_types::agent::AgentId;
use openfang_types::error::{OpenFangError, OpenFangResult};
use openfang_types::workflow::{
    CompiledTemplate, ErrorMode as WorkflowV2ErrorMode, FlowBlock, FlowMode as WorkflowV2FlowMode,
    ResolvedRuntimeSettings, TemplateNamespace, TemplateReference, TemplateSegment, WorkflowIr,
    WorkflowIrStep, WorkflowIrStepKind, WorkflowV2Definition,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{Mutex, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Unique identifier for a workflow definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub Uuid);

impl WorkflowId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WorkflowId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a running workflow instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowRunId(pub Uuid);

impl WorkflowRunId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WorkflowRunId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkflowRunId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A workflow definition — a named sequence of steps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    /// Unique identifier.
    pub id: WorkflowId,
    /// Human-readable name.
    pub name: String,
    /// Description of what this workflow does.
    pub description: String,
    /// The steps in execution order.
    pub steps: Vec<WorkflowStep>,
    /// Created at.
    pub created_at: DateTime<Utc>,
}

/// A single step in a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowStep {
    /// Step name for logging/display.
    pub name: String,
    /// Which agent to route this step to.
    pub agent: StepAgent,
    /// The prompt template. Use `{{input}}` for previous output, `{{var_name}}` for variables.
    pub prompt_template: String,
    /// Execution mode for this step.
    pub mode: StepMode,
    /// Maximum time for this step in seconds (default: 120).
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// Error handling mode for this step (default: Fail).
    #[serde(default)]
    pub error_mode: ErrorMode,
    /// Optional variable name to store this step's output in.
    #[serde(default)]
    pub output_var: Option<String>,
}

fn default_timeout() -> u64 {
    120
}

/// How to identify the agent for a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StepAgent {
    /// Reference an agent by UUID.
    ById { id: String },
    /// Reference an agent by name (first match).
    ByName { name: String },
}

/// Execution mode for a workflow step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepMode {
    /// Execute sequentially — this step runs after the previous completes.
    #[default]
    Sequential,
    /// Fan-out — this step runs in parallel with subsequent FanOut steps until Collect.
    FanOut,
    /// Collect results from all preceding fan-out steps.
    Collect,
    /// Conditional — skip this step if previous output doesn't contain `condition` (case-insensitive).
    Conditional { condition: String },
    /// Loop — repeat this step until output contains `until` or `max_iterations` reached.
    Loop { max_iterations: u32, until: String },
}

/// Error handling mode for a workflow step.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorMode {
    /// Abort the workflow on error (default).
    #[default]
    Fail,
    /// Skip this step on error and continue.
    Skip,
    /// Retry the step up to N times before failing.
    Retry { max_retries: u32 },
}

/// The current state of a workflow run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunState {
    Pending,
    Running,
    WaitingSignal,
    WaitingHitl,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// A running workflow instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    /// Run instance ID.
    pub id: WorkflowRunId,
    /// The workflow being run.
    pub workflow_id: WorkflowId,
    /// Compiled workflow version for this run.
    pub workflow_version: String,
    /// Workflow name (copied for quick access).
    pub workflow_name: String,
    /// Initial input to the workflow.
    pub input: String,
    /// Durable workflow variables encoded as JSON.
    pub vars_json: String,
    /// Current step ID mirrored from durable state.
    pub current_step_id: Option<String>,
    /// Waiting kind mirrored from durable state.
    pub waiting_kind: Option<String>,
    /// Waiting reference mirrored from durable state.
    pub waiting_ref: Option<String>,
    /// Active dispatch identifier, if any.
    pub active_dispatch_id: Option<String>,
    /// Active HITL request identifier, if any.
    pub active_hitl_request_id: Option<String>,
    /// Labels encoded as JSON.
    pub labels_json: String,
    /// Metadata encoded as JSON.
    pub metadata_json: String,
    /// Current state.
    pub state: WorkflowRunState,
    /// Results from each completed step.
    pub step_results: Vec<StepResult>,
    /// Final output (set when workflow completes).
    pub output: Option<String>,
    /// Error message if failed.
    pub error: Option<String>,
    /// Started at.
    pub started_at: DateTime<Utc>,
    /// Updated at.
    pub updated_at: DateTime<Utc>,
    /// Completed at.
    pub completed_at: Option<DateTime<Utc>>,
}

/// Result from a single workflow step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Step name.
    pub step_name: String,
    /// Agent that executed this step.
    pub agent_id: String,
    /// Agent name.
    pub agent_name: String,
    /// Output from this step.
    pub output: String,
    /// Token usage.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// Workflow registry readiness state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowRegistryReadiness {
    Bootstrapping = 0,
    Ready = 1,
}

impl WorkflowRegistryReadiness {
    fn from_stored(value: u8) -> Self {
        match value {
            1 => Self::Ready,
            _ => Self::Bootstrapping,
        }
    }
}

/// Severity of a workflow bootstrap error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowBootstrapErrorLevel {
    Warn,
    Error,
}

impl WorkflowBootstrapErrorLevel {
    pub fn is_error(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// Outcome of a workflow bootstrap attempt for a specific path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowBootstrapOutcome {
    Loaded,
    Skipped,
    MissingDirectory,
}

/// Per-path workflow bootstrap event in the order it occurred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowBootstrapEvent {
    pub path: PathBuf,
    pub outcome: WorkflowBootstrapOutcome,
}

/// A workflow bootstrap error with the originating path and severity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowBootstrapError {
    pub path: PathBuf,
    pub message: String,
    pub level: WorkflowBootstrapErrorLevel,
}

/// Observable workflow bootstrap summary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowBootstrapResult {
    pub loaded: usize,
    pub skipped: usize,
    pub errors: Vec<WorkflowBootstrapError>,
    pub events: Vec<WorkflowBootstrapEvent>,
}

impl WorkflowBootstrapResult {
    fn record_loaded(&mut self, path: PathBuf) {
        self.loaded += 1;
        self.events.push(WorkflowBootstrapEvent {
            path,
            outcome: WorkflowBootstrapOutcome::Loaded,
        });
    }

    fn record_missing_directory(&mut self, path: PathBuf) {
        self.events.push(WorkflowBootstrapEvent {
            path,
            outcome: WorkflowBootstrapOutcome::MissingDirectory,
        });
    }

    fn record_skipped(
        &mut self,
        path: PathBuf,
        message: impl Into<String>,
        level: WorkflowBootstrapErrorLevel,
    ) {
        let message = message.into();
        self.skipped += 1;
        self.errors.push(WorkflowBootstrapError {
            path: path.clone(),
            message,
            level,
        });
        self.events.push(WorkflowBootstrapEvent {
            path,
            outcome: WorkflowBootstrapOutcome::Skipped,
        });
    }
}

struct WorkflowLoadReport {
    workflows: Vec<Workflow>,
    result: WorkflowBootstrapResult,
}

/// Runtime projection for a workflow definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRuntimeStatus {
    pub workflow_id: WorkflowId,
    pub loaded: bool,
    pub healthy: bool,
    pub active_runs: usize,
    pub waiting_runs: usize,
    pub last_run_at: Option<DateTime<Utc>>,
}

/// Canonical file-backed storage for workflow definitions.
#[derive(Debug, Clone)]
pub(crate) struct WorkflowDefinitionStore {
    dir: PathBuf,
}

impl WorkflowDefinitionStore {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn workflow_path(&self, id: WorkflowId) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn temp_path(&self, id: WorkflowId) -> PathBuf {
        self.dir.join(format!("{id}.json.tmp"))
    }

    pub(crate) fn persist(&self, workflow: &Workflow) -> OpenFangResult<()> {
        std::fs::create_dir_all(&self.dir).map_err(|error| {
            OpenFangError::Internal(format!(
                "Failed to create workflow definitions directory '{}': {error}",
                self.dir.display()
            ))
        })?;

        let payload = serde_json::to_string_pretty(workflow).map_err(|error| {
            OpenFangError::Serialization(format!(
                "Failed to serialize workflow definition {}: {error}",
                workflow.id
            ))
        })?;
        let tmp_path = self.temp_path(workflow.id);
        let workflow_path = self.workflow_path(workflow.id);

        std::fs::write(&tmp_path, payload.as_bytes()).map_err(|error| {
            OpenFangError::Internal(format!(
                "Failed to write workflow definition temp file '{}': {error}",
                tmp_path.display()
            ))
        })?;

        if let Err(error) = std::fs::rename(&tmp_path, &workflow_path) {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(OpenFangError::Internal(format!(
                "Failed to replace workflow definition file '{}': {error}",
                workflow_path.display()
            )));
        }

        let persisted = std::fs::read_to_string(&workflow_path).map_err(|error| {
            OpenFangError::Internal(format!(
                "Failed to read back workflow definition '{}': {error}",
                workflow_path.display()
            ))
        })?;
        let reloaded = serde_json::from_str::<Workflow>(&persisted).map_err(|error| {
            OpenFangError::Serialization(format!(
                "Failed to deserialize persisted workflow definition '{}': {error}",
                workflow_path.display()
            ))
        })?;

        if reloaded != *workflow {
            return Err(OpenFangError::Internal(format!(
                "Persisted workflow definition '{}' did not round-trip cleanly",
                workflow_path.display()
            )));
        }

        Ok(())
    }

    pub(crate) fn delete(&self, id: WorkflowId) -> OpenFangResult<bool> {
        let workflow_path = self.workflow_path(id);
        match std::fs::remove_file(&workflow_path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(OpenFangError::Internal(format!(
                "Failed to remove workflow definition '{}': {error}",
                workflow_path.display()
            ))),
        }
    }

    fn supported_definition_file(path: &Path) -> bool {
        matches!(
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.to_ascii_lowercase()),
            Some(ext) if ext == "json" || ext == "toml"
        )
    }

    fn deserialize_workflow(
        &self,
        workflow_path: &Path,
        content: &str,
    ) -> Result<Workflow, String> {
        match workflow_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref()
        {
            Some("toml") => toml::from_str::<Workflow>(content)
                .map_err(|error| format!("Invalid workflow definition TOML: {error}")),
            _ => serde_json::from_str::<Workflow>(content)
                .map_err(|error| format!("Invalid workflow definition JSON: {error}")),
        }
    }

    fn load_all(&self) -> WorkflowLoadReport {
        let mut result = WorkflowBootstrapResult::default();
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                info!(
                    path = ?self.dir,
                    "Workflow definitions directory does not exist yet; skipping bootstrap"
                );
                result.record_missing_directory(self.dir.clone());
                return WorkflowLoadReport {
                    workflows: Vec::new(),
                    result,
                };
            }
            Err(error) => {
                error!(
                    path = ?self.dir,
                    error = %error,
                    "Failed to read workflow definitions directory"
                );
                result.record_skipped(
                    self.dir.clone(),
                    format!(
                        "Failed to read workflows directory '{}': {error}",
                        self.dir.display()
                    ),
                    WorkflowBootstrapErrorLevel::Error,
                );
                return WorkflowLoadReport {
                    workflows: Vec::new(),
                    result,
                };
            }
        };

        let mut workflow_files = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let path = entry.path();
                    if Self::supported_definition_file(&path) {
                        workflow_files.push(path);
                    }
                }
                Err(error) => {
                    error!(
                        path = ?self.dir,
                        error = %error,
                        "Failed to inspect workflow directory entry"
                    );
                    result.record_skipped(
                        self.dir.clone(),
                        format!(
                            "Failed to inspect workflow directory entry in '{}': {error}",
                            self.dir.display()
                        ),
                        WorkflowBootstrapErrorLevel::Error,
                    );
                }
            }
        }
        workflow_files.sort();

        let mut seen_ids = HashSet::with_capacity(workflow_files.len());
        let mut workflows = Vec::with_capacity(workflow_files.len());

        for workflow_path in workflow_files {
            let content = match std::fs::read_to_string(&workflow_path) {
                Ok(content) => content,
                Err(error) => {
                    error!(
                        path = ?workflow_path,
                        error = %error,
                        "Failed to read workflow definition file"
                    );
                    result.record_skipped(
                        workflow_path.clone(),
                        format!(
                            "Failed to read workflow definition file '{}': {error}",
                            workflow_path.display()
                        ),
                        WorkflowBootstrapErrorLevel::Error,
                    );
                    continue;
                }
            };

            match self.deserialize_workflow(&workflow_path, &content) {
                Ok(workflow) => {
                    if !seen_ids.insert(workflow.id) {
                        warn!(
                            path = ?workflow_path,
                            workflow_id = %workflow.id,
                            "Skipped duplicate workflow definition while loading from disk"
                        );
                        result.record_skipped(
                            workflow_path.clone(),
                            format!(
                                "Skipped duplicate workflow definition for workflow {}",
                                workflow.id
                            ),
                            WorkflowBootstrapErrorLevel::Warn,
                        );
                        continue;
                    }

                    info!(
                        path = ?workflow_path,
                        workflow_id = %workflow.id,
                        "Loaded workflow definition during bootstrap"
                    );
                    result.record_loaded(workflow_path.clone());
                    workflows.push(workflow);
                }
                Err(message) => {
                    warn!(
                        path = ?workflow_path,
                        error = %message,
                        "Invalid workflow definition, skipping"
                    );
                    result.record_skipped(
                        workflow_path.clone(),
                        message,
                        WorkflowBootstrapErrorLevel::Warn,
                    );
                }
            }
        }

        WorkflowLoadReport { workflows, result }
    }
}

#[derive(Debug, thiserror::Error)]
enum TransitionError {
    #[error(transparent)]
    Storage(#[from] WorkflowStoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("workflow run '{run_id}' was not found in durable storage")]
    RunNotFound { run_id: String },
    #[error("invalid workflow transition for run '{run_id}': {from} -> {to}")]
    InvalidStatusTransition {
        run_id: String,
        from: String,
        to: String,
    },
    #[error("failed to parse stored timestamp '{value}': {source}")]
    InvalidTimestamp {
        value: String,
        #[source]
        source: chrono::ParseError,
    },
    #[error("failed to parse stored workflow identifier '{value}': {source}")]
    InvalidWorkflowId {
        value: String,
        #[source]
        source: uuid::Error,
    },
    #[error("failed to parse stored workflow run identifier '{value}': {source}")]
    InvalidRunId {
        value: String,
        #[source]
        source: uuid::Error,
    },
    #[error("workflow durability worker failed: {0}")]
    Join(String),
}

fn transition_error_to_string(error: TransitionError) -> String {
    error.to_string()
}

fn workflow_state_from_status(status: DurableWorkflowRunStatus) -> WorkflowRunState {
    match status {
        DurableWorkflowRunStatus::Pending => WorkflowRunState::Pending,
        DurableWorkflowRunStatus::Running => WorkflowRunState::Running,
        DurableWorkflowRunStatus::WaitingSignal => WorkflowRunState::WaitingSignal,
        DurableWorkflowRunStatus::WaitingHitl => WorkflowRunState::WaitingHitl,
        DurableWorkflowRunStatus::Paused => WorkflowRunState::Paused,
        DurableWorkflowRunStatus::Completed => WorkflowRunState::Completed,
        DurableWorkflowRunStatus::Failed => WorkflowRunState::Failed,
        DurableWorkflowRunStatus::Cancelled => WorkflowRunState::Cancelled,
    }
}

fn parse_rfc3339_utc(value: &str) -> Result<DateTime<Utc>, TransitionError> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|source| TransitionError::InvalidTimestamp {
            value: value.to_string(),
            source,
        })
}

fn normalize_workflow_input_json(input: &str) -> Result<String, OpenFangError> {
    match serde_json::from_str::<JsonValue>(input) {
        Ok(value) => serde_json::to_string(&value).map_err(|error| {
            OpenFangError::Serialization(format!(
                "failed to serialize workflow input for durable storage: {error}"
            ))
        }),
        Err(_) => serde_json::to_string(input).map_err(|error| {
            OpenFangError::Serialization(format!(
                "failed to wrap workflow input as JSON string: {error}"
            ))
        }),
    }
}

fn cache_input_from_json(input_json: &str) -> String {
    match serde_json::from_str::<JsonValue>(input_json) {
        Ok(JsonValue::String(value)) => value,
        Ok(other) => other.to_string(),
        Err(_) => input_json.to_string(),
    }
}

fn error_message_from_json(error_json: Option<&str>) -> Option<String> {
    error_json.and_then(|payload| {
        serde_json::from_str::<JsonValue>(payload)
            .ok()
            .and_then(|value| {
                value
                    .get("message")
                    .and_then(JsonValue::as_str)
                    .map(ToOwned::to_owned)
            })
            .or_else(|| Some(payload.to_string()))
    })
}

fn parse_workflow_vars(vars_json: &str) -> Result<HashMap<String, String>, String> {
    serde_json::from_str(vars_json)
        .map_err(|error| format!("Failed to parse durable workflow vars JSON: {error}"))
}

fn checkpoint_data_json(value: JsonValue) -> Result<String, TransitionError> {
    serde_json::to_string(&value).map_err(Into::into)
}

fn control_action_checkpoint_data(actor_source: &str) -> Result<String, TransitionError> {
    checkpoint_data_json(serde_json::json!({
        "actor_source": actor_source,
    }))
}

fn checkpoint_output_summary(output: &str) -> String {
    const MAX_CHARS: usize = 240;
    output.chars().take(MAX_CHARS).collect()
}

fn in_memory_workflow_stores() -> WorkflowStoreSet {
    let conn =
        Connection::open_in_memory().expect("workflow engine should open in-memory compozy.db");
    conn.execute_batch(WORKFLOW_RUN_CORE_MIGRATION_SQL)
        .expect("workflow engine should apply workflow_run schema");
    conn.execute_batch(WORKFLOW_CHECKPOINT_MIGRATION_SQL)
        .expect("workflow engine should apply workflow_checkpoint schema");
    conn.execute_batch(WORKFLOW_SIGNAL_MIGRATION_SQL)
        .expect("workflow engine should apply workflow_signal schema");
    conn.execute_batch(WORKFLOW_RUNTIME_DURABILITY_MIGRATION_SQL)
        .expect("workflow engine should apply workflow durability migration");
    conn.execute_batch(WORKFLOW_SIGNAL_WAITING_STATE_MIGRATION_SQL)
        .expect("workflow engine should apply workflow signal waiting-state migration");
    conn.execute_batch(WORKFLOW_RUN_CONTROL_PLANE_MIGRATION_SQL)
        .expect("workflow engine should apply workflow control-plane migration");
    WorkflowStoreSet::new(Arc::new(StdMutex::new(conn)))
}

#[derive(Debug, Clone)]
pub(crate) struct SignalResumeContext {
    run_id: WorkflowRunId,
    workflow: WorkflowIr,
    start_index: usize,
    input: String,
    variables: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct SignalSubmissionOutcome {
    pub signal: openfang_memory::WorkflowSignalRecord,
    pub resume: Option<SignalResumeContext>,
}

#[derive(Debug, Clone)]
struct ExecutionContext {
    run_id: WorkflowRunId,
    workflow: WorkflowIr,
    start_index: usize,
    input: String,
    variables: HashMap<String, String>,
    start_run: bool,
}

enum ExecutionOutcome {
    Completed(String),
    Parked(String),
}

#[derive(Clone)]
struct TransitionWriter {
    workflow_stores: WorkflowStoreSet,
    runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>,
    workflows: Arc<RwLock<HashMap<WorkflowId, Workflow>>>,
}

impl TransitionWriter {
    fn new(
        workflow_stores: WorkflowStoreSet,
        runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>,
        workflows: Arc<RwLock<HashMap<WorkflowId, Workflow>>>,
    ) -> Self {
        Self {
            workflow_stores,
            runs,
            workflows,
        }
    }

    async fn load_run(&self, run_id: WorkflowRunId) -> Result<WorkflowRunRecord, TransitionError> {
        let repository = self.workflow_stores.workflow_run.clone();
        let run_id_text = run_id.to_string();
        let lookup_run_id = run_id_text.clone();
        let maybe_record =
            tokio::task::spawn_blocking(move || repository.find_by_id(&lookup_run_id))
                .await
                .map_err(|error| TransitionError::Join(error.to_string()))??;

        maybe_record.ok_or(TransitionError::RunNotFound {
            run_id: run_id_text,
        })
    }

    async fn persist_transition(
        &self,
        current: WorkflowRunRecord,
        next: WorkflowRunRecord,
        checkpoint: WorkflowCheckpointRecord,
    ) -> Result<(), TransitionError> {
        let repository = self.workflow_stores.workflow_run.clone();
        tokio::task::spawn_blocking(move || {
            repository.persist_transition(&current, &next, &checkpoint)
        })
        .await
        .map_err(|error| TransitionError::Join(error.to_string()))??;
        Ok(())
    }

    async fn sync_cache_from_record(
        &self,
        record: &WorkflowRunRecord,
        seed_run: Option<WorkflowRun>,
    ) -> Result<(), TransitionError> {
        let run_id = WorkflowRunId(Uuid::parse_str(&record.run_id).map_err(|source| {
            TransitionError::InvalidRunId {
                value: record.run_id.clone(),
                source,
            }
        })?);
        let workflow_id = WorkflowId(Uuid::parse_str(&record.workflow_id).map_err(|source| {
            TransitionError::InvalidWorkflowId {
                value: record.workflow_id.clone(),
                source,
            }
        })?);
        let started_at = parse_rfc3339_utc(&record.started_at)?;
        let updated_at = parse_rfc3339_utc(&record.updated_at)?;
        let completed_at = match record.completed_at.as_deref() {
            Some(value) => Some(parse_rfc3339_utc(value)?),
            None => None,
        };
        let seed_input = seed_run.as_ref().map(|run| run.input.clone());
        let seed_results = seed_run
            .as_ref()
            .map(|run| run.step_results.clone())
            .unwrap_or_default();
        let seed_output = seed_run.as_ref().and_then(|run| run.output.clone());
        let workflow_name = if let Some(run) = seed_run.as_ref() {
            run.workflow_name.clone()
        } else {
            self.workflows
                .read()
                .await
                .get(&workflow_id)
                .map(|workflow| workflow.name.clone())
                .unwrap_or_else(|| record.workflow_id.clone())
        };

        let mut runs = self.runs.write().await;
        let entry = runs.entry(run_id).or_insert_with(|| WorkflowRun {
            id: run_id,
            workflow_id,
            workflow_version: record.workflow_version.clone(),
            workflow_name,
            input: seed_input.unwrap_or_else(|| cache_input_from_json(&record.input_json)),
            vars_json: record.vars_json.clone(),
            current_step_id: record.current_step_id.clone(),
            waiting_kind: record.waiting_kind.clone(),
            waiting_ref: record.waiting_ref.clone(),
            active_dispatch_id: record.active_dispatch_id.clone(),
            active_hitl_request_id: record.active_hitl_request_id.clone(),
            labels_json: record.labels_json.clone(),
            metadata_json: record.metadata_json.clone(),
            state: workflow_state_from_status(record.status),
            step_results: seed_results,
            output: seed_output,
            error: error_message_from_json(record.error_json.as_deref()),
            started_at,
            updated_at,
            completed_at,
        });

        entry.workflow_id = workflow_id;
        entry.workflow_version = record.workflow_version.clone();
        entry.vars_json = record.vars_json.clone();
        entry.current_step_id = record.current_step_id.clone();
        entry.waiting_kind = record.waiting_kind.clone();
        entry.waiting_ref = record.waiting_ref.clone();
        entry.active_dispatch_id = record.active_dispatch_id.clone();
        entry.active_hitl_request_id = record.active_hitl_request_id.clone();
        entry.labels_json = record.labels_json.clone();
        entry.metadata_json = record.metadata_json.clone();
        entry.state = workflow_state_from_status(record.status);
        entry.error = error_message_from_json(record.error_json.as_deref());
        entry.started_at = started_at;
        entry.updated_at = updated_at;
        entry.completed_at = completed_at;

        Ok(())
    }

    async fn record_run_created(&self, seed_run: WorkflowRun) -> Result<(), TransitionError> {
        let current = self.load_run(seed_run.id).await?;
        let mut next = current.clone();
        next.updated_at = now_timestamp();

        let input = serde_json::from_str::<JsonValue>(&current.input_json)?;
        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: None,
            kind: DurableCheckpointKind::RunCreated,
            data_json: checkpoint_data_json(serde_json::json!({
                "workflow_id": current.workflow_id,
                "workflow_version": current.workflow_version,
                "input": input,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, Some(seed_run)).await
    }

    async fn record_run_started(
        &self,
        run_id: WorkflowRunId,
        initial_step_id: Option<&str>,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Pending {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Running.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::Running;
        next.current_step_id = initial_step_id.map(ToOwned::to_owned);
        next.waiting_kind = None;
        next.waiting_ref = None;
        next.updated_at = now_timestamp();

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: initial_step_id.map(ToOwned::to_owned),
            kind: DurableCheckpointKind::RunStarted,
            data_json: checkpoint_data_json(serde_json::json!({
                "initial_step_id": initial_step_id,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_step_started(
        &self,
        run_id: WorkflowRunId,
        step: &WorkflowIrStep,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Running {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Running.to_string(),
            });
        }

        let mut next = current.clone();
        next.current_step_id = Some(step.id.clone());
        next.updated_at = now_timestamp();

        let mut payload = serde_json::Map::new();
        payload.insert("step_id".to_string(), JsonValue::String(step.id.clone()));
        payload.insert(
            "kind".to_string(),
            JsonValue::String(WorkflowEngine::step_kind_name(&step.kind).to_string()),
        );
        match &step.kind {
            WorkflowIrStepKind::Agent { agent } => {
                payload.insert("agent".to_string(), JsonValue::String(agent.clone()));
            }
            WorkflowIrStepKind::Primitive { primitive } => {
                payload.insert(
                    "primitive".to_string(),
                    JsonValue::String(primitive.clone()),
                );
            }
            WorkflowIrStepKind::Workflow { workflow } => {
                payload.insert("workflow".to_string(), JsonValue::String(workflow.clone()));
            }
            WorkflowIrStepKind::WaitSignal { signal_name } => {
                payload.insert(
                    "signal_name".to_string(),
                    JsonValue::String(signal_name.clone()),
                );
            }
            WorkflowIrStepKind::StartLooper {
                task_ref,
                task_id_binding,
            } => {
                if let Some(task_ref) = task_ref.as_ref() {
                    payload.insert("task_ref".to_string(), JsonValue::String(task_ref.clone()));
                }
                if let Some(task_id_binding) = task_id_binding.as_ref() {
                    payload.insert(
                        "task_id_binding".to_string(),
                        JsonValue::String(task_id_binding.clone()),
                    );
                }
            }
            WorkflowIrStepKind::EmitEvent {
                event,
                payload_template,
            } => {
                payload.insert("event".to_string(), JsonValue::String(event.clone()));
                if let Some(payload_template) = payload_template.as_ref() {
                    payload.insert(
                        "payload_template".to_string(),
                        JsonValue::String(payload_template.clone()),
                    );
                }
            }
            WorkflowIrStepKind::Collect | WorkflowIrStepKind::Noop => {}
        }

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: Some(step.id.clone()),
            kind: DurableCheckpointKind::StepStarted,
            data_json: checkpoint_data_json(JsonValue::Object(payload))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_step_completed(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        save_as: Option<&str>,
        output_summary: &str,
        vars_json: String,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Running {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Running.to_string(),
            });
        }

        let mut next = current.clone();
        next.current_step_id = Some(step_id.to_string());
        next.vars_json = vars_json;
        next.error_json = None;
        next.updated_at = now_timestamp();

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: Some(step_id.to_string()),
            kind: DurableCheckpointKind::StepCompleted,
            data_json: checkpoint_data_json(serde_json::json!({
                "step_id": step_id,
                "save_as": save_as,
                "output_summary": output_summary,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_step_failed(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        error: &str,
        attempt: u32,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Running {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Running.to_string(),
            });
        }

        let mut next = current.clone();
        next.current_step_id = Some(step_id.to_string());
        next.error_json = Some(
            serde_json::json!({
                "message": error,
                "step_id": step_id,
                "attempt": attempt,
            })
            .to_string(),
        );
        next.updated_at = now_timestamp();

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: Some(step_id.to_string()),
            kind: DurableCheckpointKind::StepFailed,
            data_json: checkpoint_data_json(serde_json::json!({
                "step_id": step_id,
                "error": error,
                "attempt": attempt,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_step_skipped(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        reason: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Running {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Running.to_string(),
            });
        }

        let mut next = current.clone();
        next.current_step_id = Some(step_id.to_string());
        next.updated_at = now_timestamp();

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: Some(step_id.to_string()),
            kind: DurableCheckpointKind::StepSkipped,
            data_json: checkpoint_data_json(serde_json::json!({
                "step_id": step_id,
                "reason": reason,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_waiting_for_signal(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        signal_name: &str,
        resume_input: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Running {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::WaitingSignal.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::WaitingSignal;
        next.current_step_id = Some(step_id.to_string());
        next.waiting_kind = Some("signal".to_string());
        next.waiting_ref = Some(signal_name.to_string());
        next.updated_at = now_timestamp();

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: Some(step_id.to_string()),
            kind: DurableCheckpointKind::WaitingSignal,
            data_json: checkpoint_data_json(serde_json::json!({
                "signal_name": signal_name,
                "resume_input": resume_input,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_signal_consumed(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        next_step_id: Option<&str>,
        signal_id: &str,
        signal_name: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if !matches!(
            current.status,
            DurableWorkflowRunStatus::Running | DurableWorkflowRunStatus::WaitingSignal
        ) {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Running.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::Running;
        next.current_step_id = next_step_id.map(ToOwned::to_owned);
        next.waiting_kind = None;
        next.waiting_ref = None;
        next.updated_at = now_timestamp();

        let consumed_checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: Some(step_id.to_string()),
            kind: DurableCheckpointKind::SignalConsumed,
            data_json: checkpoint_data_json(serde_json::json!({
                "signal_id": signal_id,
                "signal_name": signal_name,
            }))?,
            created_at: next.updated_at.clone(),
        };
        let resumed_checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: Some(step_id.to_string()),
            kind: DurableCheckpointKind::RunResumedFromSignal,
            data_json: checkpoint_data_json(serde_json::json!({
                "signal_id": signal_id,
                "signal_name": signal_name,
            }))?,
            created_at: next.updated_at.clone(),
        };

        let repository = self.workflow_stores.workflow_run.clone();
        let signal_id = signal_id.to_string();
        let consumed_at = next.updated_at.clone();
        let current_record = current.clone();
        let next_record = next.clone();
        let consumed_checkpoint_record = consumed_checkpoint.clone();
        let resumed_checkpoint_record = resumed_checkpoint.clone();
        tokio::task::spawn_blocking(move || {
            repository.persist_signal_resume(
                &current_record,
                &next_record,
                &consumed_checkpoint_record,
                &resumed_checkpoint_record,
                &signal_id,
                &consumed_at,
            )
        })
        .await
        .map_err(|error| TransitionError::Join(error.to_string()))??;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_run_completed(
        &self,
        run_id: WorkflowRunId,
        final_output_summary: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Running {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Completed.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::Completed;
        next.updated_at = now_timestamp();
        next.completed_at = Some(next.updated_at.clone());
        next.waiting_kind = None;
        next.waiting_ref = None;
        next.error_json = None;

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: current.current_step_id.clone(),
            kind: DurableCheckpointKind::RunCompleted,
            data_json: checkpoint_data_json(serde_json::json!({
                "final_output_summary": final_output_summary,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_run_failed(
        &self,
        run_id: WorkflowRunId,
        failing_step_id: Option<&str>,
        error: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if !matches!(
            current.status,
            DurableWorkflowRunStatus::Running | DurableWorkflowRunStatus::WaitingSignal
        ) {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Failed.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::Failed;
        next.current_step_id = failing_step_id
            .map(ToOwned::to_owned)
            .or_else(|| current.current_step_id.clone());
        next.error_json = Some(
            serde_json::json!({
                "message": error,
                "step_id": failing_step_id,
            })
            .to_string(),
        );
        next.updated_at = now_timestamp();
        next.completed_at = Some(next.updated_at.clone());
        next.waiting_kind = None;
        next.waiting_ref = None;

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: failing_step_id.map(ToOwned::to_owned),
            kind: DurableCheckpointKind::RunFailed,
            data_json: checkpoint_data_json(serde_json::json!({
                "error": error,
                "failing_step_id": failing_step_id,
            }))?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_run_paused(
        &self,
        run_id: WorkflowRunId,
        actor_source: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if !matches!(
            current.status,
            DurableWorkflowRunStatus::Running | DurableWorkflowRunStatus::WaitingSignal
        ) {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Paused.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::Paused;
        next.updated_at = now_timestamp();
        next.waiting_kind = None;
        next.waiting_ref = None;

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: current.current_step_id.clone(),
            kind: DurableCheckpointKind::RunPaused,
            data_json: control_action_checkpoint_data(actor_source)?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_run_resumed(
        &self,
        run_id: WorkflowRunId,
        actor_source: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status != DurableWorkflowRunStatus::Paused {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Running.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::Running;
        next.updated_at = now_timestamp();
        next.waiting_kind = None;
        next.waiting_ref = None;

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: current.current_step_id.clone(),
            kind: DurableCheckpointKind::RunResumed,
            data_json: control_action_checkpoint_data(actor_source)?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }

    async fn record_run_cancelled(
        &self,
        run_id: WorkflowRunId,
        actor_source: &str,
    ) -> Result<(), TransitionError> {
        let current = self.load_run(run_id).await?;
        if current.status.is_terminal() {
            return Err(TransitionError::InvalidStatusTransition {
                run_id: current.run_id.clone(),
                from: current.status.to_string(),
                to: DurableWorkflowRunStatus::Cancelled.to_string(),
            });
        }

        let mut next = current.clone();
        next.status = DurableWorkflowRunStatus::Cancelled;
        next.updated_at = now_timestamp();
        next.completed_at = Some(next.updated_at.clone());
        next.waiting_kind = None;
        next.waiting_ref = None;

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: current.current_step_id.clone(),
            kind: DurableCheckpointKind::RunCancelled,
            data_json: control_action_checkpoint_data(actor_source)?,
            created_at: next.updated_at.clone(),
        };

        self.persist_transition(current, next.clone(), checkpoint)
            .await?;
        self.sync_cache_from_record(&next, None).await
    }
}

/// The workflow engine — manages definitions and executes pipeline runs.
pub struct WorkflowEngine {
    /// Registered workflow definitions.
    workflows: Arc<RwLock<HashMap<WorkflowId, Workflow>>>,
    /// Registered Workflow v2 definitions.
    workflow_v2_definitions: Arc<RwLock<BTreeMap<String, WorkflowV2Definition>>>,
    /// Cached compiled IR projections for Workflow v2 definitions.
    compiled_workflows: Arc<RwLock<BTreeMap<String, WorkflowIr>>>,
    /// Known workflow primitives used during validation.
    known_primitives: Arc<RwLock<BTreeSet<String>>>,
    /// Active and completed workflow runs.
    runs: Arc<RwLock<HashMap<WorkflowRunId, WorkflowRun>>>,
    /// Canonical file-backed workflow definition storage.
    definition_store: WorkflowDefinitionStore,
    /// Serializes definition mutations and reloads so memory and disk stay coherent.
    definition_mutation_lock: Arc<Mutex<()>>,
    /// Readiness state for the workflow registry.
    readiness: Arc<AtomicU8>,
    /// Durable workflow stores.
    workflow_stores: WorkflowStoreSet,
}

impl WorkflowEngine {
    /// Create a new workflow engine.
    pub fn new() -> Self {
        Self::with_definitions_dir(
            std::env::temp_dir().join(format!("openfang-workflows-{}", Uuid::new_v4())),
        )
    }

    /// Create a new workflow engine with an explicit definitions directory.
    pub fn with_definitions_dir(workflows_dir: PathBuf) -> Self {
        Self::build(workflows_dir, None)
    }

    /// Create a new workflow engine with an explicit definitions directory and
    /// typed durable workflow stores.
    pub fn with_definitions_dir_and_stores(
        workflows_dir: PathBuf,
        workflow_stores: WorkflowStoreSet,
    ) -> Self {
        Self::build(workflows_dir, Some(workflow_stores))
    }

    fn build(workflows_dir: PathBuf, workflow_stores: Option<WorkflowStoreSet>) -> Self {
        Self {
            workflows: Arc::new(RwLock::new(HashMap::new())),
            workflow_v2_definitions: Arc::new(RwLock::new(BTreeMap::new())),
            compiled_workflows: Arc::new(RwLock::new(BTreeMap::new())),
            known_primitives: Arc::new(RwLock::new(BTreeSet::new())),
            runs: Arc::new(RwLock::new(HashMap::new())),
            definition_store: WorkflowDefinitionStore::new(workflows_dir),
            definition_mutation_lock: Arc::new(Mutex::new(())),
            readiness: Arc::new(AtomicU8::new(
                WorkflowRegistryReadiness::Bootstrapping as u8,
            )),
            workflow_stores: workflow_stores.unwrap_or_else(in_memory_workflow_stores),
        }
    }

    fn set_readiness(&self, readiness: WorkflowRegistryReadiness) {
        self.readiness.store(readiness as u8, Ordering::Release);
    }

    pub fn readiness(&self) -> WorkflowRegistryReadiness {
        WorkflowRegistryReadiness::from_stored(self.readiness.load(Ordering::Acquire))
    }

    pub fn is_ready(&self) -> bool {
        self.readiness() == WorkflowRegistryReadiness::Ready
    }

    /// Replaces the known primitive registry used by Workflow v2 validation.
    pub async fn set_known_primitives<I, S>(&self, primitives: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let mut known_primitives = self.known_primitives.write().await;
        *known_primitives = primitives.into_iter().map(Into::into).collect();
    }

    /// Builds the current workflow compile registry from known in-memory
    /// definitions plus any additional agent and workflow references supplied
    /// by the caller.
    pub async fn build_compile_registry<I, S, J, T>(
        &self,
        available_agents: I,
        additional_workflows: J,
    ) -> WorkflowCompileRegistry
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
        J: IntoIterator<Item = T>,
        T: Into<String>,
    {
        let mut registry = WorkflowCompileRegistry::new();
        registry.set_agents(available_agents);
        registry.set_primitives(
            self.known_primitives
                .read()
                .await
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>(),
        );
        registry.set_workflows(
            self.workflow_v2_definitions
                .read()
                .await
                .keys()
                .cloned()
                .chain(additional_workflows.into_iter().map(Into::into))
                .collect::<BTreeSet<_>>(),
        );
        registry
    }

    /// Register a Workflow v2 definition and cache its compiled IR.
    pub async fn register_workflow_v2_definition(
        &self,
        definition: WorkflowV2Definition,
        available_agents: impl IntoIterator<Item = String>,
    ) -> Result<(), WorkflowCompileError> {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let registry = self
            .build_compile_registry(available_agents, std::iter::once(definition.id.clone()))
            .await;

        let compiled = compile_workflow_definition(&definition, &registry)?;

        self.workflow_v2_definitions
            .write()
            .await
            .insert(definition.id.clone(), definition);
        self.compiled_workflows
            .write()
            .await
            .insert(compiled.workflow_id.clone(), compiled);

        Ok(())
    }

    /// Returns a Workflow v2 definition by ID.
    pub async fn get_workflow_v2_definition(
        &self,
        workflow_id: &str,
    ) -> Option<WorkflowV2Definition> {
        self.workflow_v2_definitions
            .read()
            .await
            .get(workflow_id)
            .cloned()
    }

    /// Returns the cached compiled IR for a Workflow v2 definition.
    pub async fn get_compiled_workflow(&self, workflow_id: &str) -> Option<WorkflowIr> {
        self.compiled_workflows
            .read()
            .await
            .get(workflow_id)
            .cloned()
    }

    /// Register a new workflow definition.
    pub async fn register(&self, workflow: Workflow) -> OpenFangResult<WorkflowId> {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let id = workflow.id;
        let mut workflows = self.workflows.write().await;
        self.definition_store.persist(&workflow)?;
        workflows.insert(id, workflow);
        info!(workflow_id = %id, "Workflow registered");
        Ok(id)
    }

    /// List all registered workflows.
    pub async fn list_workflows(&self) -> Vec<Workflow> {
        self.workflows.read().await.values().cloned().collect()
    }

    /// Get a specific workflow by ID.
    pub async fn get_workflow(&self, id: WorkflowId) -> Option<Workflow> {
        self.workflows.read().await.get(&id).cloned()
    }

    /// Remove a workflow definition.
    pub async fn remove_workflow(&self, id: WorkflowId) -> OpenFangResult<bool> {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let mut workflows = self.workflows.write().await;
        if !workflows.contains_key(&id) {
            return Ok(false);
        }

        let file_removed = self.definition_store.delete(id)?;
        if !file_removed {
            warn!(
                workflow_id = %id,
                "Workflow definition file was already missing during delete; removing in-memory entry"
            );
        }

        if workflows.remove(&id).is_none() {
            return Err(OpenFangError::Internal(format!(
                "Workflow {id} disappeared from the in-memory registry during delete"
            )));
        }

        Ok(true)
    }

    /// Update an existing workflow definition.
    ///
    /// Preserves the original `id` and `created_at`. Replaces `name`,
    /// `description`, and `steps`. Returns `true` if the workflow was
    /// found and updated.
    pub async fn update_workflow(&self, id: WorkflowId, updated: Workflow) -> OpenFangResult<bool> {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        let mut workflows = self.workflows.write().await;
        if let Some(existing) = workflows.get(&id).cloned() {
            let mut canonical = updated;
            canonical.id = id;
            canonical.created_at = existing.created_at;
            self.definition_store.persist(&canonical)?;
            workflows.insert(id, canonical);
            info!(workflow_id = %id, "Workflow updated");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Replace the in-memory workflow registry with the workflows currently on disk.
    pub(crate) async fn bootstrap_from_store(
        &self,
        store: WorkflowDefinitionStore,
    ) -> WorkflowBootstrapResult {
        let _mutation_guard = self.definition_mutation_lock.lock().await;
        self.set_readiness(WorkflowRegistryReadiness::Bootstrapping);
        let WorkflowLoadReport {
            workflows: loaded_workflows,
            result,
        } = store.load_all();
        let mut workflows = self.workflows.write().await;
        *workflows = loaded_workflows
            .into_iter()
            .map(|workflow| (workflow.id, workflow))
            .collect();
        drop(workflows);
        self.set_readiness(WorkflowRegistryReadiness::Ready);
        result
    }

    /// Maximum number of retained workflow runs. Oldest completed/failed
    /// runs are evicted when this limit is exceeded.
    const MAX_RETAINED_RUNS: usize = 200;

    fn transition_writer(&self) -> TransitionWriter {
        TransitionWriter::new(
            self.workflow_stores.clone(),
            Arc::clone(&self.runs),
            Arc::clone(&self.workflows),
        )
    }

    async fn evict_terminal_runs_from_cache(&self) {
        let mut runs = self.runs.write().await;
        if runs.len() <= Self::MAX_RETAINED_RUNS {
            return;
        }

        let mut evictable: Vec<(WorkflowRunId, DateTime<Utc>)> = runs
            .iter()
            .filter(|(_, run)| {
                matches!(
                    run.state,
                    WorkflowRunState::Completed
                        | WorkflowRunState::Failed
                        | WorkflowRunState::Cancelled
                )
            })
            .map(|(id, run)| (*id, run.started_at))
            .collect();

        evictable.sort_by_key(|(_, timestamp)| *timestamp);
        let to_remove = runs.len() - Self::MAX_RETAINED_RUNS;
        for (run_id, _) in evictable.into_iter().take(to_remove) {
            runs.remove(&run_id);
            debug!(run_id = %run_id, "Evicted workflow run from in-memory cache");
        }
    }

    pub async fn recover_durable_runs(&self) -> OpenFangResult<usize> {
        let repository = self.workflow_stores.workflow_run.clone();
        let recovered = tokio::task::spawn_blocking(move || repository.recover_running_runs())
            .await
            .map_err(|error| {
                OpenFangError::Internal(format!("Workflow recovery query task failed: {error}"))
            })?
            .map_err(OpenFangError::from)?;
        let recovered_count = recovered.len();
        let writer = self.transition_writer();
        let repository = writer.workflow_stores.workflow_run.clone();
        let records = tokio::task::spawn_blocking(move || repository.list_non_terminal())
            .await
            .map_err(|error| {
                OpenFangError::Internal(format!(
                    "Workflow recovery projection task failed: {error}"
                ))
            })?
            .map_err(OpenFangError::from)?;

        for record in records {
            writer
                .sync_cache_from_record(&record, None)
                .await
                .map_err(|error| OpenFangError::Internal(error.to_string()))?;
        }

        Ok(recovered_count)
    }

    pub async fn create_run_with_context(
        &self,
        workflow_id: WorkflowId,
        workflow_version: impl Into<String>,
        input: String,
        labels: Vec<String>,
        metadata: JsonValue,
    ) -> OpenFangResult<WorkflowRunId> {
        let workflow = self
            .workflows
            .read()
            .await
            .get(&workflow_id)
            .cloned()
            .ok_or_else(|| OpenFangError::Internal("Workflow definition not found".to_string()))?;
        let run_id = WorkflowRunId::new();
        let timestamp = now_timestamp();
        let workflow_version = workflow_version.into();
        let run_record = WorkflowRunRecord {
            run_id: run_id.to_string(),
            workflow_id: workflow_id.to_string(),
            workflow_version: workflow_version.clone(),
            status: DurableWorkflowRunStatus::Pending,
            input_json: normalize_workflow_input_json(&input)?,
            vars_json: "{}".to_string(),
            current_step_id: None,
            waiting_kind: None,
            waiting_ref: None,
            active_dispatch_id: None,
            active_hitl_request_id: None,
            labels_json: serde_json::to_string(&labels).map_err(|error| {
                OpenFangError::Serialization(format!(
                    "Failed to serialize workflow labels for durable state: {error}"
                ))
            })?,
            metadata_json: serde_json::to_string(&metadata).map_err(|error| {
                OpenFangError::Serialization(format!(
                    "Failed to serialize workflow metadata for durable state: {error}"
                ))
            })?,
            error_json: None,
            started_at: timestamp.clone(),
            updated_at: timestamp.clone(),
            completed_at: None,
        };

        let run = WorkflowRun {
            id: run_id,
            workflow_id,
            workflow_version,
            workflow_name: workflow.name,
            input,
            vars_json: "{}".to_string(),
            current_step_id: None,
            waiting_kind: None,
            waiting_ref: None,
            active_dispatch_id: None,
            active_hitl_request_id: None,
            labels_json: run_record.labels_json.clone(),
            metadata_json: run_record.metadata_json.clone(),
            state: WorkflowRunState::Pending,
            step_results: Vec::new(),
            output: None,
            error: None,
            started_at: parse_rfc3339_utc(&timestamp)
                .map_err(|error| OpenFangError::Internal(error.to_string()))?,
            updated_at: parse_rfc3339_utc(&timestamp)
                .map_err(|error| OpenFangError::Internal(error.to_string()))?,
            completed_at: None,
        };

        let writer = self.transition_writer();
        let repository = writer.workflow_stores.workflow_run.clone();
        let run_record_clone = run_record.clone();
        tokio::task::spawn_blocking(move || repository.insert_run(&run_record_clone))
            .await
            .map_err(|error| {
                OpenFangError::Internal(format!("Workflow run insert task failed: {error}"))
            })?
            .map_err(OpenFangError::from)?;
        writer
            .record_run_created(run)
            .await
            .map_err(|error| OpenFangError::Internal(error.to_string()))?;

        self.evict_terminal_runs_from_cache().await;
        Ok(run_id)
    }

    pub async fn create_run(
        &self,
        workflow_id: WorkflowId,
        input: String,
    ) -> OpenFangResult<WorkflowRunId> {
        self.create_run_with_context(
            workflow_id,
            "legacy",
            input,
            Vec::new(),
            serde_json::json!({}),
        )
        .await
    }

    /// Get the current state of a workflow run.
    pub async fn get_run(&self, run_id: WorkflowRunId) -> Option<WorkflowRun> {
        self.runs.read().await.get(&run_id).cloned()
    }

    /// List all workflow runs (optionally filtered by state).
    pub async fn list_runs(&self, state_filter: Option<&str>) -> Vec<WorkflowRun> {
        self.runs
            .read()
            .await
            .values()
            .filter(|r| {
                state_filter
                    .map(|f| match f {
                        "pending" => matches!(r.state, WorkflowRunState::Pending),
                        "running" => matches!(r.state, WorkflowRunState::Running),
                        "waiting_signal" => matches!(r.state, WorkflowRunState::WaitingSignal),
                        "waiting_hitl" => matches!(r.state, WorkflowRunState::WaitingHitl),
                        "paused" => matches!(r.state, WorkflowRunState::Paused),
                        "completed" => matches!(r.state, WorkflowRunState::Completed),
                        "failed" => matches!(r.state, WorkflowRunState::Failed),
                        "cancelled" => matches!(r.state, WorkflowRunState::Cancelled),
                        _ => true,
                    })
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// Clear the in-memory run projection cache.
    pub async fn clear_run_cache(&self) {
        self.runs.write().await.clear();
    }

    /// Durably park a running workflow at a `wait_signal` step.
    pub async fn record_waiting_for_signal_transition(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        signal_name: &str,
        resume_input: &str,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_waiting_for_signal(run_id, step_id, signal_name, resume_input)
            .await
            .map_err(transition_error_to_string)
    }

    /// Mark a durable workflow signal as consumed and resume the run in the
    /// canonical store and cache.
    pub async fn record_signal_consumed_transition(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        next_step_id: Option<&str>,
        signal_id: &str,
        signal_name: &str,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_signal_consumed(run_id, step_id, next_step_id, signal_id, signal_name)
            .await
            .map_err(transition_error_to_string)
    }

    /// Cancel a workflow run through the durable transition writer.
    pub async fn cancel_run(
        &self,
        run_id: WorkflowRunId,
        actor_source: &str,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_run_cancelled(run_id, actor_source)
            .await
            .map_err(transition_error_to_string)
    }

    /// Pause a workflow run through the durable transition writer.
    pub async fn pause_run(&self, run_id: WorkflowRunId, actor_source: &str) -> Result<(), String> {
        self.transition_writer()
            .record_run_paused(run_id, actor_source)
            .await
            .map_err(transition_error_to_string)
    }

    /// Resume a workflow run through the durable transition writer.
    pub async fn resume_run(
        &self,
        run_id: WorkflowRunId,
        actor_source: &str,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_run_resumed(run_id, actor_source)
            .await
            .map_err(transition_error_to_string)
    }

    async fn record_run_started_transition(
        &self,
        run_id: WorkflowRunId,
        initial_step_id: Option<&str>,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_run_started(run_id, initial_step_id)
            .await
            .map_err(transition_error_to_string)
    }

    async fn record_step_started_transition(
        &self,
        run_id: WorkflowRunId,
        step: &WorkflowIrStep,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_step_started(run_id, step)
            .await
            .map_err(transition_error_to_string)
    }

    async fn record_step_completed_transition(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        save_as: Option<&str>,
        output_summary: &str,
        vars_json: String,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_step_completed(run_id, step_id, save_as, output_summary, vars_json)
            .await
            .map_err(transition_error_to_string)
    }

    async fn record_step_failed_transition(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        error: &str,
        attempt: u32,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_step_failed(run_id, step_id, error, attempt)
            .await
            .map_err(transition_error_to_string)
    }

    async fn record_step_skipped_transition(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_step_skipped(run_id, step_id, reason)
            .await
            .map_err(transition_error_to_string)
    }

    async fn record_run_failed_transition(
        &self,
        run_id: WorkflowRunId,
        failing_step_id: Option<&str>,
        error: &str,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_run_failed(run_id, failing_step_id, error)
            .await
            .map_err(transition_error_to_string)
    }

    async fn record_run_completed_transition(
        &self,
        run_id: WorkflowRunId,
        final_output: &str,
    ) -> Result<(), String> {
        self.transition_writer()
            .record_run_completed(run_id, &checkpoint_output_summary(final_output))
            .await
            .map_err(transition_error_to_string)?;

        if let Some(run) = self.runs.write().await.get_mut(&run_id) {
            run.output = Some(final_output.to_string());
        }
        Ok(())
    }

    /// Return the runtime projection for a workflow definition after readiness.
    pub async fn runtime_status(&self, workflow_id: WorkflowId) -> Option<WorkflowRuntimeStatus> {
        let workflow = self.workflows.read().await.get(&workflow_id)?.clone();
        let runs = self.runs.read().await;
        let mut active_runs = 0usize;
        let mut waiting_runs = 0usize;
        let mut last_run_at = None;

        for run in runs.values().filter(|run| run.workflow_id == workflow.id) {
            match run.state {
                WorkflowRunState::Running => active_runs += 1,
                WorkflowRunState::Pending
                | WorkflowRunState::WaitingSignal
                | WorkflowRunState::WaitingHitl => waiting_runs += 1,
                WorkflowRunState::Paused => {}
                WorkflowRunState::Completed
                | WorkflowRunState::Failed
                | WorkflowRunState::Cancelled => {}
            }

            if last_run_at
                .map(|current| run.started_at > current)
                .unwrap_or(true)
            {
                last_run_at = Some(run.started_at);
            }
        }

        Some(WorkflowRuntimeStatus {
            workflow_id,
            loaded: true,
            healthy: true,
            active_runs,
            waiting_runs,
            last_run_at,
        })
    }

    fn any_contract() -> openfang_types::contract::ContractNode {
        serde_json::from_value(serde_json::json!({ "kind": "any" }))
            .expect("static any contract should deserialize")
    }

    pub(crate) fn legacy_workflow_to_ir(workflow: &Workflow) -> WorkflowIr {
        let mut steps = Vec::with_capacity(workflow.steps.len());
        let mut symbol_table = BTreeMap::new();

        for step in &workflow.steps {
            let flow = match &step.mode {
                StepMode::Sequential | StepMode::Collect => FlowBlock {
                    mode: WorkflowV2FlowMode::Sequential,
                },
                StepMode::FanOut => FlowBlock {
                    mode: WorkflowV2FlowMode::FanOut,
                },
                StepMode::Conditional { condition } => FlowBlock {
                    mode: WorkflowV2FlowMode::Conditional {
                        when: condition.clone(),
                    },
                },
                StepMode::Loop {
                    max_iterations,
                    until,
                } => FlowBlock {
                    mode: WorkflowV2FlowMode::Loop {
                        until: until.clone(),
                        max_iterations: *max_iterations,
                    },
                },
            };

            let kind = if matches!(step.mode, StepMode::Collect) {
                WorkflowIrStepKind::Collect
            } else {
                WorkflowIrStepKind::Agent {
                    agent: match &step.agent {
                        StepAgent::ById { id } => id.clone(),
                        StepAgent::ByName { name } => name.clone(),
                    },
                }
            };

            let with = if matches!(step.mode, StepMode::Collect) {
                BTreeMap::new()
            } else {
                BTreeMap::from([(
                    "message".to_string(),
                    Self::legacy_prompt_to_template(&step.prompt_template),
                )])
            };

            if let Some(symbol) = step.output_var.as_ref() {
                symbol_table.insert(symbol.clone(), step.name.clone());
            }

            steps.push(WorkflowIrStep {
                id: step.name.clone(),
                name: step.name.clone(),
                kind,
                flow,
                runtime: ResolvedRuntimeSettings {
                    timeout_secs: step.timeout_secs,
                    error_mode: Self::map_legacy_error_mode(&step.error_mode),
                },
                with,
                save_as: step.output_var.clone(),
            });
        }

        WorkflowIr {
            workflow_id: workflow.id.to_string(),
            workflow_version: "legacy".to_string(),
            defaults: ResolvedRuntimeSettings::default(),
            input_contract: Self::any_contract(),
            output_contract: Self::any_contract(),
            steps,
            symbol_table,
            outputs: BTreeMap::new(),
        }
    }

    fn map_legacy_error_mode(error_mode: &ErrorMode) -> WorkflowV2ErrorMode {
        match error_mode {
            ErrorMode::Fail => WorkflowV2ErrorMode::Fail,
            ErrorMode::Skip => WorkflowV2ErrorMode::Skip,
            ErrorMode::Retry { max_retries } => WorkflowV2ErrorMode::Retry {
                max_retries: *max_retries,
            },
        }
    }

    fn legacy_prompt_to_template(prompt: &str) -> CompiledTemplate {
        let mut segments = Vec::new();
        let mut cursor = 0usize;

        while let Some(relative_start) = prompt[cursor..].find("{{") {
            let start = cursor + relative_start;
            if start > cursor {
                segments.push(TemplateSegment::Text {
                    value: prompt[cursor..start].to_string(),
                });
            }

            let Some(relative_end) = prompt[start + 2..].find("}}") else {
                break;
            };
            let end = start + 2 + relative_end;
            let expression = prompt[start + 2..end].trim();
            let reference = if expression == "input" {
                TemplateReference {
                    namespace: TemplateNamespace::Input,
                    path: Vec::new(),
                }
            } else {
                TemplateReference {
                    namespace: TemplateNamespace::Vars,
                    path: vec![expression.to_string()],
                }
            };
            segments.push(TemplateSegment::Reference { reference });
            cursor = end + 2;
        }

        if cursor < prompt.len() {
            segments.push(TemplateSegment::Text {
                value: prompt[cursor..].to_string(),
            });
        }

        if segments.is_empty() {
            segments.push(TemplateSegment::Text {
                value: prompt.to_string(),
            });
        }

        CompiledTemplate {
            source: prompt.to_string(),
            segments,
        }
    }

    fn render_template(
        template: &CompiledTemplate,
        input: &str,
        vars: &HashMap<String, String>,
    ) -> String {
        let mut rendered = String::new();
        for segment in &template.segments {
            match segment {
                TemplateSegment::Text { value } => rendered.push_str(value),
                TemplateSegment::Reference { reference } => rendered.push_str(
                    &Self::resolve_reference_value(reference, input, vars).unwrap_or_default(),
                ),
            }
        }
        rendered
    }

    fn resolve_reference_value(
        reference: &TemplateReference,
        input: &str,
        vars: &HashMap<String, String>,
    ) -> Option<String> {
        match reference.namespace {
            TemplateNamespace::Input => Self::resolve_string_path(input, &reference.path),
            TemplateNamespace::Vars => {
                let symbol = reference.path.first()?;
                let raw_value = vars.get(symbol)?;
                Self::resolve_string_path(raw_value, &reference.path[1..])
            }
        }
    }

    fn resolve_string_path(raw_value: &str, path: &[String]) -> Option<String> {
        if path.is_empty() {
            return Some(raw_value.to_string());
        }

        let mut current = serde_json::from_str::<serde_json::Value>(raw_value).ok()?;
        for segment in path {
            current = current.get(segment)?.clone();
        }

        Some(match current {
            serde_json::Value::Null => String::new(),
            serde_json::Value::Bool(value) => value.to_string(),
            serde_json::Value::Number(value) => value.to_string(),
            serde_json::Value::String(value) => value,
            other => other.to_string(),
        })
    }

    fn render_step_payload(
        step: &WorkflowIrStep,
        input: &str,
        vars: &HashMap<String, String>,
    ) -> String {
        if step.with.is_empty() {
            return input.to_string();
        }

        let resolved = step
            .with
            .iter()
            .map(|(key, template)| (key.clone(), Self::render_template(template, input, vars)))
            .collect::<BTreeMap<_, _>>();

        if resolved.len() == 1 {
            if let Some(message) = resolved.get("message") {
                return message.clone();
            }
        }

        serde_json::to_string(&resolved).unwrap_or_else(|_| input.to_string())
    }

    fn flow_condition_matches(condition: &str, current_input: &str) -> bool {
        current_input
            .to_lowercase()
            .contains(&condition.to_lowercase())
    }

    fn step_kind_name(kind: &WorkflowIrStepKind) -> &'static str {
        match kind {
            WorkflowIrStepKind::Agent { .. } => "agent",
            WorkflowIrStepKind::Primitive { .. } => "primitive",
            WorkflowIrStepKind::Workflow { .. } => "workflow",
            WorkflowIrStepKind::WaitSignal { .. } => "wait_signal",
            WorkflowIrStepKind::StartLooper { .. } => "start_looper",
            WorkflowIrStepKind::EmitEvent { .. } => "emit_event",
            WorkflowIrStepKind::Collect => "collect",
            WorkflowIrStepKind::Noop => "noop",
        }
    }

    fn agent_target(step: &WorkflowIrStep) -> Result<&str, String> {
        match &step.kind {
            WorkflowIrStepKind::Agent { agent } => Ok(agent.as_str()),
            kind => Err(format!(
                "Step '{}' uses unsupported runtime step kind '{}'",
                step.name,
                Self::step_kind_name(kind)
            )),
        }
    }

    async fn execute_step_with_error_mode<F, Fut>(
        step: &WorkflowIrStep,
        agent_id: AgentId,
        prompt: String,
        send_message: &F,
    ) -> Result<Option<(String, u64, u64)>, String>
    where
        F: Fn(AgentId, String) -> Fut,
        Fut: std::future::Future<Output = Result<(String, u64, u64), String>>,
    {
        let timeout_dur = std::time::Duration::from_secs(step.runtime.timeout_secs);

        match &step.runtime.error_mode {
            WorkflowV2ErrorMode::Fail => {
                let result = tokio::time::timeout(timeout_dur, send_message(agent_id, prompt))
                    .await
                    .map_err(|_| {
                        format!(
                            "Step '{}' timed out after {}s",
                            step.name, step.runtime.timeout_secs
                        )
                    })?
                    .map_err(|error| format!("Step '{}' failed: {error}", step.name))?;
                Ok(Some(result))
            }
            WorkflowV2ErrorMode::Skip => {
                match tokio::time::timeout(timeout_dur, send_message(agent_id, prompt)).await {
                    Ok(Ok(result)) => Ok(Some(result)),
                    Ok(Err(error)) => {
                        warn!("Step '{}' failed (skipping): {error}", step.name);
                        Ok(None)
                    }
                    Err(_) => {
                        warn!(
                            "Step '{}' timed out (skipping) after {}s",
                            step.name, step.runtime.timeout_secs
                        );
                        Ok(None)
                    }
                }
            }
            WorkflowV2ErrorMode::Retry { max_retries } => {
                let mut last_error = String::new();
                for attempt in 0..=*max_retries {
                    match tokio::time::timeout(timeout_dur, send_message(agent_id, prompt.clone()))
                        .await
                    {
                        Ok(Ok(result)) => return Ok(Some(result)),
                        Ok(Err(error)) => {
                            last_error = error.to_string();
                            if attempt < *max_retries {
                                warn!(
                                    "Step '{}' attempt {} failed: {error}, retrying",
                                    step.name,
                                    attempt + 1
                                );
                            }
                        }
                        Err(_) => {
                            last_error = format!("timed out after {}s", step.runtime.timeout_secs);
                            if attempt < *max_retries {
                                warn!(
                                    "Step '{}' attempt {} timed out, retrying",
                                    step.name,
                                    attempt + 1
                                );
                            }
                        }
                    }
                }

                Err(format!(
                    "Step '{}' failed after {} retries: {last_error}",
                    step.name, max_retries
                ))
            }
        }
    }

    async fn workflow_ir_for_record(
        &self,
        record: &WorkflowRunRecord,
    ) -> Result<WorkflowIr, String> {
        if let Some(workflow) = self.get_compiled_workflow(&record.workflow_id).await {
            return Ok(workflow);
        }

        let workflow_id = WorkflowId(Uuid::parse_str(&record.workflow_id).map_err(|error| {
            format!(
                "Stored workflow id '{}' is not a UUID-backed workflow: {error}",
                record.workflow_id
            )
        })?);
        let workflow = self
            .get_workflow(workflow_id)
            .await
            .ok_or_else(|| format!("Workflow definition '{}' not found", record.workflow_id))?;
        Ok(Self::legacy_workflow_to_ir(&workflow))
    }

    async fn waiting_resume_input(
        &self,
        run_id: WorkflowRunId,
        step_id: &str,
        fallback_input_json: &str,
    ) -> Result<String, String> {
        let repository = self.workflow_stores.workflow_checkpoint.clone();
        let run_id_text = run_id.to_string();
        let checkpoints =
            tokio::task::spawn_blocking(move || repository.list_for_run(&run_id_text))
                .await
                .map_err(|error| format!("Workflow checkpoint query task failed: {error}"))?
                .map_err(|error| error.to_string())?;

        for checkpoint in checkpoints.into_iter().rev() {
            if checkpoint.kind != DurableCheckpointKind::WaitingSignal {
                continue;
            }
            if checkpoint.step_id.as_deref() != Some(step_id) {
                continue;
            }

            if let Ok(payload) = serde_json::from_str::<JsonValue>(&checkpoint.data_json) {
                if let Some(value) = payload.get("resume_input") {
                    return Ok(match value {
                        JsonValue::String(value) => value.clone(),
                        other => other.to_string(),
                    });
                }
            }
        }

        Ok(cache_input_from_json(fallback_input_json))
    }

    pub(crate) async fn submit_signal(
        &self,
        run_id: WorkflowRunId,
        name: String,
        payload: JsonValue,
        source: String,
        idempotency_key: String,
    ) -> Result<SignalSubmissionOutcome, String> {
        let writer = self.transition_writer();
        let current = writer
            .load_run(run_id)
            .await
            .map_err(transition_error_to_string)?;
        let workflow = self.workflow_ir_for_record(&current).await?;
        let signal_repository = self.workflow_stores.workflow_signal.clone();
        let run_id_text = run_id.to_string();
        let idempotency_lookup_key = idempotency_key.clone();
        if let Some(existing) = tokio::task::spawn_blocking(move || {
            signal_repository.find_by_idempotency_key(&run_id_text, &idempotency_lookup_key)
        })
        .await
        .map_err(|error| format!("Workflow signal query task failed: {error}"))?
        .map_err(|error| error.to_string())?
        {
            return Ok(SignalSubmissionOutcome {
                signal: existing,
                resume: None,
            });
        }

        let signal = openfang_memory::WorkflowSignalRecord {
            signal_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            name: name.clone(),
            payload_json: serde_json::to_string(&payload)
                .map_err(|error| format!("Failed to serialize workflow signal payload: {error}"))?,
            source,
            idempotency_key,
            consumed: false,
            created_at: now_timestamp(),
            consumed_at: None,
        };
        let received_checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: current.run_id.clone(),
            step_id: current.current_step_id.clone(),
            kind: DurableCheckpointKind::SignalReceived,
            data_json: checkpoint_data_json(serde_json::json!({
                "signal_id": signal.signal_id,
                "signal_name": signal.name,
                "source": signal.source,
                "payload": payload,
            }))
            .map_err(transition_error_to_string)?,
            created_at: signal.created_at.clone(),
        };

        let waiting_matches = current.status == DurableWorkflowRunStatus::WaitingSignal
            && current.waiting_kind.as_deref() == Some("signal")
            && current.waiting_ref.as_deref() == Some(name.as_str());

        if waiting_matches {
            let step_id = current.current_step_id.clone().ok_or_else(|| {
                format!(
                    "Waiting workflow run '{}' had no current step id",
                    current.run_id
                )
            })?;
            let wait_index = workflow
                .steps
                .iter()
                .position(|step| step.id == step_id)
                .ok_or_else(|| {
                    format!(
                        "Workflow step '{}' was not found in workflow '{}'",
                        step_id, workflow.workflow_id
                    )
                })?;
            let next_step_id = workflow
                .steps
                .get(wait_index + 1)
                .map(|step| step.id.clone());
            let mut next = current.clone();
            next.status = DurableWorkflowRunStatus::Running;
            next.current_step_id = next_step_id.clone();
            next.waiting_kind = None;
            next.waiting_ref = None;
            next.updated_at = signal.created_at.clone();

            let consumed_checkpoint = WorkflowCheckpointRecord {
                checkpoint_id: Uuid::new_v4().to_string(),
                run_id: current.run_id.clone(),
                step_id: Some(step_id.clone()),
                kind: DurableCheckpointKind::SignalConsumed,
                data_json: checkpoint_data_json(serde_json::json!({
                    "signal_id": signal.signal_id,
                    "signal_name": signal.name,
                }))
                .map_err(transition_error_to_string)?,
                created_at: signal.created_at.clone(),
            };
            let resumed_checkpoint = WorkflowCheckpointRecord {
                checkpoint_id: Uuid::new_v4().to_string(),
                run_id: current.run_id.clone(),
                step_id: Some(step_id.clone()),
                kind: DurableCheckpointKind::RunResumedFromSignal,
                data_json: checkpoint_data_json(serde_json::json!({
                    "signal_id": signal.signal_id,
                    "signal_name": signal.name,
                }))
                .map_err(transition_error_to_string)?,
                created_at: signal.created_at.clone(),
            };

            let repository = self.workflow_stores.workflow_run.clone();
            let current_record = current.clone();
            let next_record = next.clone();
            let signal_record = signal.clone();
            let received_checkpoint_record = received_checkpoint.clone();
            let consumed_checkpoint_record = consumed_checkpoint.clone();
            let resumed_checkpoint_record = resumed_checkpoint.clone();
            match tokio::task::spawn_blocking(move || {
                repository.persist_signal_submission_and_resume(
                    &current_record,
                    &next_record,
                    SubmittedSignalResume {
                        signal: &signal_record,
                        received_checkpoint: &received_checkpoint_record,
                        consumed_checkpoint: &consumed_checkpoint_record,
                        resumed_checkpoint: &resumed_checkpoint_record,
                        consumed_at: &next_record.updated_at,
                    },
                )
            })
            .await
            .map_err(|error| format!("Workflow signal persist task failed: {error}"))?
            {
                Ok(()) => {
                    writer
                        .sync_cache_from_record(&next, None)
                        .await
                        .map_err(transition_error_to_string)?;
                    let resume_input = self
                        .waiting_resume_input(run_id, &step_id, &current.input_json)
                        .await?;
                    let variables = parse_workflow_vars(&current.vars_json)?;
                    return Ok(SignalSubmissionOutcome {
                        signal: openfang_memory::WorkflowSignalRecord {
                            consumed: true,
                            consumed_at: Some(next.updated_at.clone()),
                            ..signal
                        },
                        resume: Some(SignalResumeContext {
                            run_id,
                            workflow,
                            start_index: wait_index + 1,
                            input: resume_input,
                            variables,
                        }),
                    });
                }
                Err(WorkflowStoreError::UnexpectedRunState { .. }) => {}
                Err(WorkflowStoreError::SignalAlreadyExistsForIdempotency { .. }) => {
                    let repository = self.workflow_stores.workflow_signal.clone();
                    let run_id_text = run_id.to_string();
                    let idempotency_lookup_key = signal.idempotency_key.clone();
                    let existing = tokio::task::spawn_blocking(move || {
                        repository.find_by_idempotency_key(&run_id_text, &idempotency_lookup_key)
                    })
                    .await
                    .map_err(|error| format!("Workflow signal query task failed: {error}"))?
                    .map_err(|error| error.to_string())?
                    .ok_or_else(|| {
                        format!(
                            "Workflow signal for run '{}' disappeared after idempotency conflict",
                            current.run_id
                        )
                    })?;
                    return Ok(SignalSubmissionOutcome {
                        signal: existing,
                        resume: None,
                    });
                }
                Err(error) => return Err(error.to_string()),
            }
        }

        let repository = self.workflow_stores.workflow_run.clone();
        let signal_record = signal.clone();
        let checkpoint_record = received_checkpoint.clone();
        match tokio::task::spawn_blocking(move || {
            repository.persist_signal_submission(&signal_record, &checkpoint_record)
        })
        .await
        .map_err(|error| format!("Workflow signal persist task failed: {error}"))?
        {
            Ok(()) => Ok(SignalSubmissionOutcome {
                signal,
                resume: None,
            }),
            Err(WorkflowStoreError::SignalAlreadyExistsForIdempotency { .. }) => {
                let repository = self.workflow_stores.workflow_signal.clone();
                let run_id_text = run_id.to_string();
                let idempotency_lookup_key = signal.idempotency_key.clone();
                let existing = tokio::task::spawn_blocking(move || {
                    repository.find_by_idempotency_key(&run_id_text, &idempotency_lookup_key)
                })
                .await
                .map_err(|error| format!("Workflow signal query task failed: {error}"))?
                .map_err(|error| error.to_string())?
                .ok_or_else(|| {
                    format!(
                        "Workflow signal for run '{}' disappeared after idempotency conflict",
                        current.run_id
                    )
                })?;
                Ok(SignalSubmissionOutcome {
                    signal: existing,
                    resume: None,
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }

    pub(crate) async fn resume_after_signal<F, Fut>(
        &self,
        resume: SignalResumeContext,
        agent_resolver: impl Fn(&str) -> Option<(AgentId, String)>,
        send_message: F,
    ) -> Result<(), String>
    where
        F: Fn(AgentId, String) -> Fut,
        Fut: std::future::Future<Output = Result<(String, u64, u64), String>>,
    {
        let _ = self
            .execute_steps(
                ExecutionContext {
                    run_id: resume.run_id,
                    workflow: resume.workflow,
                    start_index: resume.start_index,
                    input: resume.input,
                    variables: resume.variables,
                    start_run: false,
                },
                agent_resolver,
                send_message,
            )
            .await?;
        Ok(())
    }

    async fn execute_steps<F, Fut>(
        &self,
        execution: ExecutionContext,
        agent_resolver: impl Fn(&str) -> Option<(AgentId, String)>,
        send_message: F,
    ) -> Result<ExecutionOutcome, String>
    where
        F: Fn(AgentId, String) -> Fut,
        Fut: std::future::Future<Output = Result<(String, u64, u64), String>>,
    {
        let ExecutionContext {
            run_id,
            workflow,
            start_index,
            input: mut current_input,
            mut variables,
            start_run,
        } = execution;
        let send_message = &send_message;
        let vars_json = |variables: &HashMap<String, String>| {
            serde_json::to_string(variables).unwrap_or_else(|_| "{}".to_string())
        };

        if start_run {
            self.record_run_started_transition(
                run_id,
                workflow.steps.first().map(|step| step.id.as_str()),
            )
            .await?;

            info!(
                run_id = %run_id,
                workflow_id = %workflow.workflow_id,
                steps = workflow.steps.len(),
                "Starting workflow execution"
            );
        }

        let mut all_outputs = if start_index == 0 {
            Vec::new()
        } else {
            vec![current_input.clone()]
        };
        let mut index = start_index;

        while index < workflow.steps.len() {
            let step = &workflow.steps[index];

            debug!(step = index + 1, name = %step.name, "Executing workflow step");

            match &step.flow.mode {
                WorkflowV2FlowMode::Sequential => match &step.kind {
                    WorkflowIrStepKind::Collect => {
                        self.record_step_started_transition(run_id, step).await?;
                        current_input = all_outputs.join("\n\n---\n\n");
                        all_outputs.clear();
                        all_outputs.push(current_input.clone());
                        if let Some(symbol) = step.save_as.as_ref() {
                            variables.insert(symbol.clone(), current_input.clone());
                        }
                        self.record_step_completed_transition(
                            run_id,
                            &step.id,
                            step.save_as.as_deref(),
                            &checkpoint_output_summary(&current_input),
                            vars_json(&variables),
                        )
                        .await?;
                    }
                    WorkflowIrStepKind::Noop => {
                        self.record_step_started_transition(run_id, step).await?;
                        all_outputs.push(current_input.clone());
                        if let Some(symbol) = step.save_as.as_ref() {
                            variables.insert(symbol.clone(), current_input.clone());
                        }
                        self.record_step_completed_transition(
                            run_id,
                            &step.id,
                            step.save_as.as_deref(),
                            &checkpoint_output_summary(&current_input),
                            vars_json(&variables),
                        )
                        .await?;
                    }
                    WorkflowIrStepKind::WaitSignal { signal_name } => {
                        self.record_step_started_transition(run_id, step).await?;
                        let repository = self.workflow_stores.workflow_signal.clone();
                        let run_id_text = run_id.to_string();
                        let signal_name_text = signal_name.clone();
                        let maybe_signal = tokio::task::spawn_blocking(move || {
                            repository.find_unconsumed(&run_id_text, &signal_name_text)
                        })
                        .await
                        .map_err(|error| format!("Workflow signal query task failed: {error}"))?
                        .map_err(|error| error.to_string())?;

                        if let Some(signal) = maybe_signal {
                            self.record_signal_consumed_transition(
                                run_id,
                                &step.id,
                                workflow.steps.get(index + 1).map(|next| next.id.as_str()),
                                &signal.signal_id,
                                signal_name,
                            )
                            .await?;
                            index += 1;
                            continue;
                        }

                        self.record_waiting_for_signal_transition(
                            run_id,
                            &step.id,
                            signal_name,
                            &current_input,
                        )
                        .await?;
                        info!(
                            run_id = %run_id,
                            workflow_id = %workflow.workflow_id,
                            step_id = %step.id,
                            signal_name = %signal_name,
                            "Workflow run parked waiting for signal"
                        );
                        return Ok(ExecutionOutcome::Parked(current_input));
                    }
                    _ => {
                        self.record_step_started_transition(run_id, step).await?;
                        let agent = match Self::agent_target(step) {
                            Ok(agent) => agent,
                            Err(error) => {
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };
                        let (agent_id, agent_name) = match agent_resolver(agent) {
                            Some(agent) => agent,
                            None => {
                                let error = format!("Agent not found for step '{}'", step.name);
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };
                        let prompt = Self::render_step_payload(step, &current_input, &variables);
                        let start = std::time::Instant::now();
                        let result = Self::execute_step_with_error_mode(
                            step,
                            agent_id,
                            prompt,
                            send_message,
                        )
                        .await;
                        let duration_ms = start.elapsed().as_millis() as u64;

                        match result {
                            Ok(Some((output, input_tokens, output_tokens))) => {
                                if let Some(run) = self.runs.write().await.get_mut(&run_id) {
                                    run.step_results.push(StepResult {
                                        step_name: step.name.clone(),
                                        agent_id: agent_id.to_string(),
                                        agent_name,
                                        output: output.clone(),
                                        input_tokens,
                                        output_tokens,
                                        duration_ms,
                                    });
                                }

                                if let Some(symbol) = step.save_as.as_ref() {
                                    variables.insert(symbol.clone(), output.clone());
                                }

                                all_outputs.push(output.clone());
                                current_input = output;
                                self.record_step_completed_transition(
                                    run_id,
                                    &step.id,
                                    step.save_as.as_deref(),
                                    &checkpoint_output_summary(&current_input),
                                    vars_json(&variables),
                                )
                                .await?;
                                info!(
                                    step = index + 1,
                                    name = %step.name,
                                    duration_ms,
                                    "Step completed"
                                );
                            }
                            Ok(None) => {
                                self.record_step_skipped_transition(
                                    run_id,
                                    &step.id,
                                    "step returned no output",
                                )
                                .await?;
                                info!(step = index + 1, name = %step.name, "Step skipped");
                            }
                            Err(error) => {
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        }
                    }
                },
                WorkflowV2FlowMode::FanOut => {
                    let mut fan_out_steps = vec![(index, step)];
                    let mut cursor = index + 1;
                    while cursor < workflow.steps.len() {
                        if matches!(workflow.steps[cursor].flow.mode, WorkflowV2FlowMode::FanOut) {
                            fan_out_steps.push((cursor, &workflow.steps[cursor]));
                            cursor += 1;
                        } else {
                            break;
                        }
                    }

                    let mut futures = Vec::new();
                    let mut step_infos = Vec::new();

                    for (fan_index, fan_step) in &fan_out_steps {
                        self.record_step_started_transition(run_id, fan_step)
                            .await?;
                        let agent = match Self::agent_target(fan_step) {
                            Ok(agent) => agent,
                            Err(error) => {
                                self.record_step_failed_transition(run_id, &fan_step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(
                                        run_id,
                                        Some(&fan_step.id),
                                        &error,
                                    )
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };
                        let (agent_id, agent_name) = match agent_resolver(agent) {
                            Some(agent) => agent,
                            None => {
                                let error = format!("Agent not found for step '{}'", fan_step.name);
                                self.record_step_failed_transition(run_id, &fan_step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(
                                        run_id,
                                        Some(&fan_step.id),
                                        &error,
                                    )
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };
                        let prompt =
                            Self::render_step_payload(fan_step, &current_input, &variables);

                        step_infos.push((*fan_index, fan_step.name.clone(), agent_id, agent_name));
                        futures.push(async move {
                            let start = std::time::Instant::now();
                            let result = Self::execute_step_with_error_mode(
                                fan_step,
                                agent_id,
                                prompt,
                                send_message,
                            )
                            .await;
                            let duration_ms = start.elapsed().as_millis() as u64;
                            (result, duration_ms)
                        });
                    }

                    let results = futures::future::join_all(futures).await;

                    for (result_index, (result, duration_ms)) in results.into_iter().enumerate() {
                        let (_, step_name, agent_id, agent_name) = &step_infos[result_index];
                        let fan_step = fan_out_steps[result_index].1;

                        match result {
                            Ok(Some((output, input_tokens, output_tokens))) => {
                                if let Some(run) = self.runs.write().await.get_mut(&run_id) {
                                    run.step_results.push(StepResult {
                                        step_name: step_name.clone(),
                                        agent_id: agent_id.to_string(),
                                        agent_name: agent_name.clone(),
                                        output: output.clone(),
                                        input_tokens,
                                        output_tokens,
                                        duration_ms,
                                    });
                                }

                                if let Some(symbol) = fan_step.save_as.as_ref() {
                                    variables.insert(symbol.clone(), output.clone());
                                }
                                all_outputs.push(output.clone());
                                current_input = output;
                                self.record_step_completed_transition(
                                    run_id,
                                    &fan_step.id,
                                    fan_step.save_as.as_deref(),
                                    &checkpoint_output_summary(&current_input),
                                    vars_json(&variables),
                                )
                                .await?;
                            }
                            Ok(None) => {
                                self.record_step_skipped_transition(
                                    run_id,
                                    &fan_step.id,
                                    "fan-out step returned no output",
                                )
                                .await?;
                                info!(name = %step_name, "FanOut step skipped");
                            }
                            Err(error) => {
                                let error = format!("FanOut step '{step_name}' failed: {error}");
                                self.record_step_failed_transition(run_id, &fan_step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(
                                        run_id,
                                        Some(&fan_step.id),
                                        &error,
                                    )
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        }
                    }

                    index = cursor;
                    continue;
                }
                WorkflowV2FlowMode::Conditional { when } => {
                    self.record_step_started_transition(run_id, step).await?;
                    if !Self::flow_condition_matches(when, &current_input) {
                        self.record_step_skipped_transition(run_id, &step.id, "condition_not_met")
                            .await?;
                        info!(
                            step = index + 1,
                            name = %step.name,
                            condition = when,
                            "Conditional step skipped (condition not met)"
                        );
                        index += 1;
                        continue;
                    }

                    if matches!(step.kind, WorkflowIrStepKind::Noop) {
                        if let Some(symbol) = step.save_as.as_ref() {
                            variables.insert(symbol.clone(), current_input.clone());
                        }
                        all_outputs.push(current_input.clone());
                        self.record_step_completed_transition(
                            run_id,
                            &step.id,
                            step.save_as.as_deref(),
                            &checkpoint_output_summary(&current_input),
                            vars_json(&variables),
                        )
                        .await?;
                    } else {
                        let agent = match Self::agent_target(step) {
                            Ok(agent) => agent,
                            Err(error) => {
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };
                        let (agent_id, agent_name) = match agent_resolver(agent) {
                            Some(agent) => agent,
                            None => {
                                let error = format!("Agent not found for step '{}'", step.name);
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };
                        let prompt = Self::render_step_payload(step, &current_input, &variables);
                        let start = std::time::Instant::now();
                        let result = Self::execute_step_with_error_mode(
                            step,
                            agent_id,
                            prompt,
                            send_message,
                        )
                        .await;
                        let duration_ms = start.elapsed().as_millis() as u64;

                        match result {
                            Ok(Some((output, input_tokens, output_tokens))) => {
                                if let Some(run) = self.runs.write().await.get_mut(&run_id) {
                                    run.step_results.push(StepResult {
                                        step_name: step.name.clone(),
                                        agent_id: agent_id.to_string(),
                                        agent_name,
                                        output: output.clone(),
                                        input_tokens,
                                        output_tokens,
                                        duration_ms,
                                    });
                                }

                                if let Some(symbol) = step.save_as.as_ref() {
                                    variables.insert(symbol.clone(), output.clone());
                                }
                                all_outputs.push(output.clone());
                                current_input = output;
                                self.record_step_completed_transition(
                                    run_id,
                                    &step.id,
                                    step.save_as.as_deref(),
                                    &checkpoint_output_summary(&current_input),
                                    vars_json(&variables),
                                )
                                .await?;
                            }
                            Ok(None) => {
                                self.record_step_skipped_transition(
                                    run_id,
                                    &step.id,
                                    "conditional step returned no output",
                                )
                                .await?;
                            }
                            Err(error) => {
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        }
                    }
                }
                WorkflowV2FlowMode::Loop {
                    max_iterations,
                    until,
                } => {
                    self.record_step_started_transition(run_id, step).await?;
                    if matches!(step.kind, WorkflowIrStepKind::Noop) {
                        for loop_iter in 0..*max_iterations {
                            if Self::flow_condition_matches(until, &current_input) {
                                info!(
                                    step = index + 1,
                                    name = %step.name,
                                    iterations = loop_iter + 1,
                                    "Loop terminated (until condition met)"
                                );
                                break;
                            }
                        }

                        if let Some(symbol) = step.save_as.as_ref() {
                            variables.insert(symbol.clone(), current_input.clone());
                        }
                        all_outputs.push(current_input.clone());
                        self.record_step_completed_transition(
                            run_id,
                            &step.id,
                            step.save_as.as_deref(),
                            &checkpoint_output_summary(&current_input),
                            vars_json(&variables),
                        )
                        .await?;
                    } else {
                        let agent = match Self::agent_target(step) {
                            Ok(agent) => agent,
                            Err(error) => {
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };
                        let (agent_id, agent_name) = match agent_resolver(agent) {
                            Some(agent) => agent,
                            None => {
                                let error = format!("Agent not found for step '{}'", step.name);
                                self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                    .await?;
                                if let Err(persist_error) = self
                                    .record_run_failed_transition(run_id, Some(&step.id), &error)
                                    .await
                                {
                                    return Err(format!(
                                        "{error}; additionally failed to persist workflow failure: {persist_error}"
                                    ));
                                }
                                return Err(error);
                            }
                        };

                        for loop_iter in 0..*max_iterations {
                            let prompt =
                                Self::render_step_payload(step, &current_input, &variables);
                            let start = std::time::Instant::now();
                            let result = Self::execute_step_with_error_mode(
                                step,
                                agent_id,
                                prompt,
                                send_message,
                            )
                            .await;
                            let duration_ms = start.elapsed().as_millis() as u64;

                            match result {
                                Ok(Some((output, input_tokens, output_tokens))) => {
                                    if let Some(run) = self.runs.write().await.get_mut(&run_id) {
                                        run.step_results.push(StepResult {
                                            step_name: format!(
                                                "{} (iter {})",
                                                step.name,
                                                loop_iter + 1
                                            ),
                                            agent_id: agent_id.to_string(),
                                            agent_name: agent_name.clone(),
                                            output: output.clone(),
                                            input_tokens,
                                            output_tokens,
                                            duration_ms,
                                        });
                                    }

                                    current_input = output.clone();

                                    if Self::flow_condition_matches(until, &output) {
                                        info!(
                                            step = index + 1,
                                            name = %step.name,
                                            iterations = loop_iter + 1,
                                            "Loop terminated (until condition met)"
                                        );
                                        break;
                                    }

                                    if loop_iter + 1 == *max_iterations {
                                        info!(
                                            step = index + 1,
                                            name = %step.name,
                                            "Loop terminated (max iterations reached)"
                                        );
                                    }
                                }
                                Ok(None) => break,
                                Err(error) => {
                                    self.record_step_failed_transition(run_id, &step.id, &error, 1)
                                        .await?;
                                    if let Err(persist_error) = self
                                        .record_run_failed_transition(
                                            run_id,
                                            Some(&step.id),
                                            &error,
                                        )
                                        .await
                                    {
                                        return Err(format!(
                                            "{error}; additionally failed to persist workflow failure: {persist_error}"
                                        ));
                                    }
                                    return Err(error);
                                }
                            }
                        }

                        if let Some(symbol) = step.save_as.as_ref() {
                            variables.insert(symbol.clone(), current_input.clone());
                        }
                        all_outputs.push(current_input.clone());
                        self.record_step_completed_transition(
                            run_id,
                            &step.id,
                            step.save_as.as_deref(),
                            &checkpoint_output_summary(&current_input),
                            vars_json(&variables),
                        )
                        .await?;
                    }
                }
            }

            index += 1;
        }

        let final_output = current_input.clone();
        self.record_run_completed_transition(run_id, &final_output)
            .await?;
        info!(run_id = %run_id, workflow_id = %workflow.workflow_id, "Workflow completed successfully");
        Ok(ExecutionOutcome::Completed(final_output))
    }

    /// Execute a workflow run from compiled IR.
    pub async fn execute_run<F, Fut>(
        &self,
        run_id: WorkflowRunId,
        workflow: WorkflowIr,
        agent_resolver: impl Fn(&str) -> Option<(AgentId, String)>,
        send_message: F,
    ) -> Result<String, String>
    where
        F: Fn(AgentId, String) -> Fut,
        Fut: std::future::Future<Output = Result<(String, u64, u64), String>>,
    {
        let (input, run_workflow_id) = {
            let runs = self.runs.read().await;
            let run = runs.get(&run_id).ok_or("Workflow run not found")?;
            (run.input.clone(), run.workflow_id)
        };

        if run_workflow_id.to_string() != workflow.workflow_id {
            let error = format!(
                "Workflow IR '{}' does not match run workflow '{}'",
                workflow.workflow_id, run_workflow_id
            );
            if let Err(start_error) = self.record_run_started_transition(run_id, None).await {
                return Err(format!(
                    "{error}; additionally failed to persist workflow start: {start_error}"
                ));
            }
            if let Err(persist_error) = self
                .record_run_failed_transition(run_id, None, &error)
                .await
            {
                return Err(format!(
                    "{error}; additionally failed to persist workflow failure: {persist_error}"
                ));
            }
            return Err(error);
        }

        match self
            .execute_steps(
                ExecutionContext {
                    run_id,
                    workflow,
                    start_index: 0,
                    input,
                    variables: HashMap::new(),
                    start_run: true,
                },
                agent_resolver,
                send_message,
            )
            .await?
        {
            ExecutionOutcome::Completed(output) | ExecutionOutcome::Parked(output) => Ok(output),
        }
    }
}

impl Default for WorkflowEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn test_engine(workflows_dir: PathBuf) -> WorkflowEngine {
        WorkflowEngine::with_definitions_dir(workflows_dir)
    }

    fn workflow_path(workflows_dir: &std::path::Path, workflow_id: WorkflowId) -> PathBuf {
        workflows_dir.join(format!("{workflow_id}.json"))
    }

    fn workflow_ids_from_disk(workflows_dir: &std::path::Path) -> HashSet<WorkflowId> {
        let mut workflow_ids = HashSet::new();
        let Ok(entries) = std::fs::read_dir(workflows_dir) else {
            return workflow_ids;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let content = std::fs::read_to_string(&path).expect("workflow file should be readable");
            let workflow: Workflow =
                serde_json::from_str(&content).expect("workflow file should deserialize");
            workflow_ids.insert(workflow.id);
        }

        workflow_ids
    }

    fn write_workflow_json(path: &std::path::Path, workflow: &Workflow) {
        std::fs::create_dir_all(
            path.parent()
                .expect("workflow definition path should have a parent"),
        )
        .expect("workflow definition parent should exist");
        std::fs::write(
            path,
            serde_json::to_string_pretty(workflow).expect("workflow should serialize to json"),
        )
        .expect("workflow json should be written");
    }

    fn write_workflow_toml(path: &std::path::Path, workflow: &Workflow) {
        std::fs::create_dir_all(
            path.parent()
                .expect("workflow definition path should have a parent"),
        )
        .expect("workflow definition parent should exist");
        std::fs::write(
            path,
            toml::to_string_pretty(workflow).expect("workflow should serialize to toml"),
        )
        .expect("workflow toml should be written");
    }

    fn test_workflow() -> Workflow {
        Workflow {
            id: WorkflowId::new(),
            name: "test-pipeline".to_string(),
            description: "A test pipeline".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "analyze".to_string(),
                    agent: StepAgent::ByName {
                        name: "analyst".to_string(),
                    },
                    prompt_template: "Analyze this: {{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 30,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "summarize".to_string(),
                    agent: StepAgent::ByName {
                        name: "writer".to_string(),
                    },
                    prompt_template: "Summarize this analysis: {{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 30,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        }
    }

    fn test_workflow_v2_definition(workflow_id: WorkflowId) -> WorkflowV2Definition {
        serde_json::from_value(json!({
            "id": workflow_id.to_string(),
            "name": "workflow-v2-test",
            "version": "1.0.0",
            "description": "Compiled workflow test",
            "input": {
                "kind": "object",
                "required": ["issue"],
                "open": false,
                "fields": {
                    "issue": { "kind": "string" }
                }
            },
            "output": {
                "kind": "object",
                "required": ["result"],
                "open": false,
                "fields": {
                    "result": { "kind": "string" }
                }
            },
            "steps": [
                {
                    "id": "analyze",
                    "name": "Analyze",
                    "kind": "agent",
                    "uses": { "agent": "analyst" },
                    "with": {
                        "message": "Analyze this: {{ input.issue }}"
                    },
                    "save_as": "analysis",
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "summarize",
                    "name": "Summarize",
                    "kind": "agent",
                    "uses": { "agent": "writer" },
                    "with": {
                        "message": "Summarize this analysis: {{ vars.analysis }}"
                    },
                    "save_as": "result",
                    "flow": { "mode": "sequential" }
                }
            ],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        }))
        .expect("workflow v2 test definition should deserialize")
    }

    fn test_wait_signal_definition(workflow_id: WorkflowId) -> WorkflowV2Definition {
        serde_json::from_value(json!({
            "id": workflow_id.to_string(),
            "name": "wait-signal-test",
            "version": "1.0.0",
            "description": "wait signal workflow test",
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
                    "id": "await-approval",
                    "name": "Await approval",
                    "kind": "wait_signal",
                    "uses": { "signal_name": "approval" },
                    "flow": { "mode": "sequential" }
                },
                {
                    "id": "after-approval",
                    "name": "After approval",
                    "kind": "noop",
                    "save_as": "result",
                    "flow": { "mode": "sequential" }
                }
            ],
            "outputs": {
                "result": "{{ vars.result }}"
            }
        }))
        .expect("wait signal workflow test definition should deserialize")
    }

    fn legacy_ir(workflow: &Workflow) -> WorkflowIr {
        WorkflowEngine::legacy_workflow_to_ir(workflow)
    }

    fn mock_resolver(agent: &str) -> Option<(AgentId, String)> {
        let _ = agent;
        Some((AgentId::new(), "mock-agent".to_string()))
    }

    fn load_durable_run(engine: &WorkflowEngine, run_id: WorkflowRunId) -> WorkflowRunRecord {
        engine
            .workflow_stores
            .workflow_run
            .find_by_id(&run_id.to_string())
            .expect("durable workflow run query should succeed")
            .expect("durable workflow run should exist")
    }

    fn load_durable_checkpoints(
        engine: &WorkflowEngine,
        run_id: WorkflowRunId,
    ) -> Vec<WorkflowCheckpointRecord> {
        engine
            .workflow_stores
            .workflow_checkpoint
            .list_for_run(&run_id.to_string())
            .expect("durable workflow checkpoint query should succeed")
    }

    #[tokio::test]
    async fn workflow_registry_readiness_starts_not_ready() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().join("workflows"));

        assert!(!engine.is_ready());
        assert_eq!(engine.readiness(), WorkflowRegistryReadiness::Bootstrapping);
    }

    #[tokio::test]
    async fn bootstrap_workflow_definitions_loads_all_valid_files() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("bootstrap");
        let engine = test_engine(temp_dir.path().join("managed"));
        let first = test_workflow();
        let second = Workflow {
            id: WorkflowId::new(),
            name: "toml-workflow".to_string(),
            description: "loaded from toml".to_string(),
            steps: Vec::new(),
            created_at: Utc::now(),
        };

        write_workflow_json(&workflows_dir.join("b-valid.json"), &first);
        write_workflow_toml(&workflows_dir.join("a-valid.toml"), &second);

        let bootstrap = engine
            .bootstrap_from_store(WorkflowDefinitionStore::new(workflows_dir))
            .await;

        assert_eq!(bootstrap.loaded, 2);
        assert_eq!(bootstrap.skipped, 0);
        assert!(bootstrap.errors.is_empty());

        let loaded_ids = engine
            .list_workflows()
            .await
            .into_iter()
            .map(|workflow| workflow.id)
            .collect::<HashSet<_>>();
        assert_eq!(loaded_ids, HashSet::from([first.id, second.id]));
    }

    #[tokio::test]
    async fn bootstrap_workflow_definitions_skips_invalid_files_with_warning() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("bootstrap");
        let engine = test_engine(temp_dir.path().join("managed"));
        let valid = test_workflow();

        write_workflow_json(&workflows_dir.join("a-valid.json"), &valid);
        std::fs::create_dir_all(&workflows_dir).expect("workflow dir should be created");
        std::fs::write(workflows_dir.join("b-invalid.json"), "{not valid json")
            .expect("invalid workflow file should be written");

        let bootstrap = engine
            .bootstrap_from_store(WorkflowDefinitionStore::new(workflows_dir.clone()))
            .await;

        assert_eq!(bootstrap.loaded, 1);
        assert_eq!(bootstrap.skipped, 1);
        assert_eq!(bootstrap.errors.len(), 1);
        assert_eq!(bootstrap.errors[0].level, WorkflowBootstrapErrorLevel::Warn);
        assert!(bootstrap.errors[0]
            .path
            .ends_with(std::path::Path::new("b-invalid.json")));
    }

    #[tokio::test]
    async fn bootstrap_workflow_definitions_tolerates_missing_directory() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let missing_dir = temp_dir.path().join("missing-workflows");
        let engine = test_engine(temp_dir.path().join("managed"));

        let bootstrap = engine
            .bootstrap_from_store(WorkflowDefinitionStore::new(missing_dir.clone()))
            .await;

        assert_eq!(bootstrap.loaded, 0);
        assert_eq!(bootstrap.skipped, 0);
        assert!(bootstrap.errors.is_empty());
        assert!(engine.is_ready());
        assert_eq!(
            bootstrap.events,
            vec![WorkflowBootstrapEvent {
                path: missing_dir,
                outcome: WorkflowBootstrapOutcome::MissingDirectory,
            }]
        );
    }

    #[tokio::test]
    async fn workflow_registry_readiness_set_after_bootstrap() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("bootstrap");
        let engine = test_engine(temp_dir.path().join("managed"));
        let workflow = test_workflow();
        write_workflow_json(&workflows_dir.join("ready.json"), &workflow);

        let bootstrap = engine
            .bootstrap_from_store(WorkflowDefinitionStore::new(workflows_dir))
            .await;

        assert_eq!(bootstrap.loaded, 1);
        assert!(engine.is_ready());
        assert_eq!(engine.readiness(), WorkflowRegistryReadiness::Ready);
    }

    #[tokio::test]
    async fn bootstrap_load_order_is_deterministic() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("bootstrap");
        let engine = test_engine(temp_dir.path().join("managed"));
        let first = test_workflow();
        let second = Workflow {
            id: WorkflowId::new(),
            name: "second".to_string(),
            description: "second".to_string(),
            steps: Vec::new(),
            created_at: Utc::now(),
        };
        let third = Workflow {
            id: WorkflowId::new(),
            name: "third".to_string(),
            description: "third".to_string(),
            steps: Vec::new(),
            created_at: Utc::now(),
        };

        write_workflow_json(&workflows_dir.join("c-last.json"), &third);
        write_workflow_json(&workflows_dir.join("a-first.json"), &first);
        write_workflow_toml(&workflows_dir.join("b-middle.toml"), &second);

        let bootstrap = engine
            .bootstrap_from_store(WorkflowDefinitionStore::new(workflows_dir))
            .await;
        let loaded_paths = bootstrap
            .events
            .iter()
            .filter(|event| event.outcome == WorkflowBootstrapOutcome::Loaded)
            .map(|event| {
                event
                    .path
                    .file_name()
                    .expect("loaded workflow path should have a filename")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();

        assert_eq!(
            loaded_paths,
            vec![
                "a-first.json".to_string(),
                "b-middle.toml".to_string(),
                "c-last.json".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn test_register_workflow() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let wf = test_workflow();
        let id = engine
            .register(wf.clone())
            .await
            .expect("workflow registration should succeed");
        assert_eq!(id, wf.id);

        let retrieved = engine.get_workflow(id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name, "test-pipeline");
    }

    #[tokio::test]
    async fn test_create_run() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let wf = test_workflow();
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");

        let run_id = engine
            .create_run(wf_id, "test input".to_string())
            .await
            .expect("workflow run should be created");

        let run = engine.get_run(run_id).await.unwrap();
        assert_eq!(run.input, "test input");
        assert!(matches!(run.state, WorkflowRunState::Pending));
    }

    #[tokio::test]
    async fn run_creation_persists_workflow_run_row() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");

        let run_id = engine
            .create_run(workflow_id, "test input".to_string())
            .await
            .expect("workflow run should be created");

        let record = load_durable_run(&engine, run_id);
        let stored_input: serde_json::Value =
            serde_json::from_str(&record.input_json).expect("stored input should be valid json");

        assert_eq!(record.workflow_id, workflow_id.to_string());
        assert_eq!(record.workflow_version, "legacy");
        assert_eq!(record.status, DurableWorkflowRunStatus::Pending);
        assert_eq!(stored_input, serde_json::json!("test input"));
    }

    #[tokio::test]
    async fn run_creation_appends_run_created_checkpoint() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");

        let run_id = engine
            .create_run(workflow_id, "seed input".to_string())
            .await
            .expect("workflow run should be created");

        let checkpoints = load_durable_checkpoints(&engine, run_id);

        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].kind, DurableCheckpointKind::RunCreated);
        let payload: serde_json::Value = serde_json::from_str(&checkpoints[0].data_json)
            .expect("run_created checkpoint payload should be valid json");
        assert_eq!(payload["workflow_id"], workflow_id.to_string());
        assert_eq!(payload["input"], serde_json::json!("seed input"));
    }

    #[tokio::test]
    async fn transition_writer_step_completed_updates_run_and_appends_checkpoint() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "step input".to_string())
            .await
            .expect("workflow run should be created");

        engine
            .record_run_started_transition(run_id, Some("analyze"))
            .await
            .expect("run should start");
        engine
            .record_step_started_transition(
                run_id,
                &WorkflowIrStep {
                    id: "analyze".to_string(),
                    name: "Analyze".to_string(),
                    kind: WorkflowIrStepKind::Agent {
                        agent: "analyst".to_string(),
                    },
                    flow: FlowBlock {
                        mode: WorkflowV2FlowMode::Sequential,
                    },
                    runtime: ResolvedRuntimeSettings::default(),
                    with: BTreeMap::new(),
                    save_as: Some("analysis".to_string()),
                },
            )
            .await
            .expect("step should start");
        engine
            .record_step_completed_transition(
                run_id,
                "analyze",
                Some("analysis"),
                "analysis ready",
                serde_json::json!({ "analysis": "ready" }).to_string(),
            )
            .await
            .expect("step should complete");

        let record = load_durable_run(&engine, run_id);
        let checkpoints = load_durable_checkpoints(&engine, run_id);

        assert_eq!(record.current_step_id.as_deref(), Some("analyze"));
        let vars: serde_json::Value =
            serde_json::from_str(&record.vars_json).expect("stored vars should be valid json");
        assert_eq!(vars["analysis"], "ready");
        assert_eq!(
            checkpoints.last().map(|checkpoint| checkpoint.kind),
            Some(DurableCheckpointKind::StepCompleted)
        );
    }

    #[tokio::test]
    async fn transition_writer_rejects_invalid_status_transition() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "pending input".to_string())
            .await
            .expect("workflow run should be created");

        let error = engine
            .transition_writer()
            .record_run_completed(run_id, "done")
            .await
            .expect_err("pending run should not complete directly");

        let record = load_durable_run(&engine, run_id);
        let checkpoints = load_durable_checkpoints(&engine, run_id);

        assert!(matches!(
            error,
            TransitionError::InvalidStatusTransition { .. }
        ));
        assert_eq!(record.status, DurableWorkflowRunStatus::Pending);
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].kind, DurableCheckpointKind::RunCreated);
    }

    #[tokio::test]
    async fn register_workflow_v2_definition_caches_compiled_ir() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow_id = WorkflowId::new();
        let definition = test_workflow_v2_definition(workflow_id);

        engine
            .register_workflow_v2_definition(
                definition.clone(),
                ["analyst".to_string(), "writer".to_string()],
            )
            .await
            .expect("workflow v2 registration should succeed");

        assert_eq!(
            engine.get_workflow_v2_definition(&definition.id).await,
            Some(definition.clone())
        );
        let compiled = engine
            .get_compiled_workflow(&definition.id)
            .await
            .expect("compiled workflow ir should be cached");
        assert_eq!(compiled.workflow_id, definition.id);
    }

    #[tokio::test]
    async fn test_list_workflows() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let wf = test_workflow();
        engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");

        let list = engine.list_workflows().await;
        assert_eq!(list.len(), 1);
    }

    #[tokio::test]
    async fn test_remove_workflow() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let wf = test_workflow();
        let id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");

        assert!(engine
            .remove_workflow(id)
            .await
            .expect("workflow removal should succeed"));
        assert!(engine.get_workflow(id).await.is_none());
    }

    #[tokio::test]
    async fn workflow_update_persists_canonical_definition() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("workflows");
        let engine = test_engine(workflows_dir.clone());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow.clone())
            .await
            .expect("workflow registration should succeed");

        let updated = Workflow {
            id: workflow_id,
            name: "updated-pipeline".to_string(),
            description: "Updated description".to_string(),
            steps: vec![WorkflowStep {
                name: "rewrite".to_string(),
                agent: StepAgent::ByName {
                    name: "writer".to_string(),
                },
                prompt_template: "Updated {{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 45,
                error_mode: ErrorMode::Fail,
                output_var: Some("updated".to_string()),
            }],
            created_at: Utc::now(),
        };

        let updated_result = engine
            .update_workflow(workflow_id, updated)
            .await
            .expect("workflow update should succeed");
        assert!(updated_result);

        let persisted_content = std::fs::read_to_string(workflow_path(&workflows_dir, workflow_id))
            .expect("workflow file should exist");
        let persisted_workflow: Workflow =
            serde_json::from_str(&persisted_content).expect("workflow file should deserialize");
        let in_memory_workflow = engine
            .get_workflow(workflow_id)
            .await
            .expect("workflow should remain registered");

        assert_eq!(persisted_workflow, in_memory_workflow);
        assert_eq!(persisted_workflow.name, "updated-pipeline");
        assert_eq!(persisted_workflow.description, "Updated description");
        assert_eq!(
            persisted_workflow.steps[0].prompt_template,
            "Updated {{input}}"
        );
        assert_eq!(persisted_workflow.created_at, workflow.created_at);
    }

    #[tokio::test]
    async fn workflow_delete_removes_definition_file() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("workflows");
        let engine = test_engine(workflows_dir.clone());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");
        let definition_path = workflow_path(&workflows_dir, workflow_id);

        assert!(definition_path.exists());

        let removed = engine
            .remove_workflow(workflow_id)
            .await
            .expect("workflow removal should succeed");
        assert!(removed);
        assert!(!definition_path.exists());
    }

    #[tokio::test]
    async fn workflow_create_rolls_back_on_disk_failure() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let blocked_parent = temp_dir.path().join("blocked-parent");
        std::fs::write(&blocked_parent, "not a directory")
            .expect("blocked parent file should be created");
        let engine = test_engine(blocked_parent.join("workflows"));
        let workflow = test_workflow();

        let result = engine.register(workflow.clone()).await;

        assert!(result.is_err());
        assert!(engine.get_workflow(workflow.id).await.is_none());
    }

    #[tokio::test]
    async fn workflow_update_rolls_back_on_disk_failure() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("workflows");
        let engine = test_engine(workflows_dir.clone());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow.clone())
            .await
            .expect("workflow registration should succeed");
        let old_in_memory = engine
            .get_workflow(workflow_id)
            .await
            .expect("workflow should exist");

        std::fs::create_dir_all(workflows_dir.join(format!("{workflow_id}.json.tmp")))
            .expect("blocking temp path should be created");

        let updated = Workflow {
            id: workflow_id,
            name: "should-not-stick".to_string(),
            description: "should-not-stick".to_string(),
            steps: vec![WorkflowStep {
                name: "rewrite".to_string(),
                agent: StepAgent::ByName {
                    name: "writer".to_string(),
                },
                prompt_template: "BROKEN {{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 99,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };

        let result = engine.update_workflow(workflow_id, updated).await;

        assert!(result.is_err());
        let current_in_memory = engine
            .get_workflow(workflow_id)
            .await
            .expect("workflow should still exist");
        assert_eq!(current_in_memory, old_in_memory);

        let persisted_content = std::fs::read_to_string(workflow_path(&workflows_dir, workflow_id))
            .expect("workflow file should still exist");
        let persisted_workflow: Workflow =
            serde_json::from_str(&persisted_content).expect("workflow file should deserialize");
        assert_eq!(persisted_workflow, old_in_memory);
    }

    #[tokio::test]
    async fn runtime_registry_and_file_store_stay_aligned() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("workflows");
        let engine = test_engine(workflows_dir.clone());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow.clone())
            .await
            .expect("workflow registration should succeed");

        let in_memory_ids = engine
            .list_workflows()
            .await
            .into_iter()
            .map(|workflow| workflow.id)
            .collect::<HashSet<_>>();
        assert_eq!(in_memory_ids, workflow_ids_from_disk(&workflows_dir));

        let updated = Workflow {
            id: workflow_id,
            name: "aligned".to_string(),
            description: "aligned".to_string(),
            steps: vec![WorkflowStep {
                name: "aligned".to_string(),
                agent: StepAgent::ByName {
                    name: "writer".to_string(),
                },
                prompt_template: "Aligned {{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 30,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };

        let updated_result = engine
            .update_workflow(workflow_id, updated)
            .await
            .expect("workflow update should succeed");
        assert!(updated_result);

        let updated_in_memory_ids = engine
            .list_workflows()
            .await
            .into_iter()
            .map(|workflow| workflow.id)
            .collect::<HashSet<_>>();
        assert_eq!(
            updated_in_memory_ids,
            workflow_ids_from_disk(&workflows_dir)
        );

        let removed = engine
            .remove_workflow(workflow_id)
            .await
            .expect("workflow removal should succeed");
        assert!(removed);

        let final_in_memory_ids = engine
            .list_workflows()
            .await
            .into_iter()
            .map(|workflow| workflow.id)
            .collect::<HashSet<_>>();
        assert_eq!(final_in_memory_ids, workflow_ids_from_disk(&workflows_dir));
        assert!(final_in_memory_ids.is_empty());
    }

    #[tokio::test]
    async fn bootstrap_from_store_replaces_registry_with_current_disk_snapshot() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let managed_dir = temp_dir.path().join("managed");
        let reload_dir = temp_dir.path().join("reload");
        let engine = test_engine(managed_dir);
        let existing_workflow = test_workflow();
        let new_workflow = Workflow {
            id: WorkflowId::new(),
            name: "reloaded".to_string(),
            description: "from disk".to_string(),
            steps: vec![WorkflowStep {
                name: "reloaded".to_string(),
                agent: StepAgent::ByName {
                    name: "writer".to_string(),
                },
                prompt_template: "Reloaded {{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 30,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };

        engine
            .register(existing_workflow)
            .await
            .expect("workflow registration should succeed");
        std::fs::create_dir_all(&reload_dir).expect("reload dir should be created");
        std::fs::write(
            reload_dir.join("replacement.json"),
            serde_json::to_string_pretty(&new_workflow).expect("workflow should serialize"),
        )
        .expect("replacement workflow should be written");

        let reloaded = engine
            .bootstrap_from_store(WorkflowDefinitionStore::new(reload_dir))
            .await;
        assert_eq!(reloaded.loaded, 1);
        assert_eq!(reloaded.skipped, 0);

        let workflows = engine.list_workflows().await;
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0], new_workflow);
    }

    #[tokio::test]
    async fn bootstrap_from_store_skips_duplicate_workflow_ids() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let workflows_dir = temp_dir.path().join("duplicates");
        let engine = test_engine(temp_dir.path().join("managed"));
        let workflow = test_workflow();

        std::fs::create_dir_all(&workflows_dir).expect("workflow dir should be created");
        let serialized =
            serde_json::to_string_pretty(&workflow).expect("workflow should serialize");
        std::fs::write(workflows_dir.join("first.json"), &serialized)
            .expect("first workflow file should be written");
        std::fs::write(workflows_dir.join("second.json"), &serialized)
            .expect("second workflow file should be written");

        let reloaded = engine
            .bootstrap_from_store(WorkflowDefinitionStore::new(workflows_dir))
            .await;
        assert_eq!(reloaded.loaded, 1);
        assert_eq!(reloaded.skipped, 1);
        assert_eq!(reloaded.errors[0].level, WorkflowBootstrapErrorLevel::Warn);

        let workflows = engine.list_workflows().await;
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0], workflow);
    }

    #[tokio::test]
    async fn test_execute_pipeline() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let wf = test_workflow();
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(wf_id, "raw data".to_string())
            .await
            .unwrap();

        let sender = |_id: AgentId, msg: String| async move {
            Ok((format!("Processed: {msg}"), 100u64, 50u64))
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.contains("Processed:"));

        let run = engine.get_run(run_id).await.unwrap();
        assert!(matches!(run.state, WorkflowRunState::Completed));
        assert_eq!(run.step_results.len(), 2);
        assert!(run.output.is_some());
    }

    #[tokio::test]
    async fn terminal_state_run_failed_persists_error_json() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow = Workflow {
            id: WorkflowId::new(),
            name: "missing-agent".to_string(),
            description: "fails because the agent cannot be resolved".to_string(),
            steps: vec![WorkflowStep {
                name: "missing".to_string(),
                agent: StepAgent::ByName {
                    name: "ghost-agent".to_string(),
                },
                prompt_template: "{{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 10,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&workflow);
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "input".to_string())
            .await
            .expect("workflow run should be created");

        let error = engine
            .execute_run(
                run_id,
                workflow_ir,
                |_| None,
                |_id: AgentId, _msg: String| async move { Ok(("unused".to_string(), 0, 0)) },
            )
            .await
            .expect_err("workflow should fail when agent resolution fails");

        let record = load_durable_run(&engine, run_id);
        let error_json = record
            .error_json
            .as_deref()
            .expect("failed run should persist error json");
        let payload: serde_json::Value =
            serde_json::from_str(error_json).expect("error json should be valid");

        assert!(error.contains("Agent not found"));
        assert_eq!(record.status, DurableWorkflowRunStatus::Failed);
        assert_eq!(payload["step_id"], "missing");
        assert!(payload["message"]
            .as_str()
            .expect("message should be a string")
            .contains("Agent not found"));
    }

    #[tokio::test]
    async fn terminal_state_run_completed_sets_completed_at() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow = test_workflow();
        let workflow_ir = legacy_ir(&workflow);
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "complete me".to_string())
            .await
            .expect("workflow run should be created");

        let result = engine
            .execute_run(
                run_id,
                workflow_ir,
                mock_resolver,
                |_id: AgentId, msg: String| async move { Ok((format!("Processed: {msg}"), 1, 1)) },
            )
            .await
            .expect("workflow should complete");

        let record = load_durable_run(&engine, run_id);

        assert!(result.contains("Processed:"));
        assert_eq!(record.status, DurableWorkflowRunStatus::Completed);
        assert!(record.completed_at.is_some());
    }

    #[tokio::test]
    async fn waiting_run_transitions_status_to_waiting_signal() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow_id = WorkflowId::new();
        let definition = test_wait_signal_definition(workflow_id);
        let mut registry = WorkflowCompileRegistry::new();
        registry.set_workflows(std::iter::once(definition.id.clone()));
        let workflow_ir = compile_workflow_definition(&definition, &registry)
            .expect("wait signal workflow should compile");
        engine
            .register_workflow_v2_definition(definition.clone(), Vec::<String>::new())
            .await
            .expect("workflow v2 definition should register");
        engine
            .register(Workflow {
                id: workflow_id,
                name: definition.name.clone(),
                description: definition.description.clone(),
                steps: Vec::new(),
                created_at: Utc::now(),
            })
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "waiting input".to_string())
            .await
            .expect("workflow run should be created");

        let result = engine
            .execute_run(
                run_id,
                workflow_ir,
                mock_resolver,
                |_id: AgentId, msg: String| async move { Ok((format!("Processed: {msg}"), 1, 1)) },
            )
            .await
            .expect("workflow should park instead of failing");

        let record = load_durable_run(&engine, run_id);
        let checkpoints = load_durable_checkpoints(&engine, run_id);

        assert_eq!(result, "waiting input");
        assert_eq!(record.status, DurableWorkflowRunStatus::WaitingSignal);
        assert_eq!(record.waiting_kind.as_deref(), Some("signal"));
        assert_eq!(record.waiting_ref.as_deref(), Some("approval"));
        assert!(checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == DurableCheckpointKind::WaitingSignal));
    }

    #[tokio::test]
    async fn eager_consume_fires_when_signal_arrived_before_wait_step() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow_id = WorkflowId::new();
        let definition = test_wait_signal_definition(workflow_id);
        let mut registry = WorkflowCompileRegistry::new();
        registry.set_workflows(std::iter::once(definition.id.clone()));
        let workflow_ir = compile_workflow_definition(&definition, &registry)
            .expect("wait signal workflow should compile");
        engine
            .register_workflow_v2_definition(definition.clone(), Vec::<String>::new())
            .await
            .expect("workflow v2 definition should register");
        engine
            .register(Workflow {
                id: workflow_id,
                name: definition.name.clone(),
                description: definition.description.clone(),
                steps: Vec::new(),
                created_at: Utc::now(),
            })
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "waiting input".to_string())
            .await
            .expect("workflow run should be created");
        engine
            .workflow_stores
            .workflow_signal
            .insert(&openfang_memory::WorkflowSignalRecord {
                signal_id: "signal-approval".to_string(),
                run_id: run_id.to_string(),
                name: "approval".to_string(),
                payload_json: serde_json::json!({ "decision": "approved" }).to_string(),
                source: "schedule".to_string(),
                idempotency_key: "idem-eager".to_string(),
                consumed: false,
                created_at: now_timestamp(),
                consumed_at: None,
            })
            .expect("signal should persist before execution");

        let result = engine
            .execute_run(
                run_id,
                workflow_ir,
                mock_resolver,
                |_id: AgentId, msg: String| async move { Ok((format!("Processed: {msg}"), 1, 1)) },
            )
            .await
            .expect("workflow should consume pre-arrived signal");

        let record = load_durable_run(&engine, run_id);
        let signal = engine
            .workflow_stores
            .workflow_signal
            .find_unconsumed(&run_id.to_string(), "approval")
            .expect("signal lookup should succeed");
        let checkpoints = load_durable_checkpoints(&engine, run_id);

        assert_eq!(result, "waiting input");
        assert_eq!(record.status, DurableWorkflowRunStatus::Completed);
        assert_eq!(record.waiting_kind, None);
        assert_eq!(signal, None);
        assert!(checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == DurableCheckpointKind::SignalConsumed));
        assert!(checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == DurableCheckpointKind::RunResumedFromSignal));
    }

    #[tokio::test]
    async fn signal_submission_persists_and_resumes_waiting_run() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow_id = WorkflowId::new();
        let definition = test_wait_signal_definition(workflow_id);
        let mut registry = WorkflowCompileRegistry::new();
        registry.set_workflows(std::iter::once(definition.id.clone()));
        let workflow_ir = compile_workflow_definition(&definition, &registry)
            .expect("wait signal workflow should compile");
        engine
            .register_workflow_v2_definition(definition.clone(), Vec::<String>::new())
            .await
            .expect("workflow v2 definition should register");
        engine
            .register(Workflow {
                id: workflow_id,
                name: definition.name.clone(),
                description: definition.description.clone(),
                steps: Vec::new(),
                created_at: Utc::now(),
            })
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "waiting input".to_string())
            .await
            .expect("workflow run should be created");
        engine
            .execute_run(
                run_id,
                workflow_ir.clone(),
                mock_resolver,
                |_id: AgentId, msg: String| async move { Ok((format!("Processed: {msg}"), 1, 1)) },
            )
            .await
            .expect("workflow should park");

        let outcome = engine
            .submit_signal(
                run_id,
                "approval".to_string(),
                json!({ "decision": "approved" }),
                "api".to_string(),
                "idem-submit".to_string(),
            )
            .await
            .expect("signal submission should succeed");
        engine
            .resume_after_signal(
                outcome
                    .resume
                    .expect("waiting run should produce resume context"),
                mock_resolver,
                |_id: AgentId, msg: String| async move { Ok((format!("Processed: {msg}"), 1, 1)) },
            )
            .await
            .expect("signal resume should succeed");

        let record = load_durable_run(&engine, run_id);
        let signal_record = engine
            .workflow_stores
            .workflow_signal
            .find_by_id(&outcome.signal.signal_id)
            .expect("signal should load")
            .expect("signal should exist");
        let checkpoints = load_durable_checkpoints(&engine, run_id);

        assert_eq!(record.status, DurableWorkflowRunStatus::Completed);
        assert_eq!(record.waiting_kind, None);
        assert!(signal_record.consumed);
        assert!(checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == DurableCheckpointKind::SignalReceived));
        assert!(checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == DurableCheckpointKind::SignalConsumed));
        assert!(checkpoints
            .iter()
            .any(|checkpoint| checkpoint.kind == DurableCheckpointKind::RunResumedFromSignal));
    }

    #[tokio::test]
    async fn restart_recovery_marks_running_runs_paused_with_checkpoint() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let engine = test_engine(temp_dir.path().to_path_buf());
        let workflow = test_workflow();
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "recover me".to_string())
            .await
            .expect("workflow run should be created");

        engine
            .record_run_started_transition(run_id, Some("analyze"))
            .await
            .expect("run should start");
        engine.clear_run_cache().await;

        let interrupted = engine
            .recover_durable_runs()
            .await
            .expect("durable runs should recover");
        let record = load_durable_run(&engine, run_id);
        let checkpoints = load_durable_checkpoints(&engine, run_id);
        let cached = engine
            .get_run(run_id)
            .await
            .expect("recovered run should be projected into cache");

        assert_eq!(interrupted, 1);
        assert_eq!(record.status, DurableWorkflowRunStatus::Paused);
        assert!(matches!(cached.state, WorkflowRunState::Paused));
        assert!(checkpoints.iter().any(|checkpoint| {
            checkpoint.kind == DurableCheckpointKind::RunRecoveredNeedsResume
                && serde_json::from_str::<serde_json::Value>(&checkpoint.data_json)
                    .map(|value| value == serde_json::json!({ "previous_status": "running" }))
                    .unwrap_or(false)
        }));
    }

    #[tokio::test]
    async fn workflow_v2_ir_executes_end_to_end() {
        let engine = WorkflowEngine::new();
        let workflow_id = WorkflowId::new();
        let definition = test_workflow_v2_definition(workflow_id);
        let mut registry = WorkflowCompileRegistry::new();
        registry.set_agents(["analyst".to_string(), "writer".to_string()]);
        registry.set_workflows(std::iter::once(definition.id.clone()));
        let workflow_ir = compile_workflow_definition(&definition, &registry)
            .expect("workflow v2 definition should compile");
        engine
            .register(Workflow {
                id: workflow_id,
                name: definition.name.clone(),
                description: definition.description.clone(),
                steps: Vec::new(),
                created_at: Utc::now(),
            })
            .await
            .expect("workflow registration should succeed");

        let run_id = engine
            .create_run(workflow_id, r#"{"issue":"raw data"}"#.to_string())
            .await
            .expect("workflow run should be created");

        let sender = |_id: AgentId, msg: String| async move {
            Ok((format!("Processed: {msg}"), 100u64, 50u64))
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await
            .expect("compiled workflow ir should execute");

        assert!(result.contains("Processed:"));

        let run = engine
            .get_run(run_id)
            .await
            .expect("workflow run should still exist");
        assert!(matches!(run.state, WorkflowRunState::Completed));
        assert_eq!(run.step_results.len(), 2);
    }

    #[tokio::test]
    async fn execute_run_rejects_workflow_id_mismatch() {
        let engine = WorkflowEngine::new();
        let workflow = test_workflow();
        let workflow_ir = legacy_ir(&test_workflow());
        let workflow_id = engine
            .register(workflow)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "input".to_string())
            .await
            .expect("workflow run should be created");

        let sender =
            |_id: AgentId, msg: String| async move { Ok((format!("Processed: {msg}"), 1, 1)) };

        let error = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await
            .expect_err("mismatched workflow ids should fail");
        assert!(error.contains("does not match run workflow"));

        let run = engine
            .get_run(run_id)
            .await
            .expect("workflow run should still exist");
        assert!(matches!(run.state, WorkflowRunState::Failed));
    }

    #[tokio::test]
    async fn test_conditional_skip() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "conditional-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "first".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "only-if-error".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Fix: {{input}}".to_string(),
                    mode: StepMode::Conditional {
                        condition: "ERROR".to_string(),
                    },
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(wf_id, "all good".to_string())
            .await
            .unwrap();

        let sender =
            |_id: AgentId, msg: String| async move { Ok((format!("OK: {msg}"), 10u64, 5u64)) };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        // Only 1 step executed (conditional was skipped)
        assert_eq!(run.step_results.len(), 1);
    }

    #[tokio::test]
    async fn test_conditional_executes() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "conditional-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "first".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "only-if-error".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Fix: {{input}}".to_string(),
                    mode: StepMode::Conditional {
                        condition: "ERROR".to_string(),
                    },
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        // This sender returns output containing "ERROR"
        let sender = |_id: AgentId, _msg: String| async move {
            Ok(("Found an ERROR in the data".to_string(), 10u64, 5u64))
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        // Both steps executed
        assert_eq!(run.step_results.len(), 2);
    }

    #[tokio::test]
    async fn test_loop_until_condition() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "loop-test".to_string(),
            description: "".to_string(),
            steps: vec![WorkflowStep {
                name: "refine".to_string(),
                agent: StepAgent::ByName {
                    name: "a".to_string(),
                },
                prompt_template: "Refine: {{input}}".to_string(),
                mode: StepMode::Loop {
                    max_iterations: 5,
                    until: "DONE".to_string(),
                },
                timeout_secs: 10,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine.create_run(wf_id, "draft".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, _msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n >= 2 {
                    Ok(("Result: DONE".to_string(), 10u64, 5u64))
                } else {
                    Ok(("Still working...".to_string(), 10u64, 5u64))
                }
            }
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());
        assert!(result.unwrap().contains("DONE"));
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_loop_max_iterations() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "loop-max-test".to_string(),
            description: "".to_string(),
            steps: vec![WorkflowStep {
                name: "refine".to_string(),
                agent: StepAgent::ByName {
                    name: "a".to_string(),
                },
                prompt_template: "{{input}}".to_string(),
                mode: StepMode::Loop {
                    max_iterations: 3,
                    until: "NEVER_MATCH".to_string(),
                },
                timeout_secs: 10,
                error_mode: ErrorMode::Fail,
                output_var: None,
            }],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let sender = |_id: AgentId, _msg: String| async move {
            Ok(("iteration output".to_string(), 10u64, 5u64))
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        assert_eq!(run.step_results.len(), 3); // max_iterations
    }

    #[tokio::test]
    async fn test_error_mode_skip() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "skip-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "will-fail".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Skip,
                    output_var: None,
                },
                WorkflowStep {
                    name: "succeeds".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, _msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 0 {
                    Err("simulated error".to_string())
                } else {
                    Ok(("success".to_string(), 10u64, 5u64))
                }
            }
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());

        let run = engine.get_run(run_id).await.unwrap();
        // Only 1 step result (the first was skipped due to error)
        assert_eq!(run.step_results.len(), 1);
        assert!(matches!(run.state, WorkflowRunState::Completed));
    }

    #[tokio::test]
    async fn test_error_mode_retry() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "retry-test".to_string(),
            description: "".to_string(),
            steps: vec![WorkflowStep {
                name: "flaky".to_string(),
                agent: StepAgent::ByName {
                    name: "a".to_string(),
                },
                prompt_template: "{{input}}".to_string(),
                mode: StepMode::Sequential,
                timeout_secs: 10,
                error_mode: ErrorMode::Retry { max_retries: 2 },
                output_var: None,
            }],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, _msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n < 2 {
                    Err("transient error".to_string())
                } else {
                    Ok(("finally worked".to_string(), 10u64, 5u64))
                }
            }
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "finally worked");
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_output_variables() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "vars-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "produce".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: Some("first_result".to_string()),
                },
                WorkflowStep {
                    name: "transform".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "{{input}}".to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: Some("second_result".to_string()),
                },
                WorkflowStep {
                    name: "combine".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "First: {{first_result}} | Second: {{second_result}}"
                        .to_string(),
                    mode: StepMode::Sequential,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine.create_run(wf_id, "start".to_string()).await.unwrap();

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let cc = call_count.clone();
        let sender = move |_id: AgentId, msg: String| {
            let cc = cc.clone();
            async move {
                let n = cc.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                match n {
                    0 => Ok(("alpha".to_string(), 10u64, 5u64)),
                    1 => Ok(("beta".to_string(), 10u64, 5u64)),
                    _ => Ok((format!("Combined: {msg}"), 10u64, 5u64)),
                }
            }
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());
        let output = result.unwrap();
        // The third step receives "First: alpha | Second: beta" as its prompt
        assert!(output.contains("First: alpha"));
        assert!(output.contains("Second: beta"));
    }

    #[tokio::test]
    async fn test_fan_out_parallel() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "fanout-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "task-a".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Task A: {{input}}".to_string(),
                    mode: StepMode::FanOut,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "task-b".to_string(),
                    agent: StepAgent::ByName {
                        name: "b".to_string(),
                    },
                    prompt_template: "Task B: {{input}}".to_string(),
                    mode: StepMode::FanOut,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "collect".to_string(),
                    agent: StepAgent::ByName {
                        name: "c".to_string(),
                    },
                    prompt_template: "unused".to_string(),
                    mode: StepMode::Collect,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let wf_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine.create_run(wf_id, "data".to_string()).await.unwrap();

        let sender =
            |_id: AgentId, msg: String| async move { Ok((format!("Done: {msg}"), 10u64, 5u64)) };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await;
        assert!(result.is_ok());

        let output = result.unwrap();
        // Collect step joins all outputs
        assert!(output.contains("Done: Task A"));
        assert!(output.contains("Done: Task B"));
        assert!(output.contains("---"));
    }

    #[tokio::test]
    async fn test_fan_out_error_mode_skip() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "fanout-skip-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "task-a".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Task A: {{input}}".to_string(),
                    mode: StepMode::FanOut,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Skip,
                    output_var: None,
                },
                WorkflowStep {
                    name: "task-b".to_string(),
                    agent: StepAgent::ByName {
                        name: "b".to_string(),
                    },
                    prompt_template: "Task B: {{input}}".to_string(),
                    mode: StepMode::FanOut,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
                WorkflowStep {
                    name: "collect".to_string(),
                    agent: StepAgent::ByName {
                        name: "c".to_string(),
                    },
                    prompt_template: "unused".to_string(),
                    mode: StepMode::Collect,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let workflow_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "data".to_string())
            .await
            .expect("workflow run should be created");

        let sender = |_id: AgentId, msg: String| async move {
            if msg.contains("Task A") {
                Err("simulated fan-out failure".to_string())
            } else {
                Ok((format!("Done: {msg}"), 10u64, 5u64))
            }
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await
            .expect("fan-out skip mode should continue");

        assert!(result.contains("Done: Task B"));

        let run = engine
            .get_run(run_id)
            .await
            .expect("workflow run should still exist");
        assert!(matches!(run.state, WorkflowRunState::Completed));
        assert_eq!(run.step_results.len(), 1);
    }

    #[tokio::test]
    async fn test_fan_out_error_mode_retry() {
        let engine = WorkflowEngine::new();
        let wf = Workflow {
            id: WorkflowId::new(),
            name: "fanout-retry-test".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    name: "task-a".to_string(),
                    agent: StepAgent::ByName {
                        name: "a".to_string(),
                    },
                    prompt_template: "Task A: {{input}}".to_string(),
                    mode: StepMode::FanOut,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Retry { max_retries: 1 },
                    output_var: None,
                },
                WorkflowStep {
                    name: "collect".to_string(),
                    agent: StepAgent::ByName {
                        name: "c".to_string(),
                    },
                    prompt_template: "unused".to_string(),
                    mode: StepMode::Collect,
                    timeout_secs: 10,
                    error_mode: ErrorMode::Fail,
                    output_var: None,
                },
            ],
            created_at: Utc::now(),
        };
        let workflow_ir = legacy_ir(&wf);
        let workflow_id = engine
            .register(wf)
            .await
            .expect("workflow registration should succeed");
        let run_id = engine
            .create_run(workflow_id, "data".to_string())
            .await
            .expect("workflow run should be created");

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let observed_calls = Arc::clone(&call_count);
        let sender = move |_id: AgentId, _msg: String| {
            let call_count = Arc::clone(&call_count);
            async move {
                let attempt = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if attempt == 0 {
                    Err("transient fan-out failure".to_string())
                } else {
                    Ok(("fan-out recovered".to_string(), 10u64, 5u64))
                }
            }
        };

        let result = engine
            .execute_run(run_id, workflow_ir, mock_resolver, sender)
            .await
            .expect("fan-out retry mode should recover");

        assert_eq!(result, "fan-out recovered");
        assert_eq!(observed_calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_expand_variables() {
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), "Alice".to_string());
        vars.insert("task".to_string(), "code review".to_string());

        let template = WorkflowEngine::legacy_prompt_to_template(
            "Hello {{name}}, please do {{task}} on {{input}}",
        );
        let result = WorkflowEngine::render_template(&template, "main.rs", &vars);
        assert_eq!(result, "Hello Alice, please do code review on main.rs");
    }

    #[tokio::test]
    async fn test_error_mode_serialization() {
        let fail_json = serde_json::to_string(&ErrorMode::Fail).unwrap();
        assert_eq!(fail_json, "\"fail\"");

        let skip_json = serde_json::to_string(&ErrorMode::Skip).unwrap();
        assert_eq!(skip_json, "\"skip\"");

        let retry_json = serde_json::to_string(&ErrorMode::Retry { max_retries: 3 }).unwrap();
        let retry: ErrorMode = serde_json::from_str(&retry_json).unwrap();
        assert!(matches!(retry, ErrorMode::Retry { max_retries: 3 }));
    }

    #[tokio::test]
    async fn test_step_mode_conditional_serialization() {
        let mode = StepMode::Conditional {
            condition: "error".to_string(),
        };
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: StepMode = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, StepMode::Conditional { condition } if condition == "error"));
    }

    #[tokio::test]
    async fn test_step_mode_loop_serialization() {
        let mode = StepMode::Loop {
            max_iterations: 5,
            until: "done".to_string(),
        };
        let json = serde_json::to_string(&mode).unwrap();
        let parsed: StepMode = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, StepMode::Loop { max_iterations: 5, until } if until == "done"));
    }
}
