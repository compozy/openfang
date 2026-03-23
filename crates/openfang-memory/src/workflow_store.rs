//! Typed `compozy.db` repositories for durable workflow runtime state.

use chrono::Utc;
use openfang_types::error::OpenFangError;
use rusqlite::{params, Connection, OptionalExtension};
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
}

impl WorkflowStoreSet {
    /// Create the full workflow repository set from a shared `compozy.db`
    /// connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            connection: Arc::clone(&conn),
            workflow_run: WorkflowRunRepository::new(Arc::clone(&conn)),
            workflow_checkpoint: WorkflowCheckpointRepository::new(Arc::clone(&conn)),
            workflow_signal: WorkflowSignalRepository::new(conn),
        }
    }

    /// Return the underlying `compozy.db` handle for health probes.
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.connection)
    }
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
    /// The requested workflow run does not exist.
    #[error("workflow run '{run_id}' was not found")]
    RunNotFound { run_id: String },
    /// The requested workflow signal does not exist.
    #[error("workflow signal '{signal_id}' was not found")]
    SignalNotFound { signal_id: String },
    /// The requested workflow signal was already consumed.
    #[error("workflow signal '{signal_id}' was already consumed")]
    SignalAlreadyConsumed { signal_id: String },
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
    Waiting,
    /// The run completed successfully.
    Completed,
    /// The run failed.
    Failed,
    /// The run was cancelled.
    Cancelled,
    /// The run was interrupted by process restart or crash recovery.
    Interrupted,
}

impl WorkflowRunStatus {
    /// Return the SQLite string encoding for the status.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse_db_text(value: &str) -> Result<Self, WorkflowStoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            "interrupted" => Ok(Self::Interrupted),
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
    /// The run completed successfully.
    RunCompleted,
    /// The run failed.
    RunFailed,
    /// The run was cancelled.
    RunCancelled,
    /// The run was interrupted during restart recovery.
    RunInterrupted,
    /// Legacy Task 9 checkpoint kind kept for migration compatibility.
    StepSelected,
    /// Legacy Task 9 checkpoint kind kept for migration compatibility.
    WaitingSignal,
    /// Legacy Task 9 checkpoint kind kept for migration compatibility.
    RunPaused,
    /// Legacy Task 9 checkpoint kind kept for migration compatibility.
    RunResumed,
    /// Legacy Task 9 checkpoint kind kept for migration compatibility.
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
            Self::RunCompleted => "run_completed",
            Self::RunFailed => "run_failed",
            Self::RunCancelled => "run_cancelled",
            Self::RunInterrupted => "run_interrupted",
            Self::StepSelected => "step_selected",
            Self::WaitingSignal => "waiting_signal",
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
            "run_completed" => Ok(Self::RunCompleted),
            "run_failed" => Ok(Self::RunFailed),
            "run_cancelled" => Ok(Self::RunCancelled),
            "run_interrupted" => Ok(Self::RunInterrupted),
            "step_selected" => Ok(Self::StepSelected),
            "waiting_signal" => Ok(Self::WaitingSignal),
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
             WHERE status IN ('pending', 'running', 'waiting')
             ORDER BY updated_at ASC, run_id ASC",
        )?;
        let rows = stmt.query_map([], read_workflow_run_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
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

    /// Append a checkpoint, mark a signal consumed, and then replace the
    /// durable run row inside one SQLite transaction.
    pub fn persist_signal_resume(
        &self,
        current: &WorkflowRunRecord,
        next: &WorkflowRunRecord,
        checkpoint: &WorkflowCheckpointRecord,
        signal_id: &str,
        consumed_at: &str,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        insert_checkpoint(&transaction, checkpoint)?;
        consume_signal_row(&transaction, signal_id, consumed_at)?;
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
}

impl WorkflowSignalRepository {
    /// Create a signal repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable signal row.
    pub fn insert(&self, signal: &WorkflowSignalRecord) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO workflow_signal (
                signal_id,
                run_id,
                name,
                payload_json,
                source,
                consumed,
                created_at,
                consumed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                signal.signal_id.as_str(),
                signal.run_id.as_str(),
                signal.name.as_str(),
                signal.payload_json.as_str(),
                signal.source.as_str(),
                bool_to_sql(signal.consumed),
                signal.created_at.as_str(),
                signal.consumed_at.as_deref(),
            ],
        )?;
        Ok(())
    }

    /// Load one durable signal by ID.
    pub fn find_by_id(
        &self,
        signal_id: &str,
    ) -> Result<Option<WorkflowSignalRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        conn.query_row(
            "SELECT signal_id, run_id, name, payload_json, source, consumed, created_at, consumed_at
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
        let mut stmt = conn.prepare(
            "SELECT signal_id, run_id, name, payload_json, source, consumed, created_at, consumed_at
             FROM workflow_signal
             WHERE run_id = ?1
             ORDER BY created_at ASC, signal_id ASC",
        )?;
        let rows = stmt.query_map([run_id], read_workflow_signal_row)?;
        let mut records = Vec::new();
        for row in rows {
            let record = row?;
            if consumed
                .map(|expected| record.consumed == expected)
                .unwrap_or(true)
            {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// Return the first unconsumed signal matching the run and name.
    pub fn find_unconsumed(
        &self,
        run_id: &str,
        name: &str,
    ) -> Result<Option<WorkflowSignalRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        conn.query_row(
            "SELECT signal_id, run_id, name, payload_json, source, consumed, created_at, consumed_at
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
        consumed: sql_to_bool(row.get::<_, i64>(5)?),
        created_at: row.get(6)?,
        consumed_at: row.get(7)?,
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
        let mut completed = sample_run_record("run-completed");

        running.status = WorkflowRunStatus::Running;
        running.updated_at = "2026-03-23T12:01:00Z".to_string();
        waiting.status = WorkflowRunStatus::Waiting;
        waiting.waiting_kind = Some("signal".to_string());
        waiting.waiting_ref = Some("artifact-approved".to_string());
        waiting.updated_at = "2026-03-23T12:02:00Z".to_string();
        completed.status = WorkflowRunStatus::Completed;
        completed.completed_at = Some("2026-03-23T12:03:00Z".to_string());
        completed.updated_at = "2026-03-23T12:03:00Z".to_string();

        for record in [&pending, &running, &waiting, &completed] {
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
            ]
        );
    }

    #[test]
    fn workflow_signal_repository_should_insert_list_and_consume_signals() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let run = sample_run_record("run-signal");
        let first_signal = WorkflowSignalRecord {
            signal_id: "signal-one".to_string(),
            run_id: run.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"answer\":\"yes\"}".to_string(),
            source: "api".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:05:00Z".to_string(),
            consumed_at: None,
        };
        let second_signal = WorkflowSignalRecord {
            signal_id: "signal-two".to_string(),
            run_id: run.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"answer\":\"no\"}".to_string(),
            source: "api".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:06:00Z".to_string(),
            consumed_at: None,
        };

        stores
            .workflow_run
            .insert_run(&run)
            .expect("insert workflow run");
        stores
            .workflow_signal
            .insert(&first_signal)
            .expect("insert first signal");
        stores
            .workflow_signal
            .insert(&second_signal)
            .expect("insert second signal");
        stores
            .workflow_signal
            .consume(&first_signal.signal_id, "2026-03-23T12:07:00Z")
            .expect("consume first signal");

        let unconsumed = stores
            .workflow_signal
            .list_for_run(&run.run_id, Some(false))
            .expect("list unconsumed signals");
        let consumed = stores
            .workflow_signal
            .find_by_id(&first_signal.signal_id)
            .expect("load consumed signal")
            .expect("signal should exist");

        assert_eq!(unconsumed, vec![second_signal]);
        assert!(consumed.consumed);
        assert_eq!(
            consumed.consumed_at.as_deref(),
            Some("2026-03-23T12:07:00Z")
        );
    }

    #[test]
    fn workflow_run_repository_should_consume_signal_when_persisting_resume_transition() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let mut current = sample_run_record("run-resume");
        current.status = WorkflowRunStatus::Waiting;
        current.current_step_id = Some("await-approval".to_string());
        current.waiting_kind = Some("signal".to_string());
        current.waiting_ref = Some("approval".to_string());
        current.updated_at = "2026-03-23T12:08:00Z".to_string();
        let signal = WorkflowSignalRecord {
            signal_id: "signal-resume".to_string(),
            run_id: current.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"decision\":\"approved\"}".to_string(),
            source: "api".to_string(),
            consumed: false,
            created_at: "2026-03-23T12:08:30Z".to_string(),
            consumed_at: None,
        };
        let mut next = current.clone();
        next.status = WorkflowRunStatus::Running;
        next.waiting_kind = None;
        next.waiting_ref = None;
        next.updated_at = "2026-03-23T12:09:00Z".to_string();
        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: "chk-run-resume".to_string(),
            run_id: current.run_id.clone(),
            step_id: Some("await-approval".to_string()),
            kind: CheckpointKind::SignalReceived,
            data_json: serde_json::json!({
                "signal_id": signal.signal_id,
                "signal_name": signal.name,
                "payload_summary": "approved",
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
                &checkpoint,
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
        assert!(loaded_signal.consumed);
        assert_eq!(
            loaded_signal.consumed_at.as_deref(),
            Some(next.updated_at.as_str())
        );
        assert_eq!(checkpoints, vec![checkpoint]);
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
