//! Typed `compozy.db` repositories for durable workflow runtime state.

use crate::artifact::ArtifactRepository;
use crate::dispatch::{
    list_dispatch_summaries_by_run, resolve_update_conflict as resolve_dispatch_update_conflict,
    update_dispatch_row, DispatchRecord, DispatchSummaryRecord, SqliteDispatchRepository,
};
use crate::doc::DocRepository;
use crate::hitl::{
    ensure_pending_transition, insert_hitl_request, load_required_hitl_request, next_sequence_no,
    HitlRecord, HitlStatus, NewHitlRequest, SqliteHitlRepository,
};
use crate::looper::{LooperRunRepository, LooperSubtaskRepository};
use crate::pack::PackRepository;
use crate::task::{SubtaskRepository, TaskRepository};
use chrono::Utc;
use openfang_types::error::OpenFangError;
use rusqlite::{params, Connection, ErrorCode, OptionalExtension, TransactionBehavior};
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

/// SQL for migration `0002_workflow_run_core`.
pub const WORKFLOW_RUN_CORE_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260321_002_workflow_run_core.sql");

/// SQL for migration `0003_workflow_checkpoint`.
pub const WORKFLOW_CHECKPOINT_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260321_003_workflow_checkpoint.sql");

/// SQL for migration `0004_workflow_signal`.
pub const WORKFLOW_SIGNAL_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260321_004_workflow_signal.sql");

/// SQL for migration `0005_workflow_runtime_durability`.
pub const WORKFLOW_RUNTIME_DURABILITY_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260323_005_workflow_runtime_durability.sql");

/// SQL for migration `0006_workflow_signal_waiting_state`.
pub const WORKFLOW_SIGNAL_WAITING_STATE_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260323_006_workflow_signal_waiting_state.sql");

/// SQL for migration `0007_workflow_run_control_plane`.
pub const WORKFLOW_RUN_CONTROL_PLANE_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260323_007_workflow_run_control_plane.sql");

/// SQL for migration `0014_workflow_checkpoint_retention`.
pub const WORKFLOW_CHECKPOINT_RETENTION_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260326_014_workflow_checkpoint_retention.sql");

/// Shared `compozy.db` repository handles.
#[derive(Clone)]
pub struct WorkflowStoreSet {
    connection: Arc<Mutex<Connection>>,
    /// Repository for durable workflow runs.
    pub workflow_run: WorkflowRunRepository,
    /// Repository for durable workflow checkpoints.
    pub workflow_checkpoint: WorkflowCheckpointRepository,
    /// Repository for durable workflow signals.
    pub workflow_signal: WorkflowSignalRepository,
    /// Repository for durable agent dispatch state.
    pub dispatch: SqliteDispatchRepository,
    /// Repository for durable HITL request state.
    pub hitl: SqliteHitlRepository,
    /// Repository for durable looper runs.
    pub looper_run: LooperRunRepository,
    /// Repository for durable looper subtask execution views.
    pub looper_subtask: LooperSubtaskRepository,
    /// Repository for durable task state.
    pub task: TaskRepository,
    /// Repository for durable subtask state.
    pub subtask: SubtaskRepository,
    /// Repository for durable artifact state.
    pub artifact: ArtifactRepository,
    /// Repository for durable document state.
    pub doc: DocRepository,
    /// Repository for durable pack state.
    pub pack: PackRepository,
}

impl WorkflowStoreSet {
    /// Create the full workflow repository set from a shared `compozy.db`
    /// connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            connection: Arc::clone(&conn),
            workflow_run: WorkflowRunRepository::new(Arc::clone(&conn)),
            workflow_checkpoint: WorkflowCheckpointRepository::new(Arc::clone(&conn)),
            dispatch: SqliteDispatchRepository::new(Arc::clone(&conn)),
            hitl: SqliteHitlRepository::new(Arc::clone(&conn)),
            looper_run: LooperRunRepository::new(Arc::clone(&conn)),
            looper_subtask: LooperSubtaskRepository::new(Arc::clone(&conn)),
            task: TaskRepository::new(Arc::clone(&conn)),
            subtask: SubtaskRepository::new(Arc::clone(&conn)),
            artifact: ArtifactRepository::new(Arc::clone(&conn)),
            doc: DocRepository::new(Arc::clone(&conn)),
            pack: PackRepository::new(Arc::clone(&conn)),
            workflow_signal: WorkflowSignalRepository::new(conn),
        }
    }

    /// Return the underlying `compozy.db` handle for health probes.
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.connection)
    }
}

pub struct SubmittedSignalResume<'a> {
    pub signal: &'a WorkflowSignalRecord,
    pub received_checkpoint: &'a WorkflowCheckpointRecord,
    pub consumed_checkpoint: &'a WorkflowCheckpointRecord,
    pub resumed_checkpoint: &'a WorkflowCheckpointRecord,
    pub consumed_at: &'a str,
}

pub struct HitlAnswerTransition<'a> {
    pub hitl_request_id: &'a str,
    pub response_json: &'a serde_json::Value,
    pub answered_at: &'a chrono::DateTime<Utc>,
}

/// Typed failures from the workflow repository layer.
#[derive(Debug, Error)]
pub enum WorkflowStoreError {
    /// Failed to acquire the `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Dispatch repository returned a typed durable dispatch error.
    #[error(transparent)]
    Dispatch(#[from] crate::dispatch::DispatchStoreError),
    /// HITL repository returned a typed durable HITL error.
    #[error(transparent)]
    Hitl(#[from] crate::hitl::HitlStoreError),
    /// The requested workflow run does not exist.
    #[error("workflow run '{run_id}' was not found")]
    RunNotFound { run_id: String },
    /// The requested workflow signal does not exist.
    #[error("workflow signal '{signal_id}' was not found")]
    SignalNotFound { signal_id: String },
    /// The requested workflow signal was already consumed.
    #[error("workflow signal '{signal_id}' was already consumed")]
    SignalAlreadyConsumed { signal_id: String },
    /// A signal with the same idempotency key already exists for the run.
    #[error(
        "workflow signal for run '{run_id}' already exists for idempotency key '{idempotency_key}'"
    )]
    SignalAlreadyExistsForIdempotency {
        /// Run identifier.
        run_id: String,
        /// Client-supplied idempotency key.
        idempotency_key: String,
    },
    /// One list/pruning query parameter could not be represented safely.
    #[error("invalid workflow query parameter '{field}' with value '{value}'")]
    InvalidListQuery {
        /// Invalid field name.
        field: &'static str,
        /// Original value.
        value: String,
    },
    /// A stored workflow status string was invalid.
    #[error("invalid workflow run status '{status}'")]
    InvalidRunStatus { status: String },
    /// A stored checkpoint kind string was invalid.
    #[error("invalid workflow checkpoint kind '{kind}'")]
    InvalidCheckpointKind { kind: String },
    /// A transition attempted to update a run from an unexpected state.
    #[error("workflow run '{run_id}' expected current state one of [{expected}], got '{actual}'")]
    UnexpectedRunState {
        /// Run identifier.
        run_id: String,
        /// Expected current states.
        expected: String,
        /// Actual current state found in the database.
        actual: String,
    },
}

impl From<WorkflowStoreError> for OpenFangError {
    fn from(error: WorkflowStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

/// Durable workflow status stored in `workflow_run.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowRunStatus {
    /// The run has been created but execution has not started yet.
    Pending,
    /// The run is actively executing.
    Running,
    /// The run is durably parked on an external wait condition.
    WaitingSignal,
    /// The run is durably parked on a human-in-the-loop request.
    WaitingHitl,
    /// The run is paused and requires an explicit resume action.
    Paused,
    /// The run completed successfully.
    Completed,
    /// The run failed.
    Failed,
    /// The run was cancelled.
    Cancelled,
}

impl WorkflowRunStatus {
    /// Return the SQLite string encoding for the status.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::WaitingSignal => "waiting_signal",
            Self::WaitingHitl => "waiting_hitl",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse_db_text(value: &str) -> Result<Self, WorkflowStoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "waiting" | "waiting_signal" => Ok(Self::WaitingSignal),
            "waiting_hitl" => Ok(Self::WaitingHitl),
            "paused" | "interrupted" => Ok(Self::Paused),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(WorkflowStoreError::InvalidRunStatus {
                status: other.to_string(),
            }),
        }
    }

    /// Whether the status is terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl std::fmt::Display for WorkflowRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for WorkflowRunStatus {
    type Err = WorkflowStoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value)
    }
}

/// Durable workflow checkpoint kinds stored in `workflow_checkpoint.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointKind {
    /// Initial checkpoint emitted after the run row is created.
    RunCreated,
    /// The run started execution.
    RunStarted,
    /// A workflow step started execution.
    StepStarted,
    /// A workflow step completed successfully.
    StepCompleted,
    /// A workflow step failed.
    StepFailed,
    /// A workflow step was skipped.
    StepSkipped,
    /// A durable signal was received and consumed.
    SignalReceived,
    /// A durable signal was consumed for workflow progression.
    SignalConsumed,
    /// A waiting run resumed because a signal was accepted.
    RunResumedFromSignal,
    /// The run completed successfully.
    RunCompleted,
    /// The run failed.
    RunFailed,
    /// The run was cancelled.
    RunCancelled,
    /// Legacy Task 16 checkpoint kind kept for migration compatibility.
    RunInterrupted,
    /// Legacy Task 9 checkpoint kind kept for migration compatibility.
    StepSelected,
    /// The run is durably waiting on a signal.
    WaitingSignal,
    /// The run requested a human-in-the-loop clarification.
    HitlRequested,
    /// The run was paused by an explicit control-plane action.
    RunPaused,
    /// The run was resumed by an explicit control-plane action.
    RunResumed,
    /// The run was recovered during startup and now needs manual resume.
    RunRecoveredNeedsResume,
}

impl CheckpointKind {
    /// Return the SQLite string encoding for the checkpoint kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RunCreated => "run_created",
            Self::RunStarted => "run_started",
            Self::StepStarted => "step_started",
            Self::StepCompleted => "step_completed",
            Self::StepFailed => "step_failed",
            Self::StepSkipped => "step_skipped",
            Self::SignalReceived => "signal_received",
            Self::SignalConsumed => "signal_consumed",
            Self::RunResumedFromSignal => "run_resumed_from_signal",
            Self::RunCompleted => "run_completed",
            Self::RunFailed => "run_failed",
            Self::RunCancelled => "run_cancelled",
            Self::RunInterrupted => "run_interrupted",
            Self::StepSelected => "step_selected",
            Self::WaitingSignal => "waiting_signal",
            Self::HitlRequested => "hitl_requested",
            Self::RunPaused => "run_paused",
            Self::RunResumed => "run_resumed",
            Self::RunRecoveredNeedsResume => "run_recovered_needs_resume",
        }
    }

    fn parse_db_text(value: &str) -> Result<Self, WorkflowStoreError> {
        match value {
            "run_created" => Ok(Self::RunCreated),
            "run_started" => Ok(Self::RunStarted),
            "step_started" => Ok(Self::StepStarted),
            "step_completed" => Ok(Self::StepCompleted),
            "step_failed" => Ok(Self::StepFailed),
            "step_skipped" => Ok(Self::StepSkipped),
            "signal_received" => Ok(Self::SignalReceived),
            "signal_consumed" => Ok(Self::SignalConsumed),
            "run_resumed_from_signal" => Ok(Self::RunResumedFromSignal),
            "run_completed" => Ok(Self::RunCompleted),
            "run_failed" => Ok(Self::RunFailed),
            "run_cancelled" => Ok(Self::RunCancelled),
            "run_interrupted" => Ok(Self::RunInterrupted),
            "step_selected" => Ok(Self::StepSelected),
            "waiting_signal" => Ok(Self::WaitingSignal),
            "hitl_requested" => Ok(Self::HitlRequested),
            "run_paused" => Ok(Self::RunPaused),
            "run_resumed" => Ok(Self::RunResumed),
            "run_recovered_needs_resume" => Ok(Self::RunRecoveredNeedsResume),
            other => Err(WorkflowStoreError::InvalidCheckpointKind {
                kind: other.to_string(),
            }),
        }
    }
}

impl std::fmt::Display for CheckpointKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for CheckpointKind {
    type Err = WorkflowStoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value)
    }
}

/// Durable workflow run record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunRecord {
    /// Stable run identifier.
    pub run_id: String,
    /// Stable workflow definition identifier.
    pub workflow_id: String,
    /// Compiled workflow version.
    pub workflow_version: String,
    /// Current durable run status.
    pub status: WorkflowRunStatus,
    /// Workflow input payload encoded as JSON text.
    pub input_json: String,
    /// Durable workflow variables encoded as JSON text.
    pub vars_json: String,
    /// Currently active step identifier, if any.
    pub current_step_id: Option<String>,
    /// Waiting state kind, if the run is blocked externally.
    pub waiting_kind: Option<String>,
    /// Waiting state reference, if any.
    pub waiting_ref: Option<String>,
    /// Active dispatch identifier, if any.
    pub active_dispatch_id: Option<String>,
    /// Active HITL request identifier, if any.
    pub active_hitl_request_id: Option<String>,
    /// Labels encoded as JSON text.
    pub labels_json: String,
    /// Metadata encoded as JSON text.
    pub metadata_json: String,
    /// Serialized error payload, if any.
    pub error_json: Option<String>,
    /// Run creation timestamp.
    pub started_at: String,
    /// Last update timestamp.
    pub updated_at: String,
    /// Completion timestamp, if the run finished.
    pub completed_at: Option<String>,
}

/// Durable workflow checkpoint record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowCheckpointRecord {
    /// Stable checkpoint identifier.
    pub checkpoint_id: String,
    /// Owning run identifier.
    pub run_id: String,
    /// Associated step identifier, if any.
    pub step_id: Option<String>,
    /// Checkpoint kind.
    pub kind: CheckpointKind,
    /// Checkpoint payload encoded as JSON text.
    pub data_json: String,
    /// Creation timestamp.
    pub created_at: String,
}

/// Durable workflow signal record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowSignalRecord {
    /// Stable signal identifier.
    pub signal_id: String,
    /// Owning run identifier.
    pub run_id: String,
    /// Signal name.
    pub name: String,
    /// Signal payload encoded as JSON text.
    pub payload_json: String,
    /// Signal source, such as `api`, `trigger`, or `schedule`.
    pub source: String,
    /// Client-supplied idempotency key scoped to the run.
    pub idempotency_key: String,
    /// Whether the signal was consumed.
    pub consumed: bool,
    /// Creation timestamp.
    pub created_at: String,
    /// Consumption timestamp, if already consumed.
    pub consumed_at: Option<String>,
}

/// Filter arguments for durable run listing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkflowRunListQuery {
    /// Filter by workflow definition ID.
    pub workflow_id: Option<String>,
    /// Filter by status.
    pub status: Option<WorkflowRunStatus>,
    /// Filter by waiting kind.
    pub waiting_kind: Option<String>,
    /// Filter by label membership.
    pub label: Option<String>,
    /// Free-text search over run and workflow identifiers plus JSON payloads.
    pub search: Option<String>,
}

/// One run downgraded during startup recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunRecoveryRecord {
    /// Run identifier that was recovered.
    pub run_id: String,
    /// Previous status observed before the downgrade.
    pub previous_status: WorkflowRunStatus,
}

/// Durable dispatch summary for one run.
pub type WorkflowDispatchSummaryRecord = DispatchSummaryRecord;

/// Repository for `workflow_run`.
#[derive(Clone)]
pub struct WorkflowRunRepository {
    conn: Arc<Mutex<Connection>>,
}

/// Repository for `workflow_checkpoint`.
#[derive(Clone)]
pub struct WorkflowCheckpointRepository {
    conn: Arc<Mutex<Connection>>,
}

/// Repository for `workflow_signal`.
#[derive(Clone)]
pub struct WorkflowSignalRepository {
    conn: Arc<Mutex<Connection>>,
}

/// Backward-compatible alias kept while the rest of the workspace migrates to
/// the repository terminology.
pub type WorkflowRunStore = WorkflowRunRepository;
/// Backward-compatible alias kept while the rest of the workspace migrates to
/// the repository terminology.
pub type WorkflowCheckpointStore = WorkflowCheckpointRepository;
/// Backward-compatible alias kept while the rest of the workspace migrates to
/// the repository terminology.
pub type WorkflowSignalStore = WorkflowSignalRepository;

impl WorkflowRunRepository {
    /// Create a run repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable workflow run row.
    pub fn insert_run(&self, record: &WorkflowRunRecord) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        insert_workflow_run(&conn, record)?;
        Ok(())
    }

    /// Replace the durable workflow run row with the full provided record.
    pub fn replace_run(&self, record: &WorkflowRunRecord) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = update_workflow_run_record(&conn, record, None)?;
        ensure_row_updated(rows, &record.run_id)?;
        Ok(())
    }

    /// Load one durable workflow run by ID.
    pub fn find_by_id(
        &self,
        run_id: &str,
    ) -> Result<Option<WorkflowRunRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_workflow_run(&conn, run_id)
    }

    /// List durable workflow runs using optional in-memory filtering over the
    /// canonical database rows.
    pub fn list_runs(
        &self,
        query: &WorkflowRunListQuery,
    ) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut records = list_all_workflow_runs(&conn)?;

        if let Some(workflow_id) = query.workflow_id.as_deref() {
            records.retain(|record| record.workflow_id == workflow_id);
        }

        if let Some(status) = query.status {
            records.retain(|record| record.status == status);
        }

        if let Some(waiting_kind) = query.waiting_kind.as_deref() {
            records.retain(|record| record.waiting_kind.as_deref() == Some(waiting_kind));
        }

        if let Some(label) = query.label.as_deref() {
            records.retain(|record| {
                serde_json::from_str::<Vec<String>>(&record.labels_json)
                    .map(|labels| labels.iter().any(|candidate| candidate == label))
                    .unwrap_or(false)
            });
        }

        if let Some(search) = query.search.as_deref() {
            let needle = search.to_lowercase();
            records.retain(|record| {
                record.run_id.to_lowercase().contains(&needle)
                    || record.workflow_id.to_lowercase().contains(&needle)
                    || record.input_json.to_lowercase().contains(&needle)
                    || record.metadata_json.to_lowercase().contains(&needle)
            });
        }

        Ok(records)
    }

    /// List durable workflow runs using the canonical database-backed query
    /// parameters.
    pub fn list(
        &self,
        query: &WorkflowRunListQuery,
    ) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
        self.list_runs(query)
    }

    /// List durable runs for one workflow definition.
    pub fn list_for_workflow(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
        self.list_runs(&WorkflowRunListQuery {
            workflow_id: Some(workflow_id.to_string()),
            ..WorkflowRunListQuery::default()
        })
    }

    /// Append a checkpoint row for an existing run.
    pub fn insert_checkpoint(
        &self,
        checkpoint: &WorkflowCheckpointRecord,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        insert_checkpoint(&conn, checkpoint)?;
        Ok(())
    }

    /// List checkpoints for one run ordered by creation time.
    pub fn find_checkpoints_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowCheckpointRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_run_checkpoints(&conn, run_id)
    }

    /// Update one durable workflow run status and write the matching checkpoint
    /// in the same transaction.
    pub fn update_status(
        &self,
        current: &WorkflowRunRecord,
        next: &WorkflowRunRecord,
        checkpoint: &WorkflowCheckpointRecord,
    ) -> Result<WorkflowRunRecord, WorkflowStoreError> {
        validate_status_update_transition(current, next, checkpoint)?;
        self.persist_transition(current, next, checkpoint)?;
        Ok(next.clone())
    }

    /// Return the canonical set of non-terminal runs used for restart
    /// recovery.
    pub fn list_non_terminal(&self) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn.prepare(
            "SELECT
                run_id,
                workflow_id,
                workflow_version,
                status,
                input_json,
                vars_json,
                current_step_id,
                waiting_kind,
                waiting_ref,
                active_dispatch_id,
                active_hitl_request_id,
                labels_json,
                metadata_json,
                error_json,
                started_at,
                updated_at,
                completed_at
             FROM workflow_run
             WHERE status IN ('pending', 'running', 'waiting_signal', 'waiting_hitl', 'paused')
             ORDER BY updated_at ASC, run_id ASC",
        )?;
        let rows = stmt.query_map([], read_workflow_run_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// Atomically downgrade all `running` rows to `paused` and append one
    /// recovery checkpoint per affected run.
    pub fn recover_running_runs(
        &self,
    ) -> Result<Vec<WorkflowRunRecoveryRecord>, WorkflowStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        recover_running_runs_with(&mut conn, |run, index| {
            format!("chk-recovered-{}-{index}", run.run_id)
        })
    }

    /// List durable dispatch summaries for one run. Returns an empty list until
    /// the later dispatch schema lands in `compozy.db`.
    pub fn list_dispatches_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowDispatchSummaryRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_run_dispatches(&conn, run_id)
    }

    /// Append a checkpoint row and then replace the durable run row inside one
    /// SQLite transaction.
    pub fn persist_transition(
        &self,
        current: &WorkflowRunRecord,
        next: &WorkflowRunRecord,
        checkpoint: &WorkflowCheckpointRecord,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        insert_checkpoint(&transaction, checkpoint)?;
        let rows = update_workflow_run_record(&transaction, next, Some(current.status))?;

        if rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current.run_id,
                Some(current.status),
            ));
        }

        transaction.commit()?;
        Ok(())
    }

    /// Insert an unconsumed signal row and append a `signal_received`
    /// checkpoint inside one SQLite transaction.
    pub fn persist_signal_submission(
        &self,
        signal: &WorkflowSignalRecord,
        received_checkpoint: &WorkflowCheckpointRecord,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        insert_signal_row(&transaction, signal)?;
        insert_checkpoint(&transaction, received_checkpoint)?;
        transaction.commit()?;
        Ok(())
    }

    /// Insert, consume, and resume from a just-submitted signal inside one
    /// SQLite transaction.
    pub fn persist_signal_submission_and_resume(
        &self,
        current: &WorkflowRunRecord,
        next: &WorkflowRunRecord,
        submitted_signal: SubmittedSignalResume<'_>,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        insert_signal_row(&transaction, submitted_signal.signal)?;
        insert_checkpoint(&transaction, submitted_signal.received_checkpoint)?;
        consume_signal_row(
            &transaction,
            &submitted_signal.signal.signal_id,
            submitted_signal.consumed_at,
        )?;
        insert_checkpoint(&transaction, submitted_signal.consumed_checkpoint)?;
        insert_checkpoint(&transaction, submitted_signal.resumed_checkpoint)?;
        let rows = update_workflow_run_record(&transaction, next, Some(current.status))?;

        if rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current.run_id,
                Some(current.status),
            ));
        }

        transaction.commit()?;
        Ok(())
    }

    /// Mark a pre-existing signal consumed and resume the run inside one SQLite
    /// transaction.
    pub fn persist_signal_resume(
        &self,
        current: &WorkflowRunRecord,
        next: &WorkflowRunRecord,
        consumed_checkpoint: &WorkflowCheckpointRecord,
        resumed_checkpoint: &WorkflowCheckpointRecord,
        signal_id: &str,
        consumed_at: &str,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        consume_signal_row(&transaction, signal_id, consumed_at)?;
        insert_checkpoint(&transaction, consumed_checkpoint)?;
        insert_checkpoint(&transaction, resumed_checkpoint)?;
        let rows = update_workflow_run_record(&transaction, next, Some(current.status))?;

        if rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current.run_id,
                Some(current.status),
            ));
        }

        transaction.commit()?;
        Ok(())
    }

    /// Create a pending HITL request, park the dispatch, and update the run in
    /// one SQLite transaction.
    pub fn persist_hitl_pause(
        &self,
        current_run: &WorkflowRunRecord,
        next_run: &WorkflowRunRecord,
        current_dispatch: &DispatchRecord,
        next_dispatch: &DispatchRecord,
        request: &NewHitlRequest,
        checkpoint: &WorkflowCheckpointRecord,
    ) -> Result<HitlRecord, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;

        let hitl_record = HitlRecord {
            hitl_request_id: request.hitl_request_id.clone(),
            run_id: request.run_id.clone(),
            step_id: request.step_id.clone(),
            dispatch_id: request.dispatch_id.clone(),
            kind: request.kind,
            status: HitlStatus::Pending,
            question: request.question.clone(),
            context_json: request.context_json.clone(),
            response_json: None,
            sequence_no: next_sequence_no(
                &transaction,
                &request.run_id,
                &request.step_id,
                request.dispatch_id.as_deref(),
            )?,
            created_at: request.created_at,
            answered_at: None,
            timeout_at: request.timeout_at,
        };

        insert_hitl_request(&transaction, &hitl_record)?;
        insert_checkpoint(&transaction, checkpoint)?;

        let dispatch_rows = update_dispatch_row(&transaction, current_dispatch, next_dispatch)?;
        if dispatch_rows == 0 {
            return Err(resolve_dispatch_update_conflict(
                &transaction,
                &current_dispatch.dispatch_id,
                current_dispatch.status,
            )
            .into());
        }

        let run_rows =
            update_workflow_run_record(&transaction, next_run, Some(current_run.status))?;
        if run_rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current_run.run_id,
                Some(current_run.status),
            ));
        }

        transaction.commit()?;
        Ok(hitl_record)
    }

    /// Answer a pending HITL request, resume the dispatch, and clear the run's
    /// active HITL marker inside one SQLite transaction.
    pub fn persist_hitl_answer_and_resume(
        &self,
        current_run: &WorkflowRunRecord,
        next_run: &WorkflowRunRecord,
        current_dispatch: &DispatchRecord,
        next_dispatch: &DispatchRecord,
        answer: HitlAnswerTransition<'_>,
    ) -> Result<HitlRecord, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        let current_hitl = load_required_hitl_request(&transaction, answer.hitl_request_id)?;
        ensure_pending_transition(&current_hitl, HitlStatus::Answered)?;

        let answered_timestamp = answer.answered_at.to_owned();
        let answered_at = answered_timestamp.to_rfc3339();
        let hitl_rows = transaction.execute(
            "UPDATE hitl_request
             SET status = ?1,
                 response_json = ?2,
                 answered_at = ?3
             WHERE hitl_request_id = ?4",
            params![
                HitlStatus::Answered.as_str(),
                serde_json::to_string(answer.response_json)?,
                answered_at.as_str(),
                answer.hitl_request_id,
            ],
        )?;
        if hitl_rows == 0 {
            return Err(WorkflowStoreError::Hitl(
                crate::hitl::HitlStoreError::HitlRequestNotFound {
                    hitl_request_id: answer.hitl_request_id.to_string(),
                },
            ));
        }

        let dispatch_rows = update_dispatch_row(&transaction, current_dispatch, next_dispatch)?;
        if dispatch_rows == 0 {
            return Err(resolve_dispatch_update_conflict(
                &transaction,
                &current_dispatch.dispatch_id,
                current_dispatch.status,
            )
            .into());
        }

        let run_rows =
            update_workflow_run_record(&transaction, next_run, Some(current_run.status))?;
        if run_rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current_run.run_id,
                Some(current_run.status),
            ));
        }

        transaction.commit()?;

        Ok(HitlRecord {
            status: HitlStatus::Answered,
            response_json: Some(answer.response_json.clone()),
            answered_at: Some(answered_timestamp),
            ..current_hitl
        })
    }

    /// Cancel a pending HITL request, fail its dispatch, and clear the run's
    /// active HITL marker inside one SQLite transaction.
    pub fn persist_hitl_cancel(
        &self,
        current_run: &WorkflowRunRecord,
        next_run: &WorkflowRunRecord,
        current_dispatch: &DispatchRecord,
        next_dispatch: &DispatchRecord,
        hitl_request_id: &str,
    ) -> Result<HitlRecord, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        let current_hitl = load_required_hitl_request(&transaction, hitl_request_id)?;
        ensure_pending_transition(&current_hitl, HitlStatus::Cancelled)?;

        let hitl_rows = transaction.execute(
            "UPDATE hitl_request
             SET status = ?1
             WHERE hitl_request_id = ?2",
            params![HitlStatus::Cancelled.as_str(), hitl_request_id],
        )?;
        if hitl_rows == 0 {
            return Err(WorkflowStoreError::Hitl(
                crate::hitl::HitlStoreError::HitlRequestNotFound {
                    hitl_request_id: hitl_request_id.to_string(),
                },
            ));
        }

        let dispatch_rows = update_dispatch_row(&transaction, current_dispatch, next_dispatch)?;
        if dispatch_rows == 0 {
            return Err(resolve_dispatch_update_conflict(
                &transaction,
                &current_dispatch.dispatch_id,
                current_dispatch.status,
            )
            .into());
        }

        let run_rows =
            update_workflow_run_record(&transaction, next_run, Some(current_run.status))?;
        if run_rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current_run.run_id,
                Some(current_run.status),
            ));
        }

        transaction.commit()?;

        Ok(HitlRecord {
            status: HitlStatus::Cancelled,
            ..current_hitl
        })
    }

    /// Mark a timed out HITL request, fail its dispatch, and clear the run's
    /// active HITL marker inside one SQLite transaction.
    pub fn persist_hitl_timeout(
        &self,
        current_run: &WorkflowRunRecord,
        next_run: &WorkflowRunRecord,
        current_dispatch: &DispatchRecord,
        next_dispatch: &DispatchRecord,
        hitl_request_id: &str,
    ) -> Result<HitlRecord, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        let current_hitl = load_required_hitl_request(&transaction, hitl_request_id)?;

        match current_hitl.status {
            HitlStatus::Pending => {
                let hitl_rows = transaction.execute(
                    "UPDATE hitl_request
                     SET status = ?1
                     WHERE hitl_request_id = ?2",
                    params![HitlStatus::TimedOut.as_str(), hitl_request_id],
                )?;
                if hitl_rows == 0 {
                    return Err(WorkflowStoreError::Hitl(
                        crate::hitl::HitlStoreError::HitlRequestNotFound {
                            hitl_request_id: hitl_request_id.to_string(),
                        },
                    ));
                }
            }
            HitlStatus::TimedOut => {}
            other => {
                return Err(WorkflowStoreError::Hitl(
                    crate::hitl::HitlStoreError::InvalidStatusTransition {
                        hitl_request_id: hitl_request_id.to_string(),
                        from: other,
                        to: HitlStatus::TimedOut,
                    },
                ));
            }
        }

        let dispatch_rows = update_dispatch_row(&transaction, current_dispatch, next_dispatch)?;
        if dispatch_rows == 0 {
            return Err(resolve_dispatch_update_conflict(
                &transaction,
                &current_dispatch.dispatch_id,
                current_dispatch.status,
            )
            .into());
        }

        let run_rows =
            update_workflow_run_record(&transaction, next_run, Some(current_run.status))?;
        if run_rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current_run.run_id,
                Some(current_run.status),
            ));
        }

        transaction.commit()?;

        Ok(HitlRecord {
            status: HitlStatus::TimedOut,
            ..current_hitl
        })
    }

    /// Cancel one dispatch, cascade any linked pending HITL requests, and
    /// optionally update the owning workflow run inside one SQLite transaction.
    pub fn persist_dispatch_cancel(
        &self,
        current_dispatch: &DispatchRecord,
        next_dispatch: &DispatchRecord,
        pending_hitl_request_ids: &[String],
        run_transition: Option<(&WorkflowRunRecord, &WorkflowRunRecord)>,
    ) -> Result<(), WorkflowStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        for hitl_request_id in pending_hitl_request_ids {
            let current_hitl = load_required_hitl_request(&transaction, hitl_request_id)?;
            ensure_pending_transition(&current_hitl, HitlStatus::Cancelled)?;
            let rows = transaction.execute(
                "UPDATE hitl_request
                 SET status = ?1
                 WHERE hitl_request_id = ?2",
                params![HitlStatus::Cancelled.as_str(), hitl_request_id.as_str()],
            )?;
            if rows == 0 {
                return Err(WorkflowStoreError::Hitl(
                    crate::hitl::HitlStoreError::HitlRequestNotFound {
                        hitl_request_id: hitl_request_id.clone(),
                    },
                ));
            }
        }

        let dispatch_rows = update_dispatch_row(&transaction, current_dispatch, next_dispatch)?;
        if dispatch_rows == 0 {
            return Err(resolve_dispatch_update_conflict(
                &transaction,
                &current_dispatch.dispatch_id,
                current_dispatch.status,
            )
            .into());
        }

        if let Some((current_run, next_run)) = run_transition {
            let run_rows =
                update_workflow_run_record(&transaction, next_run, Some(current_run.status))?;
            if run_rows == 0 {
                return Err(resolve_update_conflict(
                    &transaction,
                    &current_run.run_id,
                    Some(current_run.status),
                ));
            }
        }

        transaction.commit()?;
        Ok(())
    }
}

impl WorkflowCheckpointRepository {
    /// Create a checkpoint repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Append one checkpoint row.
    pub fn append(&self, checkpoint: &WorkflowCheckpointRecord) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        insert_checkpoint(&conn, checkpoint)?;
        Ok(())
    }

    /// List checkpoints for one run ordered by creation time.
    pub fn list_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowCheckpointRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_run_checkpoints(&conn, run_id)
    }

    /// Prune durable checkpoints for terminal runs older than a cutoff.
    ///
    /// Keeps at most `max_rows_per_run` newest checkpoints per eligible run and
    /// deletes at most `batch_limit` rows per call.
    pub fn prune_terminal_runs_older_than(
        &self,
        older_than: &str,
        max_rows_per_run: usize,
        batch_limit: usize,
    ) -> Result<usize, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        prune_terminal_run_checkpoints(
            &conn,
            older_than,
            usize_to_i64(max_rows_per_run, "max_rows_per_run")?,
            usize_to_i64(batch_limit, "batch_limit")?,
        )
    }
}

impl WorkflowSignalRepository {
    /// Create a signal repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable signal row.
    pub fn insert(&self, signal: &WorkflowSignalRecord) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        insert_signal_row(&conn, signal)?;
        Ok(())
    }

    /// Load one durable signal by ID.
    pub fn find_by_id(
        &self,
        signal_id: &str,
    ) -> Result<Option<WorkflowSignalRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        conn.query_row(
            "SELECT signal_id, run_id, name, payload_json, source, idempotency_key, consumed, created_at, consumed_at
             FROM workflow_signal
             WHERE signal_id = ?1",
            [signal_id],
            read_workflow_signal_row,
        )
        .optional()
        .map_err(Into::into)
    }

    /// List signals for one run, optionally filtered by consumed state.
    pub fn list_for_run(
        &self,
        run_id: &str,
        consumed: Option<bool>,
    ) -> Result<Vec<WorkflowSignalRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut records = Vec::new();
        match consumed {
            Some(consumed) => {
                let mut stmt = conn.prepare(
                    "SELECT signal_id, run_id, name, payload_json, source, idempotency_key, consumed, created_at, consumed_at
                     FROM workflow_signal
                     WHERE run_id = ?1
                       AND consumed = ?2
                     ORDER BY created_at ASC, signal_id ASC",
                )?;
                let rows = stmt.query_map(
                    params![run_id, bool_to_sql(consumed)],
                    read_workflow_signal_row,
                )?;
                for row in rows {
                    records.push(row?);
                }
            }
            None => {
                let mut stmt = conn.prepare(
                    "SELECT signal_id, run_id, name, payload_json, source, idempotency_key, consumed, created_at, consumed_at
                     FROM workflow_signal
                     WHERE run_id = ?1
                     ORDER BY created_at ASC, signal_id ASC",
                )?;
                let rows = stmt.query_map([run_id], read_workflow_signal_row)?;
                for row in rows {
                    records.push(row?);
                }
            }
        }
        Ok(records)
    }

    /// Load a signal by `(run_id, idempotency_key)`.
    pub fn find_by_idempotency_key(
        &self,
        run_id: &str,
        idempotency_key: &str,
    ) -> Result<Option<WorkflowSignalRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        find_signal_by_idempotency_key(&conn, run_id, idempotency_key)
    }

    /// Return the first unconsumed signal matching the run and name.
    pub fn find_unconsumed(
        &self,
        run_id: &str,
        name: &str,
    ) -> Result<Option<WorkflowSignalRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        conn.query_row(
            "SELECT signal_id, run_id, name, payload_json, source, idempotency_key, consumed, created_at, consumed_at
             FROM workflow_signal
             WHERE run_id = ?1
               AND name = ?2
               AND consumed = 0
             ORDER BY created_at ASC, signal_id ASC
             LIMIT 1",
            params![run_id, name],
            read_workflow_signal_row,
        )
        .optional()
        .map_err(Into::into)
    }

    /// Mark one signal as consumed and populate `consumed_at`.
    pub fn consume(&self, signal_id: &str, consumed_at: &str) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        consume_signal_row(&conn, signal_id, consumed_at)
    }
}

fn insert_signal_row(
    conn: &Connection,
    signal: &WorkflowSignalRecord,
) -> Result<(), WorkflowStoreError> {
    match conn.execute(
        "INSERT INTO workflow_signal (
            signal_id,
            run_id,
            name,
            payload_json,
            source,
            idempotency_key,
            consumed,
            created_at,
            consumed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            signal.signal_id.as_str(),
            signal.run_id.as_str(),
            signal.name.as_str(),
            signal.payload_json.as_str(),
            signal.source.as_str(),
            signal.idempotency_key.as_str(),
            bool_to_sql(signal.consumed),
            signal.created_at.as_str(),
            signal.consumed_at.as_deref(),
        ],
    ) {
        Ok(_) => Ok(()),
        Err(error) if is_unique_constraint(&error) => {
            Err(WorkflowStoreError::SignalAlreadyExistsForIdempotency {
                run_id: signal.run_id.clone(),
                idempotency_key: signal.idempotency_key.clone(),
            })
        }
        Err(error) => Err(error.into()),
    }
}

fn find_signal_by_idempotency_key(
    conn: &Connection,
    run_id: &str,
    idempotency_key: &str,
) -> Result<Option<WorkflowSignalRecord>, WorkflowStoreError> {
    conn.query_row(
        "SELECT signal_id, run_id, name, payload_json, source, idempotency_key, consumed, created_at, consumed_at
         FROM workflow_signal
         WHERE run_id = ?1
           AND idempotency_key = ?2",
        params![run_id, idempotency_key],
        read_workflow_signal_row,
    )
    .optional()
    .map_err(Into::into)
}

fn is_unique_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(inner, _)
            if matches!(inner.code, ErrorCode::ConstraintViolation)
    )
}

fn lock_conn(
    conn: &Arc<Mutex<Connection>>,
) -> Result<MutexGuard<'_, Connection>, WorkflowStoreError> {
    conn.lock()
        .map_err(|error| WorkflowStoreError::ConnectionLock(error.to_string()))
}

fn ensure_row_updated(rows: usize, run_id: &str) -> Result<(), WorkflowStoreError> {
    if rows == 0 {
        return Err(WorkflowStoreError::RunNotFound {
            run_id: run_id.to_string(),
        });
    }

    Ok(())
}

fn resolve_update_conflict(
    conn: &Connection,
    run_id: &str,
    expected_status: Option<WorkflowRunStatus>,
) -> WorkflowStoreError {
    let actual = conn
        .query_row(
            "SELECT status FROM workflow_run WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional();

    match actual {
        Ok(Some(actual)) => WorkflowStoreError::UnexpectedRunState {
            run_id: run_id.to_string(),
            expected: expected_status
                .map(|status| status.as_str().to_string())
                .unwrap_or_else(|| "any".to_string()),
            actual,
        },
        Ok(None) | Err(_) => WorkflowStoreError::RunNotFound {
            run_id: run_id.to_string(),
        },
    }
}

fn insert_workflow_run(
    conn: &Connection,
    record: &WorkflowRunRecord,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO workflow_run (
            run_id,
            workflow_id,
            workflow_version,
            status,
            input_json,
            vars_json,
            current_step_id,
            waiting_kind,
            waiting_ref,
            active_dispatch_id,
            active_hitl_request_id,
            labels_json,
            metadata_json,
            error_json,
            started_at,
            updated_at,
            completed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            record.run_id.as_str(),
            record.workflow_id.as_str(),
            record.workflow_version.as_str(),
            record.status.as_str(),
            record.input_json.as_str(),
            record.vars_json.as_str(),
            record.current_step_id.as_deref(),
            record.waiting_kind.as_deref(),
            record.waiting_ref.as_deref(),
            record.active_dispatch_id.as_deref(),
            record.active_hitl_request_id.as_deref(),
            record.labels_json.as_str(),
            record.metadata_json.as_str(),
            record.error_json.as_deref(),
            record.started_at.as_str(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
        ],
    )?;
    Ok(())
}

fn update_workflow_run_record(
    conn: &Connection,
    record: &WorkflowRunRecord,
    expected_status: Option<WorkflowRunStatus>,
) -> Result<usize, rusqlite::Error> {
    match expected_status {
        Some(expected_status) => conn.execute(
            "UPDATE workflow_run
             SET workflow_id = ?2,
                 workflow_version = ?3,
                 status = ?4,
                 input_json = ?5,
                 vars_json = ?6,
                 current_step_id = ?7,
                 waiting_kind = ?8,
                 waiting_ref = ?9,
                 active_dispatch_id = ?10,
                 active_hitl_request_id = ?11,
                 labels_json = ?12,
                 metadata_json = ?13,
                 error_json = ?14,
                 started_at = ?15,
                 updated_at = ?16,
                 completed_at = ?17
             WHERE run_id = ?1
               AND status = ?18",
            params![
                record.run_id.as_str(),
                record.workflow_id.as_str(),
                record.workflow_version.as_str(),
                record.status.as_str(),
                record.input_json.as_str(),
                record.vars_json.as_str(),
                record.current_step_id.as_deref(),
                record.waiting_kind.as_deref(),
                record.waiting_ref.as_deref(),
                record.active_dispatch_id.as_deref(),
                record.active_hitl_request_id.as_deref(),
                record.labels_json.as_str(),
                record.metadata_json.as_str(),
                record.error_json.as_deref(),
                record.started_at.as_str(),
                record.updated_at.as_str(),
                record.completed_at.as_deref(),
                expected_status.as_str(),
            ],
        ),
        None => conn.execute(
            "UPDATE workflow_run
             SET workflow_id = ?2,
                 workflow_version = ?3,
                 status = ?4,
                 input_json = ?5,
                 vars_json = ?6,
                 current_step_id = ?7,
                 waiting_kind = ?8,
                 waiting_ref = ?9,
                 active_dispatch_id = ?10,
                 active_hitl_request_id = ?11,
                 labels_json = ?12,
                 metadata_json = ?13,
                 error_json = ?14,
                 started_at = ?15,
                 updated_at = ?16,
                 completed_at = ?17
             WHERE run_id = ?1",
            params![
                record.run_id.as_str(),
                record.workflow_id.as_str(),
                record.workflow_version.as_str(),
                record.status.as_str(),
                record.input_json.as_str(),
                record.vars_json.as_str(),
                record.current_step_id.as_deref(),
                record.waiting_kind.as_deref(),
                record.waiting_ref.as_deref(),
                record.active_dispatch_id.as_deref(),
                record.active_hitl_request_id.as_deref(),
                record.labels_json.as_str(),
                record.metadata_json.as_str(),
                record.error_json.as_deref(),
                record.started_at.as_str(),
                record.updated_at.as_str(),
                record.completed_at.as_deref(),
            ],
        ),
    }
}

fn insert_checkpoint(
    conn: &Connection,
    checkpoint: &WorkflowCheckpointRecord,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO workflow_checkpoint (
            checkpoint_id,
            run_id,
            step_id,
            kind,
            data_json,
            created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            checkpoint.checkpoint_id.as_str(),
            checkpoint.run_id.as_str(),
            checkpoint.step_id.as_deref(),
            checkpoint.kind.as_str(),
            checkpoint.data_json.as_str(),
            checkpoint.created_at.as_str(),
        ],
    )?;
    Ok(())
}

fn consume_signal_row(
    conn: &Connection,
    signal_id: &str,
    consumed_at: &str,
) -> Result<(), WorkflowStoreError> {
    let rows = conn.execute(
        "UPDATE workflow_signal
         SET consumed = 1,
             consumed_at = ?2
         WHERE signal_id = ?1 AND consumed = 0",
        params![signal_id, consumed_at],
    )?;

    if rows > 0 {
        return Ok(());
    }

    let consumed_state = conn
        .query_row(
            "SELECT consumed FROM workflow_signal WHERE signal_id = ?1",
            [signal_id],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;

    match consumed_state {
        Some(value) if sql_to_bool(value) => Err(WorkflowStoreError::SignalAlreadyConsumed {
            signal_id: signal_id.to_string(),
        }),
        Some(_) => Err(WorkflowStoreError::SignalAlreadyConsumed {
            signal_id: signal_id.to_string(),
        }),
        None => Err(WorkflowStoreError::SignalNotFound {
            signal_id: signal_id.to_string(),
        }),
    }
}

fn recover_running_runs_with<F>(
    conn: &mut Connection,
    mut checkpoint_id_for: F,
) -> Result<Vec<WorkflowRunRecoveryRecord>, WorkflowStoreError>
where
    F: FnMut(&WorkflowRunRecord, usize) -> String,
{
    let transaction = conn.unchecked_transaction()?;
    let running = list_workflow_runs_by_status(&transaction, WorkflowRunStatus::Running)?;
    let recovered_at = now_timestamp();
    let mut recovered = Vec::with_capacity(running.len());

    for (index, current) in running.into_iter().enumerate() {
        let mut next = current.clone();
        next.status = WorkflowRunStatus::Paused;
        next.waiting_kind = None;
        next.waiting_ref = None;
        next.updated_at = recovered_at.clone();

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: checkpoint_id_for(&current, index),
            run_id: current.run_id.clone(),
            step_id: current.current_step_id.clone(),
            kind: CheckpointKind::RunRecoveredNeedsResume,
            data_json: serde_json::json!({
                "previous_status": current.status.as_str(),
            })
            .to_string(),
            created_at: recovered_at.clone(),
        };
        insert_checkpoint(&transaction, &checkpoint)?;

        let rows =
            update_workflow_run_record(&transaction, &next, Some(WorkflowRunStatus::Running))?;
        if rows == 0 {
            return Err(resolve_update_conflict(
                &transaction,
                &current.run_id,
                Some(WorkflowRunStatus::Running),
            ));
        }

        recovered.push(WorkflowRunRecoveryRecord {
            run_id: current.run_id,
            previous_status: WorkflowRunStatus::Running,
        });
    }

    transaction.commit()?;
    Ok(recovered)
}

fn validate_status_update_transition(
    current: &WorkflowRunRecord,
    next: &WorkflowRunRecord,
    checkpoint: &WorkflowCheckpointRecord,
) -> Result<(), WorkflowStoreError> {
    match checkpoint.kind {
        CheckpointKind::RunPaused => {
            if !matches!(
                current.status,
                WorkflowRunStatus::Running | WorkflowRunStatus::WaitingSignal
            ) {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: "running, waiting_signal".to_string(),
                    actual: current.status.to_string(),
                });
            }
            if next.status != WorkflowRunStatus::Paused {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: WorkflowRunStatus::Paused.to_string(),
                    actual: next.status.to_string(),
                });
            }
        }
        CheckpointKind::RunResumed => {
            if current.status != WorkflowRunStatus::Paused {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: WorkflowRunStatus::Paused.to_string(),
                    actual: current.status.to_string(),
                });
            }
            if next.status != WorkflowRunStatus::Running {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: WorkflowRunStatus::Running.to_string(),
                    actual: next.status.to_string(),
                });
            }
        }
        CheckpointKind::RunCancelled => {
            if current.status.is_terminal() {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: "pending, running, waiting_signal, waiting_hitl, paused".to_string(),
                    actual: current.status.to_string(),
                });
            }
            if next.status != WorkflowRunStatus::Cancelled {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: WorkflowRunStatus::Cancelled.to_string(),
                    actual: next.status.to_string(),
                });
            }
        }
        CheckpointKind::RunRecoveredNeedsResume => {
            if current.status != WorkflowRunStatus::Running {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: WorkflowRunStatus::Running.to_string(),
                    actual: current.status.to_string(),
                });
            }
            if next.status != WorkflowRunStatus::Paused {
                return Err(WorkflowStoreError::UnexpectedRunState {
                    run_id: current.run_id.clone(),
                    expected: WorkflowRunStatus::Paused.to_string(),
                    actual: next.status.to_string(),
                });
            }
        }
        _ => {}
    }

    Ok(())
}

fn list_run_checkpoints(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<WorkflowCheckpointRecord>, WorkflowStoreError> {
    let mut stmt = conn.prepare(
        "SELECT checkpoint_id, run_id, step_id, kind, data_json, created_at
         FROM workflow_checkpoint
         WHERE run_id = ?1
         ORDER BY created_at ASC, checkpoint_id ASC",
    )?;
    let rows = stmt.query_map([run_id], read_workflow_checkpoint_row)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn prune_terminal_run_checkpoints(
    conn: &Connection,
    older_than: &str,
    max_rows_per_run: i64,
    batch_limit: i64,
) -> Result<usize, WorkflowStoreError> {
    if max_rows_per_run < 0 {
        return Err(WorkflowStoreError::InvalidListQuery {
            field: "max_rows_per_run",
            value: max_rows_per_run.to_string(),
        });
    }
    if batch_limit <= 0 {
        return Ok(0);
    }

    let deleted = conn.execute(
        "WITH ranked AS (
             SELECT
                 wc.checkpoint_id,
                 ROW_NUMBER() OVER (
                     PARTITION BY wc.run_id
                     ORDER BY wc.created_at DESC, wc.checkpoint_id DESC
                 ) AS row_no
             FROM workflow_checkpoint AS wc
             JOIN workflow_run AS wr
               ON wr.run_id = wc.run_id
             WHERE wr.status IN ('completed', 'failed', 'cancelled')
               AND datetime(COALESCE(wr.completed_at, wr.updated_at)) < datetime(?1)
         ),
         to_delete AS (
             SELECT checkpoint_id
             FROM ranked
             WHERE row_no > ?2
             ORDER BY checkpoint_id ASC
             LIMIT ?3
         )
         DELETE FROM workflow_checkpoint
         WHERE checkpoint_id IN (SELECT checkpoint_id FROM to_delete)",
        params![older_than, max_rows_per_run, batch_limit],
    )?;

    Ok(deleted)
}

fn list_run_dispatches(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<WorkflowDispatchSummaryRecord>, WorkflowStoreError> {
    if !table_exists(conn, "agent_dispatch")? {
        return Ok(Vec::new());
    }

    list_dispatch_summaries_by_run(conn, run_id).map_err(WorkflowStoreError::from)
}

fn table_exists(conn: &Connection, table_name: &str) -> Result<bool, rusqlite::Error> {
    conn.query_row(
        "SELECT EXISTS(
            SELECT 1
            FROM sqlite_master
            WHERE type = 'table'
              AND name = ?1
        )",
        [table_name],
        |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists == 1)
}

fn load_workflow_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<WorkflowRunRecord>, WorkflowStoreError> {
    conn.query_row(
        "SELECT
            run_id,
            workflow_id,
            workflow_version,
            status,
            input_json,
            vars_json,
            current_step_id,
            waiting_kind,
            waiting_ref,
            active_dispatch_id,
            active_hitl_request_id,
            labels_json,
            metadata_json,
            error_json,
            started_at,
            updated_at,
            completed_at
         FROM workflow_run
         WHERE run_id = ?1",
        [run_id],
        read_workflow_run_row,
    )
    .optional()
    .map_err(Into::into)
}

fn list_all_workflow_runs(conn: &Connection) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            run_id,
            workflow_id,
            workflow_version,
            status,
            input_json,
            vars_json,
            current_step_id,
            waiting_kind,
            waiting_ref,
            active_dispatch_id,
            active_hitl_request_id,
            labels_json,
            metadata_json,
            error_json,
            started_at,
            updated_at,
            completed_at
         FROM workflow_run
         ORDER BY updated_at DESC, run_id DESC",
    )?;
    let rows = stmt.query_map([], read_workflow_run_row)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn list_workflow_runs_by_status(
    conn: &Connection,
    status: WorkflowRunStatus,
) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            run_id,
            workflow_id,
            workflow_version,
            status,
            input_json,
            vars_json,
            current_step_id,
            waiting_kind,
            waiting_ref,
            active_dispatch_id,
            active_hitl_request_id,
            labels_json,
            metadata_json,
            error_json,
            started_at,
            updated_at,
            completed_at
         FROM workflow_run
         WHERE status = ?1
         ORDER BY updated_at ASC, run_id ASC",
    )?;
    let rows = stmt.query_map([status.as_str()], read_workflow_run_row)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn read_workflow_run_row(row: &rusqlite::Row<'_>) -> Result<WorkflowRunRecord, rusqlite::Error> {
    let status = row.get::<_, String>(3)?;
    let status = status
        .parse::<WorkflowRunStatus>()
        .map_err(invalid_text_error)?;

    Ok(WorkflowRunRecord {
        run_id: row.get(0)?,
        workflow_id: row.get(1)?,
        workflow_version: row.get(2)?,
        status,
        input_json: row.get(4)?,
        vars_json: row.get(5)?,
        current_step_id: row.get(6)?,
        waiting_kind: row.get(7)?,
        waiting_ref: row.get(8)?,
        active_dispatch_id: row.get(9)?,
        active_hitl_request_id: row.get(10)?,
        labels_json: row.get(11)?,
        metadata_json: row.get(12)?,
        error_json: row.get(13)?,
        started_at: row.get(14)?,
        updated_at: row.get(15)?,
        completed_at: row.get(16)?,
    })
}

fn read_workflow_checkpoint_row(
    row: &rusqlite::Row<'_>,
) -> Result<WorkflowCheckpointRecord, rusqlite::Error> {
    let kind = row.get::<_, String>(3)?;
    let kind = kind.parse::<CheckpointKind>().map_err(invalid_text_error)?;

    Ok(WorkflowCheckpointRecord {
        checkpoint_id: row.get(0)?,
        run_id: row.get(1)?,
        step_id: row.get(2)?,
        kind,
        data_json: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn read_workflow_signal_row(
    row: &rusqlite::Row<'_>,
) -> Result<WorkflowSignalRecord, rusqlite::Error> {
    Ok(WorkflowSignalRecord {
        signal_id: row.get(0)?,
        run_id: row.get(1)?,
        name: row.get(2)?,
        payload_json: row.get(3)?,
        source: row.get(4)?,
        idempotency_key: row.get(5)?,
        consumed: sql_to_bool(row.get::<_, i64>(6)?),
        created_at: row.get(7)?,
        consumed_at: row.get(8)?,
    })
}

fn invalid_text_error(error: WorkflowStoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
}

fn bool_to_sql(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn sql_to_bool(value: i64) -> bool {
    value != 0
}

fn usize_to_i64(value: usize, field: &'static str) -> Result<i64, WorkflowStoreError> {
    i64::try_from(value).map_err(|_| WorkflowStoreError::InvalidListQuery {
        field,
        value: value.to_string(),
    })
}

/// Return the current UTC timestamp in RFC 3339 format.
pub fn now_timestamp() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn compozy_conn() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open in-memory compozy.db");
        conn.execute_batch(WORKFLOW_RUN_CORE_MIGRATION_SQL)
            .expect("apply workflow_run schema");
        conn.execute_batch(WORKFLOW_CHECKPOINT_MIGRATION_SQL)
            .expect("apply workflow_checkpoint schema");
        conn.execute_batch(WORKFLOW_SIGNAL_MIGRATION_SQL)
            .expect("apply workflow_signal schema");
        conn.execute_batch(WORKFLOW_RUNTIME_DURABILITY_MIGRATION_SQL)
            .expect("apply workflow runtime durability migration");
        conn.execute_batch(WORKFLOW_SIGNAL_WAITING_STATE_MIGRATION_SQL)
            .expect("apply workflow signal waiting-state migration");
        conn.execute_batch(WORKFLOW_RUN_CONTROL_PLANE_MIGRATION_SQL)
            .expect("apply workflow control-plane migration");
        conn.execute_batch(crate::dispatch::AGENT_DISPATCH_MIGRATION_SQL)
            .expect("apply agent_dispatch migration");
        conn.execute_batch(crate::artifact::ARTIFACT_DOC_VERSIONING_MIGRATION_SQL)
            .expect("apply artifact/doc migration");
        Arc::new(Mutex::new(conn))
    }

    fn sample_run_record(run_id: &str) -> WorkflowRunRecord {
        WorkflowRunRecord {
            run_id: run_id.to_string(),
            workflow_id: "workflow-alpha".to_string(),
            workflow_version: "1.0.0".to_string(),
            status: WorkflowRunStatus::Pending,
            input_json: "\"hello world\"".to_string(),
            vars_json: "{}".to_string(),
            current_step_id: None,
            waiting_kind: None,
            waiting_ref: None,
            active_dispatch_id: None,
            active_hitl_request_id: None,
            labels_json: "[\"manual\"]".to_string(),
            metadata_json: "{\"source\":\"api\"}".to_string(),
            error_json: None,
            started_at: "2026-03-23T12:00:00Z".to_string(),
            updated_at: "2026-03-23T12:00:00Z".to_string(),
            completed_at: None,
        }
    }

    fn sample_checkpoint(
        run_id: &str,
        step_id: Option<&str>,
        kind: CheckpointKind,
        created_at: &str,
    ) -> WorkflowCheckpointRecord {
        WorkflowCheckpointRecord {
            checkpoint_id: format!("chk-{run_id}-{created_at}"),
            run_id: run_id.to_string(),
            step_id: step_id.map(ToOwned::to_owned),
            kind,
            data_json: "{}".to_string(),
            created_at: created_at.to_string(),
        }
    }

    fn sample_status_checkpoint(
        run_id: &str,
        step_id: Option<&str>,
        kind: CheckpointKind,
        created_at: &str,
        data: serde_json::Value,
    ) -> WorkflowCheckpointRecord {
        WorkflowCheckpointRecord {
            checkpoint_id: format!("chk-{run_id}-{created_at}-{kind}"),
            run_id: run_id.to_string(),
            step_id: step_id.map(ToOwned::to_owned),
            kind,
            data_json: data.to_string(),
            created_at: created_at.to_string(),
        }
    }

    #[test]
    fn workflow_run_repository_should_insert_and_load_rows() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let record = sample_run_record("run-create");

        stores
            .workflow_run
            .insert_run(&record)
            .expect("insert workflow run");

        let loaded = stores
            .workflow_run
            .find_by_id(&record.run_id)
            .expect("load workflow run")
            .expect("workflow run should exist");

        assert_eq!(loaded, record);
    }

    #[test]
    fn workflow_run_repository_should_persist_checkpoint_before_row_update_atomically() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let current = sample_run_record("run-transition");
        let mut next = current.clone();
        let conflict = sample_checkpoint(
            &current.run_id,
            Some("step-1"),
            CheckpointKind::RunStarted,
            "2026-03-23T12:01:00Z",
        );

        stores
            .workflow_run
            .insert_run(&current)
            .expect("insert workflow run");
        stores
            .workflow_checkpoint
            .append(&conflict)
            .expect("insert conflicting checkpoint");

        next.status = WorkflowRunStatus::Running;
        next.current_step_id = Some("step-1".to_string());
        next.updated_at = "2026-03-23T12:01:00Z".to_string();

        let result = stores
            .workflow_run
            .persist_transition(&current, &next, &conflict);

        assert!(
            result.is_err(),
            "transition should fail on checkpoint conflict"
        );

        let loaded = stores
            .workflow_run
            .find_by_id(&current.run_id)
            .expect("load workflow run")
            .expect("workflow run should exist");
        let checkpoints = stores
            .workflow_checkpoint
            .list_for_run(&current.run_id)
            .expect("load checkpoints");

        assert_eq!(loaded, current);
        assert_eq!(checkpoints.len(), 1);
    }

    #[test]
    fn workflow_run_repository_should_list_non_terminal_rows() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let pending = sample_run_record("run-pending");
        let mut running = sample_run_record("run-running");
        let mut waiting = sample_run_record("run-waiting");
        let mut waiting_hitl = sample_run_record("run-waiting-hitl");
        let mut paused = sample_run_record("run-paused");
        let mut completed = sample_run_record("run-completed");

        running.status = WorkflowRunStatus::Running;
        running.updated_at = "2026-03-23T12:01:00Z".to_string();
        waiting.status = WorkflowRunStatus::WaitingSignal;
        waiting.waiting_kind = Some("signal".to_string());
        waiting.waiting_ref = Some("artifact-approved".to_string());
        waiting.updated_at = "2026-03-23T12:02:00Z".to_string();
        waiting_hitl.status = WorkflowRunStatus::WaitingHitl;
        waiting_hitl.waiting_kind = Some("hitl".to_string());
        waiting_hitl.waiting_ref = Some("hitl-request-42".to_string());
        waiting_hitl.updated_at = "2026-03-23T12:02:30Z".to_string();
        paused.status = WorkflowRunStatus::Paused;
        paused.updated_at = "2026-03-23T12:02:45Z".to_string();
        completed.status = WorkflowRunStatus::Completed;
        completed.completed_at = Some("2026-03-23T12:03:00Z".to_string());
        completed.updated_at = "2026-03-23T12:03:00Z".to_string();

        for record in [
            &pending,
            &running,
            &waiting,
            &waiting_hitl,
            &paused,
            &completed,
        ] {
            stores
                .workflow_run
                .insert_run(record)
                .expect("insert workflow run");
        }

        let records = stores
            .workflow_run
            .list_non_terminal()
            .expect("list non-terminal runs");
        let run_ids = records
            .into_iter()
            .map(|record| record.run_id)
            .collect::<Vec<_>>();

        assert_eq!(
            run_ids,
            vec![
                pending.run_id.clone(),
                running.run_id.clone(),
                waiting.run_id.clone(),
                waiting_hitl.run_id.clone(),
                paused.run_id.clone(),
            ]
        );
    }

    #[test]
    fn running_runs_downgrade_to_paused_on_recovery_scan() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut running = sample_run_record("run-recovery-running");
        running.status = WorkflowRunStatus::Running;
        running.current_step_id = Some("step-1".to_string());
        running.updated_at = "2026-03-23T12:01:00Z".to_string();

        stores
            .workflow_run
            .insert_run(&running)
            .expect("insert running workflow run");

        let recovered = stores
            .workflow_run
            .recover_running_runs()
            .expect("recover running runs");
        let loaded = stores
            .workflow_run
            .find_by_id(&running.run_id)
            .expect("load recovered run")
            .expect("run should exist");
        let checkpoints = stores
            .workflow_run
            .find_checkpoints_for_run(&running.run_id)
            .expect("load recovery checkpoints");

        assert_eq!(
            recovered,
            vec![WorkflowRunRecoveryRecord {
                run_id: running.run_id.clone(),
                previous_status: WorkflowRunStatus::Running,
            }]
        );
        assert_eq!(loaded.status, WorkflowRunStatus::Paused);
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].kind, CheckpointKind::RunRecoveredNeedsResume);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&checkpoints[0].data_json)
                .expect("checkpoint data should deserialize"),
            serde_json::json!({ "previous_status": "running" })
        );
    }

    #[test]
    fn waiting_signal_runs_survive_recovery_scan_unchanged() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut waiting = sample_run_record("run-recovery-wait-signal");
        waiting.status = WorkflowRunStatus::WaitingSignal;
        waiting.current_step_id = Some("await-approval".to_string());
        waiting.waiting_kind = Some("signal".to_string());
        waiting.waiting_ref = Some("approval".to_string());
        waiting.updated_at = "2026-03-23T12:02:00Z".to_string();

        stores
            .workflow_run
            .insert_run(&waiting)
            .expect("insert waiting workflow run");

        let recovered = stores
            .workflow_run
            .recover_running_runs()
            .expect("recover running runs");
        let loaded = stores
            .workflow_run
            .find_by_id(&waiting.run_id)
            .expect("load waiting run")
            .expect("waiting run should exist");
        let checkpoints = stores
            .workflow_run
            .find_checkpoints_for_run(&waiting.run_id)
            .expect("load waiting checkpoints");

        assert!(recovered.is_empty());
        assert_eq!(loaded, waiting);
        assert!(checkpoints.is_empty());
    }

    #[test]
    fn waiting_hitl_runs_survive_recovery_scan_unchanged() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut waiting = sample_run_record("run-recovery-wait-hitl");
        waiting.status = WorkflowRunStatus::WaitingHitl;
        waiting.current_step_id = Some("await-review".to_string());
        waiting.waiting_kind = Some("hitl".to_string());
        waiting.waiting_ref = Some("hitl-request-42".to_string());
        waiting.active_hitl_request_id = Some("hitl-request-42".to_string());
        waiting.updated_at = "2026-03-23T12:03:00Z".to_string();

        stores
            .workflow_run
            .insert_run(&waiting)
            .expect("insert waiting workflow run");

        let recovered = stores
            .workflow_run
            .recover_running_runs()
            .expect("recover running runs");
        let loaded = stores
            .workflow_run
            .find_by_id(&waiting.run_id)
            .expect("load waiting run")
            .expect("waiting run should exist");
        let checkpoints = stores
            .workflow_run
            .find_checkpoints_for_run(&waiting.run_id)
            .expect("load waiting checkpoints");

        assert!(recovered.is_empty());
        assert_eq!(loaded, waiting);
        assert!(checkpoints.is_empty());
    }

    #[test]
    fn terminal_runs_are_not_touched_by_recovery_scan() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut completed = sample_run_record("run-recovery-completed");
        let mut failed = sample_run_record("run-recovery-failed");
        let mut cancelled = sample_run_record("run-recovery-cancelled");

        completed.status = WorkflowRunStatus::Completed;
        completed.completed_at = Some("2026-03-23T12:03:00Z".to_string());
        completed.updated_at = "2026-03-23T12:03:00Z".to_string();
        failed.status = WorkflowRunStatus::Failed;
        failed.error_json = Some(serde_json::json!({ "message": "boom" }).to_string());
        failed.completed_at = Some("2026-03-23T12:04:00Z".to_string());
        failed.updated_at = "2026-03-23T12:04:00Z".to_string();
        cancelled.status = WorkflowRunStatus::Cancelled;
        cancelled.completed_at = Some("2026-03-23T12:05:00Z".to_string());
        cancelled.updated_at = "2026-03-23T12:05:00Z".to_string();

        for record in [&completed, &failed, &cancelled] {
            stores
                .workflow_run
                .insert_run(record)
                .expect("insert terminal run");
        }

        let recovered = stores
            .workflow_run
            .recover_running_runs()
            .expect("recover running runs");

        assert!(recovered.is_empty());
        for record in [&completed, &failed, &cancelled] {
            let loaded = stores
                .workflow_run
                .find_by_id(&record.run_id)
                .expect("load terminal run")
                .expect("terminal run should exist");
            let checkpoints = stores
                .workflow_run
                .find_checkpoints_for_run(&record.run_id)
                .expect("load terminal checkpoints");

            assert_eq!(loaded, *record);
            assert!(checkpoints.is_empty());
        }
    }

    #[test]
    fn recovery_scan_is_atomic() {
        let conn = compozy_conn();
        let stores = WorkflowStoreSet::new(Arc::clone(&conn));
        let mut first = sample_run_record("run-recovery-atomic-a");
        let mut second = sample_run_record("run-recovery-atomic-b");
        first.status = WorkflowRunStatus::Running;
        second.status = WorkflowRunStatus::Running;
        first.current_step_id = Some("step-a".to_string());
        second.current_step_id = Some("step-b".to_string());

        stores
            .workflow_run
            .insert_run(&first)
            .expect("insert first running run");
        stores
            .workflow_run
            .insert_run(&second)
            .expect("insert second running run");

        {
            let mut guard = conn.lock().expect("lock workflow db");
            let result = recover_running_runs_with(&mut guard, |_run, _index| {
                "chk-recovery-duplicate".to_string()
            });
            assert!(result.is_err(), "duplicate checkpoint ids should fail");
        }

        for run_id in [&first.run_id, &second.run_id] {
            let loaded = stores
                .workflow_run
                .find_by_id(run_id)
                .expect("load run after failed recovery")
                .expect("run should exist");
            let checkpoints = stores
                .workflow_run
                .find_checkpoints_for_run(run_id)
                .expect("load checkpoints after failed recovery");

            assert_eq!(loaded.status, WorkflowRunStatus::Running);
            assert!(checkpoints.is_empty());
        }
    }

    #[test]
    fn recovery_checkpoints_record_previous_status() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut first = sample_run_record("run-recovery-data-a");
        let mut second = sample_run_record("run-recovery-data-b");
        first.status = WorkflowRunStatus::Running;
        second.status = WorkflowRunStatus::Running;

        stores
            .workflow_run
            .insert_run(&first)
            .expect("insert first running run");
        stores
            .workflow_run
            .insert_run(&second)
            .expect("insert second running run");
        stores
            .workflow_run
            .recover_running_runs()
            .expect("recover running runs");

        for run_id in [&first.run_id, &second.run_id] {
            let checkpoints = stores
                .workflow_run
                .find_checkpoints_for_run(run_id)
                .expect("load recovery checkpoints");
            assert_eq!(checkpoints.len(), 1);
            assert_eq!(checkpoints[0].kind, CheckpointKind::RunRecoveredNeedsResume);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&checkpoints[0].data_json)
                    .expect("checkpoint data should deserialize")["previous_status"],
                "running"
            );
        }
    }

    #[test]
    fn recovery_scan_is_idempotent() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut running = sample_run_record("run-recovery-idempotent");
        running.status = WorkflowRunStatus::Running;

        stores
            .workflow_run
            .insert_run(&running)
            .expect("insert running workflow run");

        let first = stores
            .workflow_run
            .recover_running_runs()
            .expect("first recovery should succeed");
        let second = stores
            .workflow_run
            .recover_running_runs()
            .expect("second recovery should succeed");
        let checkpoints = stores
            .workflow_run
            .find_checkpoints_for_run(&running.run_id)
            .expect("load recovery checkpoints");

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].kind, CheckpointKind::RunRecoveredNeedsResume);
    }

    #[test]
    fn pause_action_rejects_invalid_source_status() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut current = sample_run_record("run-pause-invalid");
        current.status = WorkflowRunStatus::Completed;
        current.completed_at = Some("2026-03-23T12:04:00Z".to_string());
        current.updated_at = "2026-03-23T12:04:00Z".to_string();
        let mut next = current.clone();
        next.status = WorkflowRunStatus::Paused;
        next.updated_at = "2026-03-23T12:05:00Z".to_string();
        let checkpoint = sample_status_checkpoint(
            &current.run_id,
            current.current_step_id.as_deref(),
            CheckpointKind::RunPaused,
            &next.updated_at,
            serde_json::json!({ "actor_source": "api" }),
        );

        stores
            .workflow_run
            .insert_run(&current)
            .expect("insert completed run");

        let result = stores
            .workflow_run
            .update_status(&current, &next, &checkpoint);
        let loaded = stores
            .workflow_run
            .find_by_id(&current.run_id)
            .expect("load completed run")
            .expect("completed run should exist");
        let checkpoints = stores
            .workflow_run
            .find_checkpoints_for_run(&current.run_id)
            .expect("load completed run checkpoints");

        assert!(matches!(
            result,
            Err(WorkflowStoreError::UnexpectedRunState { .. })
        ));
        assert_eq!(loaded, current);
        assert!(checkpoints.is_empty());
    }

    #[test]
    fn resume_action_only_valid_from_paused() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut current = sample_run_record("run-resume-invalid");
        current.status = WorkflowRunStatus::Running;
        current.current_step_id = Some("step-1".to_string());
        current.updated_at = "2026-03-23T12:01:00Z".to_string();
        let mut next = current.clone();
        next.updated_at = "2026-03-23T12:02:00Z".to_string();
        let checkpoint = sample_status_checkpoint(
            &current.run_id,
            current.current_step_id.as_deref(),
            CheckpointKind::RunResumed,
            &next.updated_at,
            serde_json::json!({ "actor_source": "api" }),
        );

        stores
            .workflow_run
            .insert_run(&current)
            .expect("insert running run");

        let result = stores
            .workflow_run
            .update_status(&current, &next, &checkpoint);
        let loaded = stores
            .workflow_run
            .find_by_id(&current.run_id)
            .expect("load running run")
            .expect("running run should exist");
        let checkpoints = stores
            .workflow_run
            .find_checkpoints_for_run(&current.run_id)
            .expect("load running run checkpoints");

        assert!(matches!(
            result,
            Err(WorkflowStoreError::UnexpectedRunState { .. })
        ));
        assert_eq!(loaded.status, WorkflowRunStatus::Running);
        assert!(checkpoints.is_empty());
    }

    #[test]
    fn signal_insert_persists_payload_and_source() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let run = sample_run_record("run-signal");
        let signal = WorkflowSignalRecord {
            signal_id: "signal-one".to_string(),
            run_id: run.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"answer\":\"yes\"}".to_string(),
            source: "trigger".to_string(),
            idempotency_key: "idem-trigger".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:05:00Z".to_string(),
            consumed_at: None,
        };

        stores
            .workflow_run
            .insert_run(&run)
            .expect("insert workflow run");
        stores
            .workflow_signal
            .insert(&signal)
            .expect("insert durable signal");

        let loaded = stores
            .workflow_signal
            .find_by_id(&signal.signal_id)
            .expect("load signal")
            .expect("signal should exist");

        assert_eq!(loaded.name, signal.name);
        assert_eq!(loaded.payload_json, signal.payload_json);
        assert_eq!(loaded.source, "trigger");
        assert_eq!(loaded.idempotency_key, "idem-trigger");
        assert!(!loaded.consumed);
        assert_eq!(loaded.consumed_at, None);
    }

    #[test]
    fn signal_idempotency_key_prevents_duplicate() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let run = sample_run_record("run-idempotent");
        let signal = WorkflowSignalRecord {
            signal_id: "signal-idempotent".to_string(),
            run_id: run.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"decision\":\"approved\"}".to_string(),
            source: "api".to_string(),
            idempotency_key: "same-key".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:08:00Z".to_string(),
            consumed_at: None,
        };

        stores
            .workflow_run
            .insert_run(&run)
            .expect("insert workflow run");
        stores
            .workflow_signal
            .insert(&signal)
            .expect("insert signal");
        let duplicate = stores
            .workflow_signal
            .insert(&WorkflowSignalRecord {
                signal_id: "signal-idempotent-duplicate".to_string(),
                run_id: run.run_id.clone(),
                idempotency_key: "same-key".to_string(),
                ..signal.clone()
            })
            .expect_err("duplicate idempotency key should be rejected");

        assert!(matches!(
            duplicate,
            WorkflowStoreError::SignalAlreadyExistsForIdempotency { .. }
        ));
        let existing = stores
            .workflow_signal
            .find_by_idempotency_key(&run.run_id, "same-key")
            .expect("lookup by idempotency key should succeed")
            .expect("signal should exist");
        let stored = stores
            .workflow_signal
            .list_for_run(&run.run_id, None)
            .expect("list signals should succeed");

        assert_eq!(existing.signal_id, signal.signal_id);
        assert_eq!(stored, vec![signal]);
    }

    #[test]
    fn list_signals_returns_only_for_requested_run() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let first_run = sample_run_record("run-first");
        let second_run = sample_run_record("run-second");
        stores
            .workflow_run
            .insert_run(&first_run)
            .expect("insert first run");
        stores
            .workflow_run
            .insert_run(&second_run)
            .expect("insert second run");
        stores
            .workflow_signal
            .insert(&WorkflowSignalRecord {
                signal_id: "signal-first".to_string(),
                run_id: first_run.run_id.clone(),
                name: "approval".to_string(),
                payload_json: "{\"answer\":\"yes\"}".to_string(),
                source: "schedule".to_string(),
                idempotency_key: "idem-first".to_string(),
                consumed: false,
                created_at: "2026-03-23T12:09:00Z".to_string(),
                consumed_at: None,
            })
            .expect("insert first signal");
        stores
            .workflow_signal
            .insert(&WorkflowSignalRecord {
                signal_id: "signal-second".to_string(),
                run_id: second_run.run_id.clone(),
                name: "approval".to_string(),
                payload_json: "{\"answer\":\"no\"}".to_string(),
                source: "api".to_string(),
                idempotency_key: "idem-second".to_string(),
                consumed: false,
                created_at: "2026-03-23T12:09:30Z".to_string(),
                consumed_at: None,
            })
            .expect("insert second signal");

        let listed = stores
            .workflow_signal
            .list_for_run(&first_run.run_id, None)
            .expect("list signals should succeed");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].run_id, first_run.run_id);
        assert_eq!(listed[0].source, "schedule");
    }

    #[test]
    fn list_signals_consumed_filter_works() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let run = sample_run_record("run-consumed-filter");
        let consumed_signal = WorkflowSignalRecord {
            signal_id: "signal-consumed".to_string(),
            run_id: run.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"answer\":\"yes\"}".to_string(),
            source: "api".to_string(),
            idempotency_key: "idem-consumed".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:10:00Z".to_string(),
            consumed_at: None,
        };
        let unconsumed_signal = WorkflowSignalRecord {
            signal_id: "signal-unconsumed".to_string(),
            run_id: run.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"answer\":\"no\"}".to_string(),
            source: "schedule".to_string(),
            idempotency_key: "idem-unconsumed".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:10:30Z".to_string(),
            consumed_at: None,
        };

        stores
            .workflow_run
            .insert_run(&run)
            .expect("insert workflow run");
        stores
            .workflow_signal
            .insert(&consumed_signal)
            .expect("insert consumed candidate");
        stores
            .workflow_signal
            .insert(&unconsumed_signal)
            .expect("insert unconsumed candidate");
        stores
            .workflow_signal
            .consume(&consumed_signal.signal_id, "2026-03-23T12:11:00Z")
            .expect("consume signal");

        let consumed = stores
            .workflow_signal
            .list_for_run(&run.run_id, Some(true))
            .expect("list consumed signals");
        let unconsumed = stores
            .workflow_signal
            .list_for_run(&run.run_id, Some(false))
            .expect("list unconsumed signals");

        assert_eq!(consumed.len(), 1);
        assert_eq!(consumed[0].signal_id, consumed_signal.signal_id);
        assert_eq!(unconsumed.len(), 1);
        assert_eq!(unconsumed[0].signal_id, unconsumed_signal.signal_id);
        assert_eq!(unconsumed[0].source, "schedule");
    }

    #[test]
    fn signal_consumption_is_transactional() {
        let conn = compozy_conn();
        let stores = WorkflowStoreSet::new(Arc::clone(&conn));
        let mut current = sample_run_record("run-transactional");
        current.status = WorkflowRunStatus::WaitingSignal;
        current.current_step_id = Some("await-approval".to_string());
        current.waiting_kind = Some("signal".to_string());
        current.waiting_ref = Some("approval".to_string());
        let signal = WorkflowSignalRecord {
            signal_id: "signal-transactional".to_string(),
            run_id: current.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"decision\":\"approved\"}".to_string(),
            source: "api".to_string(),
            idempotency_key: "idem-transactional".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:11:00Z".to_string(),
            consumed_at: None,
        };

        stores
            .workflow_run
            .insert_run(&current)
            .expect("insert waiting run");
        stores
            .workflow_signal
            .insert(&signal)
            .expect("insert signal");

        let guard = conn.lock().expect("lock connection");
        let transaction = guard.unchecked_transaction().expect("open transaction");
        consume_signal_row(&transaction, &signal.signal_id, "2026-03-23T12:12:00Z")
            .expect("consume signal inside transaction");
        drop(transaction);
        drop(guard);

        let loaded_run = stores
            .workflow_run
            .find_by_id(&current.run_id)
            .expect("load run")
            .expect("run should exist");
        let loaded_signal = stores
            .workflow_signal
            .find_by_id(&signal.signal_id)
            .expect("load signal")
            .expect("signal should exist");

        assert_eq!(loaded_run.waiting_kind.as_deref(), Some("signal"));
        assert!(!loaded_signal.consumed);
        assert_eq!(loaded_signal.consumed_at, None);
    }

    #[test]
    fn consumed_flag_and_timestamp_update_atomically() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut current = sample_run_record("run-resume");
        current.status = WorkflowRunStatus::WaitingSignal;
        current.current_step_id = Some("await-approval".to_string());
        current.waiting_kind = Some("signal".to_string());
        current.waiting_ref = Some("approval".to_string());
        current.updated_at = "2026-03-23T12:12:00Z".to_string();
        let signal = WorkflowSignalRecord {
            signal_id: "signal-resume".to_string(),
            run_id: current.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"decision\":\"approved\"}".to_string(),
            source: "api".to_string(),
            idempotency_key: "idem-resume".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:12:30Z".to_string(),
            consumed_at: None,
        };
        let mut next = current.clone();
        next.status = WorkflowRunStatus::Running;
        next.current_step_id = Some("after-approval".to_string());
        next.waiting_kind = None;
        next.waiting_ref = None;
        next.updated_at = "2026-03-23T12:13:00Z".to_string();
        let consumed_checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: "chk-signal-consumed".to_string(),
            run_id: current.run_id.clone(),
            step_id: Some("await-approval".to_string()),
            kind: CheckpointKind::SignalConsumed,
            data_json: serde_json::json!({
                "signal_id": signal.signal_id,
                "signal_name": signal.name,
            })
            .to_string(),
            created_at: next.updated_at.clone(),
        };
        let resumed_checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: "chk-run-resumed".to_string(),
            run_id: current.run_id.clone(),
            step_id: Some("await-approval".to_string()),
            kind: CheckpointKind::RunResumedFromSignal,
            data_json: serde_json::json!({
                "signal_id": signal.signal_id,
                "signal_name": signal.name,
            })
            .to_string(),
            created_at: next.updated_at.clone(),
        };

        stores
            .workflow_run
            .insert_run(&current)
            .expect("insert waiting run");
        stores
            .workflow_signal
            .insert(&signal)
            .expect("insert signal");
        stores
            .workflow_run
            .persist_signal_resume(
                &current,
                &next,
                &consumed_checkpoint,
                &resumed_checkpoint,
                &signal.signal_id,
                &next.updated_at,
            )
            .expect("persist signal resume transition");

        let loaded_run = stores
            .workflow_run
            .find_by_id(&current.run_id)
            .expect("load resumed run")
            .expect("run should exist");
        let loaded_signal = stores
            .workflow_signal
            .find_by_id(&signal.signal_id)
            .expect("load resumed signal")
            .expect("signal should exist");
        let checkpoints = stores
            .workflow_checkpoint
            .list_for_run(&current.run_id)
            .expect("load resume checkpoints");

        assert_eq!(loaded_run.status, WorkflowRunStatus::Running);
        assert_eq!(loaded_run.waiting_kind, None);
        assert_eq!(loaded_run.waiting_ref, None);
        assert!(loaded_signal.consumed);
        assert_eq!(
            loaded_signal.consumed_at.as_deref(),
            Some(next.updated_at.as_str())
        );
        assert_eq!(checkpoints.len(), 2);
        assert!(checkpoints.contains(&consumed_checkpoint));
        assert!(checkpoints.contains(&resumed_checkpoint));
    }

    #[test]
    fn checkpoint_retention_should_prune_terminal_runs_in_bounded_batches() {
        let stores = WorkflowStoreSet::new(compozy_conn());

        let mut terminal = sample_run_record("run-terminal-old");
        terminal.status = WorkflowRunStatus::Completed;
        terminal.completed_at = Some("2025-01-01T00:00:00Z".to_string());
        terminal.updated_at = "2025-01-01T00:00:00Z".to_string();

        stores
            .workflow_run
            .insert_run(&terminal)
            .expect("insert terminal run");
        for index in 0..1200 {
            stores
                .workflow_checkpoint
                .append(&sample_checkpoint(
                    &terminal.run_id,
                    Some("archive"),
                    CheckpointKind::StepCompleted,
                    &format!("2025-01-01T00:{:02}:{:02}Z", (index / 60) % 60, index % 60),
                ))
                .expect("append terminal checkpoint");
        }

        let deleted_first_batch = stores
            .workflow_checkpoint
            .prune_terminal_runs_older_than("2026-01-01T00:00:00Z", 200, 500)
            .expect("first prune batch should succeed");
        let deleted_second_batch = stores
            .workflow_checkpoint
            .prune_terminal_runs_older_than("2026-01-01T00:00:00Z", 200, 500)
            .expect("second prune batch should succeed");
        let deleted_third_batch = stores
            .workflow_checkpoint
            .prune_terminal_runs_older_than("2026-01-01T00:00:00Z", 200, 500)
            .expect("final prune batch should succeed");

        let remaining = stores
            .workflow_checkpoint
            .list_for_run(&terminal.run_id)
            .expect("remaining checkpoints should load");

        assert_eq!(deleted_first_batch, 500);
        assert_eq!(deleted_second_batch, 500);
        assert_eq!(deleted_third_batch, 0);
        assert_eq!(remaining.len(), 200);
    }

    #[test]
    fn checkpoint_retention_should_ignore_active_and_recent_runs() {
        let stores = WorkflowStoreSet::new(compozy_conn());

        let mut active = sample_run_record("run-active");
        active.status = WorkflowRunStatus::Running;
        active.completed_at = None;
        active.updated_at = "2026-03-26T12:00:00Z".to_string();

        let mut recent_terminal = sample_run_record("run-terminal-recent");
        recent_terminal.status = WorkflowRunStatus::Failed;
        recent_terminal.completed_at = Some("2026-03-20T10:00:00Z".to_string());
        recent_terminal.updated_at = "2026-03-20T10:00:00Z".to_string();

        stores
            .workflow_run
            .insert_run(&active)
            .expect("insert active run");
        stores
            .workflow_run
            .insert_run(&recent_terminal)
            .expect("insert recent terminal run");

        for index in 0..80 {
            let minute = (index / 60) % 60;
            let second = index % 60;
            stores
                .workflow_checkpoint
                .append(&sample_checkpoint(
                    &active.run_id,
                    Some("step-active"),
                    CheckpointKind::StepStarted,
                    &format!("2026-03-26T12:{minute:02}:{second:02}Z"),
                ))
                .expect("append active checkpoint");
            stores
                .workflow_checkpoint
                .append(&sample_checkpoint(
                    &recent_terminal.run_id,
                    Some("step-terminal"),
                    CheckpointKind::StepFailed,
                    &format!("2026-03-20T10:{minute:02}:{second:02}Z"),
                ))
                .expect("append recent terminal checkpoint");
        }

        let deleted = stores
            .workflow_checkpoint
            .prune_terminal_runs_older_than("2026-03-01T00:00:00Z", 10, 500)
            .expect("prune should succeed");
        let active_checkpoints = stores
            .workflow_checkpoint
            .list_for_run(&active.run_id)
            .expect("active checkpoints should load");
        let recent_terminal_checkpoints = stores
            .workflow_checkpoint
            .list_for_run(&recent_terminal.run_id)
            .expect("recent terminal checkpoints should load");

        assert_eq!(deleted, 0);
        assert_eq!(active_checkpoints.len(), 80);
        assert_eq!(recent_terminal_checkpoints.len(), 80);
    }

    #[test]
    fn workflow_checkpoint_repository_should_return_checkpoints_in_created_at_order() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let run = sample_run_record("run-checkpoints");

        stores
            .workflow_run
            .insert_run(&run)
            .expect("insert workflow run");
        stores
            .workflow_checkpoint
            .append(&sample_checkpoint(
                &run.run_id,
                Some("step-2"),
                CheckpointKind::StepCompleted,
                "2026-03-23T12:10:00Z",
            ))
            .expect("append newer checkpoint");
        stores
            .workflow_checkpoint
            .append(&sample_checkpoint(
                &run.run_id,
                Some("step-1"),
                CheckpointKind::StepStarted,
                "2026-03-23T12:01:00Z",
            ))
            .expect("append older checkpoint");

        let checkpoints = stores
            .workflow_checkpoint
            .list_for_run(&run.run_id)
            .expect("list checkpoints");

        assert_eq!(checkpoints.len(), 2);
        assert_eq!(checkpoints[0].kind, CheckpointKind::StepStarted);
        assert_eq!(checkpoints[1].kind, CheckpointKind::StepCompleted);
    }
}
