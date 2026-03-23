//! Typed compozy.db stores for durable workflow runtime state.

use chrono::Utc;
use openfang_types::error::OpenFangError;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;
use uuid::Uuid;

/// SQL for migration `0002_workflow_run_core`.
pub const WORKFLOW_RUN_CORE_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260321_002_workflow_run_core.sql");

/// SQL for migration `0003_workflow_checkpoint`.
pub const WORKFLOW_CHECKPOINT_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260321_003_workflow_checkpoint.sql");

/// SQL for migration `0004_workflow_signal`.
pub const WORKFLOW_SIGNAL_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260321_004_workflow_signal.sql");

/// Shared compozy.db store handles.
#[derive(Clone)]
pub struct WorkflowStoreSet {
    connection: Arc<Mutex<Connection>>,
    /// Store for durable workflow runs.
    pub workflow_run: WorkflowRunStore,
    /// Store for workflow checkpoints.
    pub workflow_checkpoint: WorkflowCheckpointStore,
    /// Store for durable workflow signals.
    pub workflow_signal: WorkflowSignalStore,
}

impl WorkflowStoreSet {
    /// Create the full workflow store set from a shared compozy.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            connection: Arc::clone(&conn),
            workflow_run: WorkflowRunStore::new(Arc::clone(&conn)),
            workflow_checkpoint: WorkflowCheckpointStore::new(Arc::clone(&conn)),
            workflow_signal: WorkflowSignalStore::new(conn),
        }
    }

    /// Return the underlying compozy.db handle for health probes.
    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        Arc::clone(&self.connection)
    }
}

/// Typed failures from the workflow store layer.
#[derive(Debug, Error)]
pub enum WorkflowStoreError {
    /// Failed to acquire the compozy.db connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested workflow run does not exist.
    #[error("workflow run '{run_id}' was not found")]
    RunNotFound { run_id: String },
    /// The requested workflow signal does not exist.
    #[error("workflow signal '{signal_id}' was not found")]
    SignalNotFound { signal_id: String },
}

impl From<WorkflowStoreError> for OpenFangError {
    fn from(error: WorkflowStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

/// Durable workflow status stored in `workflow_run.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkflowRunStatus {
    /// The run has been created but not yet started.
    Pending,
    /// The run is currently executing.
    Running,
    /// The run is waiting for an external signal.
    WaitingSignal,
    /// The run was paused and requires an explicit resume.
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
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::fmt::Display for WorkflowRunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Durable workflow checkpoint kinds stored in `workflow_checkpoint.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckpointKind {
    /// The workflow run row was created.
    RunCreated,
    /// The workflow run moved to running.
    RunStarted,
    /// The current step selection changed.
    StepSelected,
    /// The run is waiting for a signal.
    WaitingSignal,
    /// A signal was delivered to the run.
    SignalReceived,
    /// The run was paused.
    RunPaused,
    /// The run was resumed.
    RunResumed,
    /// The run completed successfully.
    RunCompleted,
    /// The run failed.
    RunFailed,
    /// The run was recovered on boot and needs an explicit resume.
    RunRecoveredNeedsResume,
}

impl CheckpointKind {
    /// Return the SQLite string encoding for the checkpoint kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RunCreated => "run_created",
            Self::RunStarted => "run_started",
            Self::StepSelected => "step_selected",
            Self::WaitingSignal => "waiting_signal",
            Self::SignalReceived => "signal_received",
            Self::RunPaused => "run_paused",
            Self::RunResumed => "run_resumed",
            Self::RunCompleted => "run_completed",
            Self::RunFailed => "run_failed",
            Self::RunRecoveredNeedsResume => "run_recovered_needs_resume",
        }
    }
}

impl std::fmt::Display for CheckpointKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Durable workflow run record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowRunRecord {
    /// Stable run identifier.
    pub run_id: String,
    /// Stable workflow definition identifier.
    pub workflow_id: String,
    /// Optional workflow definition version.
    pub workflow_version: Option<String>,
    /// Current durable run status.
    pub status: WorkflowRunStatus,
    /// Workflow input payload encoded as JSON text.
    pub input_json: String,
    /// Durable workflow variables encoded as JSON text.
    pub vars_json: String,
    /// Currently selected step, if any.
    pub current_step_id: Option<String>,
    /// Waiting state kind, if the run is blocked on an external event.
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
    /// Workflow start timestamp, if execution began.
    pub started_at: Option<String>,
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
    /// Optional source identifier.
    pub source: Option<String>,
    /// Whether the signal was consumed.
    pub consumed: bool,
    /// Creation timestamp.
    pub created_at: String,
    /// Consumption timestamp, if already consumed.
    pub consumed_at: Option<String>,
}

/// Atomic status transition and checkpoint append request.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowTransitionRequest {
    /// New durable status.
    pub new_status: WorkflowRunStatus,
    /// Optional step ID to set or preserve when absent.
    pub step_id: Option<String>,
    /// Stable checkpoint identifier.
    pub checkpoint_id: String,
    /// Checkpoint kind to append.
    pub checkpoint_kind: CheckpointKind,
    /// JSON checkpoint payload.
    pub checkpoint_data: Value,
    /// Timestamp to store on the run and checkpoint.
    pub updated_at: String,
    /// Optional terminal timestamp for completed/failed/cancelled transitions.
    pub completed_at: Option<String>,
}

/// Store for `workflow_run`.
#[derive(Clone)]
pub struct WorkflowRunStore {
    conn: Arc<Mutex<Connection>>,
}

/// Store for `workflow_checkpoint`.
#[derive(Clone)]
pub struct WorkflowCheckpointStore {
    conn: Arc<Mutex<Connection>>,
}

/// Store for `workflow_signal`.
#[derive(Clone)]
pub struct WorkflowSignalStore {
    conn: Arc<Mutex<Connection>>,
}

impl WorkflowRunStore {
    /// Create a workflow run store from a shared compozy.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable workflow run and append the initial `run_created` checkpoint
    /// in a single SQLite transaction.
    pub fn create_run(&self, record: &WorkflowRunRecord) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        insert_workflow_run(&transaction, record)?;

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: Uuid::new_v4().to_string(),
            run_id: record.run_id.clone(),
            step_id: record.current_step_id.clone(),
            kind: CheckpointKind::RunCreated,
            data_json: "{}".to_string(),
            created_at: record.updated_at.clone(),
        };
        insert_checkpoint(&transaction, &checkpoint)?;
        transaction.commit()?;
        Ok(())
    }

    /// Load one workflow run by ID.
    pub fn get_run(&self, run_id: &str) -> Result<Option<WorkflowRunRecord>, WorkflowStoreError> {
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
             WHERE run_id = ?1",
        )?;

        stmt.query_row([run_id], read_workflow_run_row)
            .optional()
            .map_err(Into::into)
    }

    /// List all workflow runs ordered by most recent update first.
    pub fn list_runs(&self) -> Result<Vec<WorkflowRunRecord>, WorkflowStoreError> {
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
             ORDER BY updated_at DESC, run_id DESC",
        )?;
        let rows = stmt.query_map([], read_workflow_run_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// Update only the durable workflow status and touch `updated_at`.
    pub fn update_run_status(
        &self,
        run_id: &str,
        status: WorkflowRunStatus,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let updated_at = now_timestamp();
        let rows = conn.execute(
            "UPDATE workflow_run
             SET status = ?1,
                 updated_at = ?2,
                 started_at = CASE
                     WHEN ?1 = 'running' AND started_at IS NULL THEN ?2
                     ELSE started_at
                 END,
                 completed_at = CASE
                     WHEN ?1 IN ('completed', 'failed', 'cancelled') THEN ?2
                     ELSE completed_at
                 END
             WHERE run_id = ?3",
            params![status.as_str(), updated_at.as_str(), run_id],
        )?;
        ensure_row_updated(rows, run_id)?;
        Ok(())
    }

    /// Update the durable waiting state and touch `updated_at`.
    pub fn update_run_waiting_state(
        &self,
        run_id: &str,
        waiting_kind: Option<&str>,
        waiting_ref: Option<&str>,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let updated_at = now_timestamp();
        let rows = conn.execute(
            "UPDATE workflow_run
             SET waiting_kind = ?1,
                 waiting_ref = ?2,
                 updated_at = ?3
             WHERE run_id = ?4",
            params![waiting_kind, waiting_ref, updated_at.as_str(), run_id],
        )?;
        ensure_row_updated(rows, run_id)?;
        Ok(())
    }

    /// Atomically mutate a workflow run and append a checkpoint.
    pub fn transition(
        &self,
        run_id: &str,
        transition: &WorkflowTransitionRequest,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction()?;
        let rows = transaction.execute(
            "UPDATE workflow_run
             SET status = ?1,
                 current_step_id = COALESCE(?2, current_step_id),
                 updated_at = ?3,
                 started_at = CASE
                     WHEN ?1 = 'running' AND started_at IS NULL THEN ?3
                     ELSE started_at
                 END,
                 completed_at = CASE
                     WHEN ?1 IN ('completed', 'failed', 'cancelled')
                         THEN COALESCE(?4, ?3)
                     ELSE completed_at
                 END
             WHERE run_id = ?5",
            params![
                transition.new_status.as_str(),
                transition.step_id.as_deref(),
                transition.updated_at.as_str(),
                transition.completed_at.as_deref(),
                run_id,
            ],
        )?;
        ensure_row_updated(rows, run_id)?;

        let checkpoint = WorkflowCheckpointRecord {
            checkpoint_id: transition.checkpoint_id.clone(),
            run_id: run_id.to_string(),
            step_id: transition.step_id.clone(),
            kind: transition.checkpoint_kind,
            data_json: serde_json::to_string(&transition.checkpoint_data)?,
            created_at: transition.updated_at.clone(),
        };
        insert_checkpoint(&transaction, &checkpoint)?;
        transaction.commit()?;
        Ok(())
    }

    /// Downgrade all `running` rows to `paused` and append a recovery checkpoint.
    pub fn recover_running_runs(&self) -> Result<usize, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let running_runs = {
            let mut stmt = conn.prepare(
                "SELECT run_id, current_step_id
                 FROM workflow_run
                 WHERE status = 'running'
                 ORDER BY updated_at ASC, run_id ASC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?;
            let mut run_ids = Vec::new();
            for row in rows {
                run_ids.push(row?);
            }
            run_ids
        };

        if running_runs.is_empty() {
            return Ok(0);
        }

        let transaction = conn.unchecked_transaction()?;
        for (run_id, step_id) in &running_runs {
            let updated_at = now_timestamp();
            let rows = transaction.execute(
                "UPDATE workflow_run
                 SET status = 'paused',
                     updated_at = ?1
                 WHERE run_id = ?2",
                params![updated_at.as_str(), run_id],
            )?;
            ensure_row_updated(rows, run_id)?;

            let checkpoint = WorkflowCheckpointRecord {
                checkpoint_id: Uuid::new_v4().to_string(),
                run_id: run_id.clone(),
                step_id: step_id.clone(),
                kind: CheckpointKind::RunRecoveredNeedsResume,
                data_json: "{}".to_string(),
                created_at: updated_at,
            };
            insert_checkpoint(&transaction, &checkpoint)?;
        }
        transaction.commit()?;

        Ok(running_runs.len())
    }
}

impl WorkflowCheckpointStore {
    /// Create a workflow checkpoint store from a shared compozy.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Append one checkpoint row.
    pub fn append_checkpoint(
        &self,
        checkpoint: &WorkflowCheckpointRecord,
    ) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        insert_checkpoint(&conn, checkpoint)?;
        Ok(())
    }

    /// List checkpoints for one run ordered by creation time.
    pub fn list_checkpoints_for_run(
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

impl WorkflowSignalStore {
    /// Create a workflow signal store from a shared compozy.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable signal row.
    pub fn insert_signal(&self, signal: &WorkflowSignalRecord) -> Result<(), WorkflowStoreError> {
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
                signal.source.as_deref(),
                bool_to_sql(signal.consumed),
                signal.created_at.as_str(),
                signal.consumed_at.as_deref(),
            ],
        )?;
        Ok(())
    }

    /// List pending signals for a run ordered by creation time.
    pub fn list_pending_signals_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<WorkflowSignalRecord>, WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn.prepare(
            "SELECT signal_id, run_id, name, payload_json, source, consumed, created_at, consumed_at
             FROM workflow_signal
             WHERE run_id = ?1 AND consumed = 0
             ORDER BY created_at ASC, signal_id ASC",
        )?;
        let rows = stmt.query_map([run_id], read_workflow_signal_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(row?);
        }
        Ok(records)
    }

    /// Mark one signal as consumed and set `consumed_at = datetime('now')`.
    pub fn mark_signal_consumed(&self, signal_id: &str) -> Result<(), WorkflowStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute(
            "UPDATE workflow_signal
             SET consumed = 1,
                 consumed_at = datetime('now')
             WHERE signal_id = ?1 AND consumed = 0",
            [signal_id],
        )?;

        if rows > 0 {
            return Ok(());
        }

        let exists = conn
            .query_row(
                "SELECT 1 FROM workflow_signal WHERE signal_id = ?1",
                [signal_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?;

        if exists.is_some() {
            return Ok(());
        }

        Err(WorkflowStoreError::SignalNotFound {
            signal_id: signal_id.to_string(),
        })
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
            record.workflow_version.as_deref(),
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
            record.started_at.as_deref(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
        ],
    )?;
    Ok(())
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

fn read_workflow_run_row(row: &rusqlite::Row<'_>) -> Result<WorkflowRunRecord, rusqlite::Error> {
    Ok(WorkflowRunRecord {
        run_id: row.get(0)?,
        workflow_id: row.get(1)?,
        workflow_version: row.get(2)?,
        status: decode_workflow_run_status(&row.get::<_, String>(3)?)?,
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
    Ok(WorkflowCheckpointRecord {
        checkpoint_id: row.get(0)?,
        run_id: row.get(1)?,
        step_id: row.get(2)?,
        kind: decode_checkpoint_kind(&row.get::<_, String>(3)?)?,
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

fn decode_workflow_run_status(value: &str) -> Result<WorkflowRunStatus, rusqlite::Error> {
    match value {
        "pending" => Ok(WorkflowRunStatus::Pending),
        "running" => Ok(WorkflowRunStatus::Running),
        "waiting_signal" => Ok(WorkflowRunStatus::WaitingSignal),
        "paused" => Ok(WorkflowRunStatus::Paused),
        "completed" => Ok(WorkflowRunStatus::Completed),
        "failed" => Ok(WorkflowRunStatus::Failed),
        "cancelled" => Ok(WorkflowRunStatus::Cancelled),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("invalid workflow run status '{other}'").into(),
        )),
    }
}

fn decode_checkpoint_kind(value: &str) -> Result<CheckpointKind, rusqlite::Error> {
    match value {
        "run_created" => Ok(CheckpointKind::RunCreated),
        "run_started" => Ok(CheckpointKind::RunStarted),
        "step_selected" => Ok(CheckpointKind::StepSelected),
        "waiting_signal" => Ok(CheckpointKind::WaitingSignal),
        "signal_received" => Ok(CheckpointKind::SignalReceived),
        "run_paused" => Ok(CheckpointKind::RunPaused),
        "run_resumed" => Ok(CheckpointKind::RunResumed),
        "run_completed" => Ok(CheckpointKind::RunCompleted),
        "run_failed" => Ok(CheckpointKind::RunFailed),
        "run_recovered_needs_resume" => Ok(CheckpointKind::RunRecoveredNeedsResume),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("invalid workflow checkpoint kind '{other}'").into(),
        )),
    }
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

fn now_timestamp() -> String {
    Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn compozy_conn() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open in-memory compozy.db");
        conn.execute_batch(WORKFLOW_RUN_CORE_MIGRATION_SQL)
            .expect("apply workflow_run schema");
        conn.execute_batch(WORKFLOW_CHECKPOINT_MIGRATION_SQL)
            .expect("apply workflow_checkpoint schema");
        conn.execute_batch(WORKFLOW_SIGNAL_MIGRATION_SQL)
            .expect("apply workflow_signal schema");
        Arc::new(Mutex::new(conn))
    }

    fn sample_run_record(run_id: &str) -> WorkflowRunRecord {
        WorkflowRunRecord {
            run_id: run_id.to_string(),
            workflow_id: Uuid::new_v4().to_string(),
            workflow_version: Some("2026.03.23".to_string()),
            status: WorkflowRunStatus::Pending,
            input_json: "\"hello world\"".to_string(),
            vars_json: "{}".to_string(),
            current_step_id: None,
            waiting_kind: None,
            waiting_ref: None,
            active_dispatch_id: None,
            active_hitl_request_id: None,
            labels_json: "[]".to_string(),
            metadata_json: "{}".to_string(),
            error_json: None,
            started_at: None,
            updated_at: "2026-03-23T12:00:00Z".to_string(),
            completed_at: None,
        }
    }

    #[test]
    fn workflow_run_store_should_create_run_and_write_run_created_checkpoint() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let record = sample_run_record("run-create");

        stores
            .workflow_run
            .create_run(&record)
            .expect("create workflow run");

        let loaded = stores
            .workflow_run
            .get_run(&record.run_id)
            .expect("load workflow run")
            .expect("workflow run should exist");
        let checkpoints = stores
            .workflow_checkpoint
            .list_checkpoints_for_run(&record.run_id)
            .expect("load checkpoints");

        assert_eq!(loaded, record);
        assert_eq!(checkpoints.len(), 1);
        assert_eq!(checkpoints[0].kind, CheckpointKind::RunCreated);
        assert_eq!(checkpoints[0].run_id, record.run_id);
    }

    #[test]
    fn workflow_run_store_transition_should_write_run_and_checkpoint_atomically() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let record = sample_run_record("run-transition");
        let checkpoint_id = "checkpoint-conflict".to_string();

        stores
            .workflow_run
            .create_run(&record)
            .expect("create workflow run");
        stores
            .workflow_checkpoint
            .append_checkpoint(&WorkflowCheckpointRecord {
                checkpoint_id: checkpoint_id.clone(),
                run_id: record.run_id.clone(),
                step_id: None,
                kind: CheckpointKind::StepSelected,
                data_json: "{}".to_string(),
                created_at: "2026-03-23T12:00:01Z".to_string(),
            })
            .expect("insert conflicting checkpoint");

        let result = stores.workflow_run.transition(
            &record.run_id,
            &WorkflowTransitionRequest {
                new_status: WorkflowRunStatus::Completed,
                step_id: Some("final-step".to_string()),
                checkpoint_id,
                checkpoint_kind: CheckpointKind::RunCompleted,
                checkpoint_data: json!({"result":"ok"}),
                updated_at: "2026-03-23T12:00:02Z".to_string(),
                completed_at: Some("2026-03-23T12:00:02Z".to_string()),
            },
        );

        assert!(
            result.is_err(),
            "transition should fail on checkpoint conflict"
        );

        let loaded = stores
            .workflow_run
            .get_run(&record.run_id)
            .expect("load workflow run")
            .expect("workflow run should exist");
        let checkpoints = stores
            .workflow_checkpoint
            .list_checkpoints_for_run(&record.run_id)
            .expect("load checkpoints");

        assert_eq!(loaded.status, WorkflowRunStatus::Pending);
        assert_eq!(loaded.current_step_id, None);
        assert_eq!(loaded.completed_at, None);
        assert_eq!(checkpoints.len(), 2);
        assert_eq!(
            checkpoints
                .iter()
                .filter(|checkpoint| checkpoint.kind == CheckpointKind::RunCompleted)
                .count(),
            0
        );
    }

    #[test]
    fn workflow_signal_store_should_insert_and_consume_signals() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let run_record = sample_run_record("run-signal");
        let first_signal = WorkflowSignalRecord {
            signal_id: "signal-one".to_string(),
            run_id: run_record.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"answer\":\"yes\"}".to_string(),
            source: Some("human".to_string()),
            consumed: false,
            created_at: "2026-03-23T12:05:00Z".to_string(),
            consumed_at: None,
        };
        let second_signal = WorkflowSignalRecord {
            signal_id: "signal-two".to_string(),
            run_id: run_record.run_id.clone(),
            name: "approval".to_string(),
            payload_json: "{\"answer\":\"no\"}".to_string(),
            source: Some("human".to_string()),
            consumed: false,
            created_at: "2026-03-23T12:06:00Z".to_string(),
            consumed_at: None,
        };

        stores
            .workflow_run
            .create_run(&run_record)
            .expect("create workflow run");
        stores
            .workflow_signal
            .insert_signal(&first_signal)
            .expect("insert first signal");
        stores
            .workflow_signal
            .insert_signal(&second_signal)
            .expect("insert second signal");
        stores
            .workflow_signal
            .mark_signal_consumed(&first_signal.signal_id)
            .expect("consume first signal");

        let pending = stores
            .workflow_signal
            .list_pending_signals_for_run(&run_record.run_id)
            .expect("list pending signals");
        let conn = lock_conn(&stores.connection).expect("lock workflow connection");
        let consumed_row = conn
            .query_row(
                "SELECT consumed, consumed_at FROM workflow_signal WHERE signal_id = ?1",
                [first_signal.signal_id.as_str()],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
            )
            .expect("load consumed signal");

        assert_eq!(pending, vec![second_signal]);
        assert_eq!(consumed_row.0, 1);
        assert!(consumed_row.1.is_some(), "consumed_at should be populated");
    }

    #[test]
    fn workflow_checkpoint_store_should_return_checkpoints_in_created_at_order() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let record = sample_run_record("run-checkpoints");
        stores
            .workflow_run
            .create_run(&record)
            .expect("create workflow run");

        let newer = WorkflowCheckpointRecord {
            checkpoint_id: "checkpoint-newer".to_string(),
            run_id: record.run_id.clone(),
            step_id: Some("step-two".to_string()),
            kind: CheckpointKind::StepSelected,
            data_json: "{}".to_string(),
            created_at: "2026-03-23T12:10:00Z".to_string(),
        };
        let older = WorkflowCheckpointRecord {
            checkpoint_id: "checkpoint-older".to_string(),
            run_id: record.run_id.clone(),
            step_id: Some("step-one".to_string()),
            kind: CheckpointKind::RunStarted,
            data_json: "{}".to_string(),
            created_at: "2026-03-23T12:01:00Z".to_string(),
        };

        stores
            .workflow_checkpoint
            .append_checkpoint(&newer)
            .expect("append newer checkpoint");
        stores
            .workflow_checkpoint
            .append_checkpoint(&older)
            .expect("append older checkpoint");

        let checkpoints = stores
            .workflow_checkpoint
            .list_checkpoints_for_run(&record.run_id)
            .expect("list checkpoints");

        assert_eq!(checkpoints[0].kind, CheckpointKind::RunCreated);
        assert_eq!(checkpoints[1], older);
        assert_eq!(checkpoints[2], newer);
    }

    #[test]
    fn recovery_scan_should_downgrade_running_to_paused_and_write_checkpoint() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let record = sample_run_record("run-recovery");

        stores
            .workflow_run
            .create_run(&record)
            .expect("create workflow run");
        stores
            .workflow_run
            .update_run_status(&record.run_id, WorkflowRunStatus::Running)
            .expect("mark run as running");

        let recovered = stores
            .workflow_run
            .recover_running_runs()
            .expect("recover running runs");
        let loaded = stores
            .workflow_run
            .get_run(&record.run_id)
            .expect("load workflow run")
            .expect("workflow run should exist");
        let checkpoints = stores
            .workflow_checkpoint
            .list_checkpoints_for_run(&record.run_id)
            .expect("list checkpoints");

        assert_eq!(recovered, 1);
        assert_eq!(loaded.status, WorkflowRunStatus::Paused);
        assert_eq!(
            checkpoints
                .iter()
                .filter(|checkpoint| checkpoint.kind == CheckpointKind::RunRecoveredNeedsResume)
                .count(),
            1
        );
    }

    #[test]
    fn recovery_scan_should_not_modify_waiting_signal_runs() {
        let stores = WorkflowStoreSet::new(compozy_conn());
        let record = sample_run_record("run-waiting");

        stores
            .workflow_run
            .create_run(&record)
            .expect("create workflow run");
        stores
            .workflow_run
            .update_run_waiting_state(&record.run_id, Some("signal"), Some("approval-1"))
            .expect("set waiting state");
        stores
            .workflow_run
            .update_run_status(&record.run_id, WorkflowRunStatus::WaitingSignal)
            .expect("mark run as waiting_signal");

        let recovered = stores
            .workflow_run
            .recover_running_runs()
            .expect("recover running runs");
        let loaded = stores
            .workflow_run
            .get_run(&record.run_id)
            .expect("load workflow run")
            .expect("workflow run should exist");

        assert_eq!(recovered, 0);
        assert_eq!(loaded.status, WorkflowRunStatus::WaitingSignal);
        assert_eq!(loaded.waiting_kind.as_deref(), Some("signal"));
        assert_eq!(loaded.waiting_ref.as_deref(), Some("approval-1"));
    }
}
