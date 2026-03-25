//! Typed `compozy.db` repositories for durable looper state.

use openfang_types::error::OpenFangError;
use openfang_types::looper::{
    LooperExecutionMode, LooperExecutionPolicy, LooperProgress, LooperRunId, LooperRunListQuery,
    LooperRunRecord, LooperRunStatus, LooperSelectionStrategy, LooperSubtaskId,
    LooperSubtaskRecord, LooperSubtaskStatus,
};
use openfang_types::task::{SubtaskId, SubtaskRecord, SubtaskStatus, TaskId};
use rusqlite::{params, params_from_iter, types::Value as SqlValue, Connection, OptionalExtension};
use serde_json::Value as JsonValue;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;
use uuid::Uuid;

/// SQL for migration `0011_looper_runtime`.
pub const LOOPER_RUNTIME_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260325_011_looper_runtime.sql");

/// Input payload used when creating a durable looper run.
#[derive(Debug, Clone, PartialEq)]
pub struct NewLooperRun {
    /// Stable looper run identifier.
    pub looper_run_id: LooperRunId,
    /// Owning task identifier.
    pub task_id: TaskId,
    /// Producing workflow run identifier, when applicable.
    pub source_run_id: Option<String>,
    /// Initial lifecycle state.
    pub status: LooperRunStatus,
    /// Raw execution policy payload that must validate against the public shape.
    pub execution_policy_json: JsonValue,
    /// Current in-flight subtask, if any.
    pub current_subtask_id: Option<SubtaskId>,
    /// Raw progress payload.
    pub progress_json: JsonValue,
    /// Structured error payload, when applicable.
    pub error_json: Option<JsonValue>,
    /// Start timestamp in RFC 3339 UTC format.
    pub started_at: String,
    /// Last update timestamp in RFC 3339 UTC format.
    pub updated_at: String,
    /// Completion timestamp for terminal states.
    pub completed_at: Option<String>,
}

/// Typed failures from the looper repository layer.
#[derive(Debug, Error)]
pub enum LooperStoreError {
    /// Failed to acquire the shared `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested task does not exist.
    #[error("task '{task_id}' was not found")]
    TaskNotFound { task_id: String },
    /// The requested looper run does not exist.
    #[error("looper run '{looper_run_id}' was not found")]
    RunNotFound { looper_run_id: String },
    /// The requested looper-subtask record does not exist.
    #[error("looper subtask '{looper_subtask_id}' was not found")]
    LooperSubtaskNotFound { looper_subtask_id: String },
    /// The looper run already exists.
    #[error("looper run '{looper_run_id}' already exists")]
    RunAlreadyExists { looper_run_id: String },
    /// The looper subtask row already exists for the run and canonical subtask.
    #[error("looper subtask for run '{looper_run_id}' and subtask '{subtask_id}' already exists")]
    LooperSubtaskAlreadyExists {
        looper_run_id: String,
        subtask_id: String,
    },
    /// A stored looper run status string was invalid.
    #[error("invalid looper run status '{status}'")]
    InvalidRunStatus { status: String },
    /// A stored looper-subtask status string was invalid.
    #[error("invalid looper subtask status '{status}'")]
    InvalidSubtaskStatus { status: String },
    /// The execution policy was missing a required field.
    #[error("execution_policy_json is missing required field '{field}'")]
    MissingExecutionPolicyField { field: &'static str },
    /// The execution policy contained an unknown execution mode.
    #[error("invalid looper execution mode '{mode}'")]
    InvalidExecutionMode { mode: String },
    /// The execution policy contained an unknown selection strategy.
    #[error("invalid looper selection strategy '{selection}'")]
    InvalidSelectionStrategy { selection: String },
    /// The execution policy declared an invalid maximum parallelism.
    #[error("execution_policy_json.max_parallelism must be at least 1 (got {value})")]
    InvalidMaxParallelism { value: i64 },
    /// A JSON field could not be parsed into the expected shape.
    #[error("invalid JSON in field '{field}': {message}")]
    InvalidJsonField {
        field: &'static str,
        message: String,
    },
    /// The requested status transition was not allowed.
    #[error(
        "looper run '{looper_run_id}' expected current state one of [{expected}], got '{actual}'"
    )]
    UnexpectedRunState {
        looper_run_id: String,
        expected: String,
        actual: String,
    },
    /// The canonical subtask does not belong to the looper run's task.
    #[error(
        "subtask '{subtask_id}' does not belong to looper run '{looper_run_id}' task '{task_id}'"
    )]
    SubtaskOutsideRunTask {
        looper_run_id: String,
        task_id: String,
        subtask_id: String,
    },
}

impl From<LooperStoreError> for OpenFangError {
    fn from(error: LooperStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

/// Repository for `looper_run`.
#[derive(Clone)]
pub struct LooperRunRepository {
    conn: Arc<Mutex<Connection>>,
}

/// Repository for `looper_subtask`.
#[derive(Clone)]
pub struct LooperSubtaskRepository {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug)]
struct LooperRunRow {
    looper_run_id: String,
    task_id: String,
    source_run_id: Option<String>,
    status: String,
    execution_policy_json: String,
    current_subtask_id: Option<String>,
    progress_json: String,
    error_json: Option<String>,
    started_at: String,
    updated_at: String,
    completed_at: Option<String>,
}

#[derive(Debug)]
struct LooperSubtaskRow {
    looper_subtask_id: String,
    looper_run_id: String,
    subtask_id: String,
    status: String,
    dispatch_id: Option<String>,
    result_json: Option<String>,
    error_json: Option<String>,
    updated_at: String,
}

impl LooperRunRepository {
    /// Create the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert a durable looper run row after validating the explicit policy.
    pub fn create(&self, input: &NewLooperRun) -> Result<LooperRunRecord, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        ensure_task_exists(&conn, input.task_id.as_ref())?;
        let record = validate_new_run(input)?;
        insert_looper_run(&conn, &record)?;
        Ok(record)
    }

    /// Load one durable looper run by ID.
    pub fn find_by_id(
        &self,
        looper_run_id: &LooperRunId,
    ) -> Result<Option<LooperRunRecord>, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_looper_run(&conn, looper_run_id.as_ref())
    }

    /// List durable looper runs using the canonical SQLite-backed filters.
    pub fn list(
        &self,
        query: &LooperRunListQuery,
    ) -> Result<Vec<LooperRunRecord>, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_looper_runs(&conn, query)
    }

    /// Update the durable looper run status and optional error payload.
    pub fn update_status(
        &self,
        looper_run_id: &LooperRunId,
        status: LooperRunStatus,
        error: Option<&JsonValue>,
        updated_at: &str,
        completed_at: Option<&str>,
    ) -> Result<LooperRunRecord, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute(
            "UPDATE looper_run
             SET status = ?1,
                 error_json = ?2,
                 updated_at = ?3,
                 completed_at = ?4
             WHERE looper_run_id = ?5",
            params![
                status.as_str(),
                serialize_optional_json(error)?,
                updated_at,
                completed_at,
                looper_run_id.as_ref(),
            ],
        )?;
        ensure_run_updated(rows, looper_run_id)?;
        load_required_looper_run(&conn, looper_run_id.as_ref())
    }

    /// Update the stored looper progress payload.
    pub fn update_progress(
        &self,
        looper_run_id: &LooperRunId,
        progress: LooperProgress,
        updated_at: &str,
    ) -> Result<LooperRunRecord, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute(
            "UPDATE looper_run
             SET progress_json = ?1,
                 updated_at = ?2
             WHERE looper_run_id = ?3",
            params![
                serialize_json_field("progress_json", &progress)?,
                updated_at,
                looper_run_id.as_ref(),
            ],
        )?;
        ensure_run_updated(rows, looper_run_id)?;
        load_required_looper_run(&conn, looper_run_id.as_ref())
    }

    /// Update the current in-flight subtask pointer.
    pub fn set_current_subtask(
        &self,
        looper_run_id: &LooperRunId,
        current_subtask_id: Option<&SubtaskId>,
        updated_at: &str,
    ) -> Result<LooperRunRecord, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute(
            "UPDATE looper_run
             SET current_subtask_id = ?1,
                 updated_at = ?2
             WHERE looper_run_id = ?3",
            params![
                current_subtask_id.map(AsRef::as_ref),
                updated_at,
                looper_run_id.as_ref(),
            ],
        )?;
        ensure_run_updated(rows, looper_run_id)?;
        load_required_looper_run(&conn, looper_run_id.as_ref())
    }

    /// Pause the looper run while allowing in-flight dispatches to settle.
    pub fn pause(
        &self,
        looper_run_id: &LooperRunId,
        updated_at: &str,
    ) -> Result<LooperRunRecord, LooperStoreError> {
        self.transition_status(
            looper_run_id,
            &[LooperRunStatus::Pending, LooperRunStatus::Running],
            LooperRunStatus::Paused,
            updated_at,
            None,
            None,
        )
    }

    /// Resume a previously paused looper run.
    pub fn resume(
        &self,
        looper_run_id: &LooperRunId,
        updated_at: &str,
    ) -> Result<LooperRunRecord, LooperStoreError> {
        self.transition_status(
            looper_run_id,
            &[LooperRunStatus::Paused],
            LooperRunStatus::Running,
            updated_at,
            None,
            None,
        )
    }

    /// Cancel a non-terminal looper run.
    pub fn cancel(
        &self,
        looper_run_id: &LooperRunId,
        reason: Option<&JsonValue>,
        updated_at: &str,
    ) -> Result<LooperRunRecord, LooperStoreError> {
        self.transition_status(
            looper_run_id,
            &[
                LooperRunStatus::Pending,
                LooperRunStatus::Running,
                LooperRunStatus::Paused,
            ],
            LooperRunStatus::Cancelled,
            updated_at,
            reason,
            Some(updated_at),
        )
    }

    fn transition_status(
        &self,
        looper_run_id: &LooperRunId,
        expected: &[LooperRunStatus],
        next_status: LooperRunStatus,
        updated_at: &str,
        error: Option<&JsonValue>,
        completed_at: Option<&str>,
    ) -> Result<LooperRunRecord, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        let current = load_required_looper_run(&conn, looper_run_id.as_ref())?;
        if !expected.contains(&current.status) {
            return Err(LooperStoreError::UnexpectedRunState {
                looper_run_id: looper_run_id.to_string(),
                expected: expected
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
                actual: current.status.to_string(),
            });
        }

        let rows = conn.execute(
            "UPDATE looper_run
             SET status = ?1,
                 error_json = CASE WHEN ?2 = 1 THEN error_json ELSE ?3 END,
                 updated_at = ?4,
                 completed_at = ?5
             WHERE looper_run_id = ?6
               AND status = ?7",
            params![
                next_status.as_str(),
                error.is_none(),
                serialize_optional_json(error)?,
                updated_at,
                completed_at,
                looper_run_id.as_ref(),
                current.status.as_str(),
            ],
        )?;
        if rows == 0 {
            return Err(LooperStoreError::UnexpectedRunState {
                looper_run_id: looper_run_id.to_string(),
                expected: current.status.to_string(),
                actual: load_required_looper_run(&conn, looper_run_id.as_ref())?
                    .status
                    .to_string(),
            });
        }
        load_required_looper_run(&conn, looper_run_id.as_ref())
    }
}

impl LooperSubtaskRepository {
    /// Create the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert one looper-subtask execution view row for the given run.
    pub fn create_for_run(
        &self,
        looper_run: &LooperRunRecord,
        subtask: &SubtaskRecord,
    ) -> Result<LooperSubtaskRecord, LooperStoreError> {
        if subtask.task_id != looper_run.task_id {
            return Err(LooperStoreError::SubtaskOutsideRunTask {
                looper_run_id: looper_run.looper_run_id.to_string(),
                task_id: looper_run.task_id.to_string(),
                subtask_id: subtask.subtask_id.to_string(),
            });
        }

        let conn = lock_conn(&self.conn)?;
        let record = LooperSubtaskRecord {
            looper_subtask_id: LooperSubtaskId::new(Uuid::new_v4().to_string()),
            looper_run_id: looper_run.looper_run_id.clone(),
            subtask_id: subtask.subtask_id.clone(),
            status: initial_looper_subtask_status(subtask.status),
            dispatch_id: None,
            result: subtask.result.clone(),
            error: None,
            updated_at: subtask.updated_at.clone(),
        };
        insert_looper_subtask(&conn, &record)?;
        Ok(record)
    }

    /// Load all looper-subtask execution view rows for one looper run.
    pub fn find_by_looper_run(
        &self,
        looper_run_id: &LooperRunId,
    ) -> Result<Vec<LooperSubtaskRecord>, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn.prepare(
            "SELECT
                looper_subtask_id,
                looper_run_id,
                subtask_id,
                status,
                dispatch_id,
                result_json,
                error_json,
                updated_at
             FROM looper_subtask
             WHERE looper_run_id = ?1
             ORDER BY updated_at ASC, looper_subtask_id ASC",
        )?;
        let rows = stmt.query_map([looper_run_id.as_ref()], read_looper_subtask_sqlite_row)?;
        let mut records = Vec::new();
        for row in rows {
            records.push(decode_looper_subtask_row(row?)?);
        }
        Ok(records)
    }

    /// Update the execution-view status and optional result/error payloads.
    pub fn update_status(
        &self,
        looper_subtask_id: &LooperSubtaskId,
        status: LooperSubtaskStatus,
        result: Option<&JsonValue>,
        error: Option<&JsonValue>,
        updated_at: &str,
    ) -> Result<LooperSubtaskRecord, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute(
            "UPDATE looper_subtask
             SET status = ?1,
                 result_json = ?2,
                 error_json = ?3,
                 updated_at = ?4
             WHERE looper_subtask_id = ?5",
            params![
                status.as_str(),
                serialize_optional_json(result)?,
                serialize_optional_json(error)?,
                updated_at,
                looper_subtask_id.as_ref(),
            ],
        )?;
        ensure_looper_subtask_updated(rows, looper_subtask_id)?;
        load_required_looper_subtask(&conn, looper_subtask_id.as_ref())
    }

    /// Set or clear the durable dispatch link for one looper-subtask row.
    pub fn set_dispatch(
        &self,
        looper_subtask_id: &LooperSubtaskId,
        dispatch_id: Option<&str>,
        updated_at: &str,
    ) -> Result<LooperSubtaskRecord, LooperStoreError> {
        let conn = lock_conn(&self.conn)?;
        let rows = conn.execute(
            "UPDATE looper_subtask
             SET dispatch_id = ?1,
                 updated_at = ?2
             WHERE looper_subtask_id = ?3",
            params![dispatch_id, updated_at, looper_subtask_id.as_ref()],
        )?;
        ensure_looper_subtask_updated(rows, looper_subtask_id)?;
        load_required_looper_subtask(&conn, looper_subtask_id.as_ref())
    }
}

fn lock_conn(
    conn: &Arc<Mutex<Connection>>,
) -> Result<MutexGuard<'_, Connection>, LooperStoreError> {
    conn.lock()
        .map_err(|error| LooperStoreError::ConnectionLock(error.to_string()))
}

fn ensure_task_exists(conn: &Connection, task_id: &str) -> Result<(), LooperStoreError> {
    let exists = conn
        .query_row("SELECT 1 FROM task WHERE task_id = ?1", [task_id], |row| {
            row.get::<_, i64>(0)
        })
        .optional()?;
    if exists.is_some() {
        Ok(())
    } else {
        Err(LooperStoreError::TaskNotFound {
            task_id: task_id.to_string(),
        })
    }
}

fn validate_new_run(input: &NewLooperRun) -> Result<LooperRunRecord, LooperStoreError> {
    let execution_policy = parse_execution_policy_json(&input.execution_policy_json)?;
    let progress = parse_progress_json(&input.progress_json)?;
    Ok(LooperRunRecord {
        looper_run_id: input.looper_run_id.clone(),
        task_id: input.task_id.clone(),
        source_run_id: input.source_run_id.clone(),
        status: input.status,
        execution_policy,
        current_subtask_id: input.current_subtask_id.clone(),
        progress,
        error: input.error_json.clone(),
        started_at: input.started_at.clone(),
        updated_at: input.updated_at.clone(),
        completed_at: input.completed_at.clone(),
    })
}

fn parse_execution_policy_json(
    execution_policy_json: &JsonValue,
) -> Result<LooperExecutionPolicy, LooperStoreError> {
    let object =
        execution_policy_json
            .as_object()
            .ok_or_else(|| LooperStoreError::InvalidJsonField {
                field: "execution_policy_json",
                message: "must be a JSON object".to_string(),
            })?;

    let mode_text = object
        .get("mode")
        .ok_or(LooperStoreError::MissingExecutionPolicyField { field: "mode" })?
        .as_str()
        .ok_or_else(|| LooperStoreError::InvalidJsonField {
            field: "execution_policy_json.mode",
            message: "must be a string".to_string(),
        })?;
    let mode = LooperExecutionMode::from_str(mode_text).map_err(|_| {
        LooperStoreError::InvalidExecutionMode {
            mode: mode_text.to_string(),
        }
    })?;

    let max_parallelism_value =
        object
            .get("max_parallelism")
            .ok_or(LooperStoreError::MissingExecutionPolicyField {
                field: "max_parallelism",
            })?;
    let max_parallelism =
        max_parallelism_value
            .as_i64()
            .ok_or_else(|| LooperStoreError::InvalidJsonField {
                field: "execution_policy_json.max_parallelism",
                message: "must be an integer".to_string(),
            })?;
    if max_parallelism < 1 {
        return Err(LooperStoreError::InvalidMaxParallelism {
            value: max_parallelism,
        });
    }

    let selection_text = object
        .get("selection")
        .ok_or(LooperStoreError::MissingExecutionPolicyField { field: "selection" })?
        .as_str()
        .ok_or_else(|| LooperStoreError::InvalidJsonField {
            field: "execution_policy_json.selection",
            message: "must be a string".to_string(),
        })?;
    let selection = LooperSelectionStrategy::from_str(selection_text).map_err(|_| {
        LooperStoreError::InvalidSelectionStrategy {
            selection: selection_text.to_string(),
        }
    })?;

    Ok(LooperExecutionPolicy {
        mode,
        max_parallelism: u32::try_from(max_parallelism).map_err(|_| {
            LooperStoreError::InvalidJsonField {
                field: "execution_policy_json.max_parallelism",
                message: "must fit in u32".to_string(),
            }
        })?,
        selection,
    })
}

fn parse_progress_json(progress_json: &JsonValue) -> Result<LooperProgress, LooperStoreError> {
    let object = progress_json
        .as_object()
        .ok_or_else(|| LooperStoreError::InvalidJsonField {
            field: "progress_json",
            message: "must be a JSON object".to_string(),
        })?;

    Ok(LooperProgress {
        total: parse_progress_count(object, "total")?,
        completed: parse_progress_count(object, "completed")?,
        failed: parse_progress_count(object, "failed")?,
    })
}

fn parse_progress_count(
    object: &serde_json::Map<String, JsonValue>,
    field: &'static str,
) -> Result<usize, LooperStoreError> {
    let value = object
        .get(field)
        .ok_or(LooperStoreError::MissingExecutionPolicyField { field })?
        .as_u64()
        .ok_or_else(|| LooperStoreError::InvalidJsonField {
            field,
            message: "must be an unsigned integer".to_string(),
        })?;
    usize::try_from(value).map_err(|_| LooperStoreError::InvalidJsonField {
        field,
        message: "value does not fit in usize".to_string(),
    })
}

fn insert_looper_run(conn: &Connection, record: &LooperRunRecord) -> Result<(), LooperStoreError> {
    match conn.execute(
        "INSERT INTO looper_run (
            looper_run_id,
            task_id,
            source_run_id,
            status,
            execution_policy_json,
            current_subtask_id,
            progress_json,
            error_json,
            started_at,
            updated_at,
            completed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            record.looper_run_id.as_ref(),
            record.task_id.as_ref(),
            record.source_run_id.as_deref(),
            record.status.as_str(),
            serialize_json_field("execution_policy_json", &record.execution_policy)?,
            record.current_subtask_id.as_ref().map(AsRef::as_ref),
            serialize_json_field("progress_json", &record.progress)?,
            serialize_optional_json(record.error.as_ref())?,
            record.started_at.as_str(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
        ],
    ) {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation)
            ) =>
        {
            Err(LooperStoreError::RunAlreadyExists {
                looper_run_id: record.looper_run_id.to_string(),
            })
        }
        Err(error) => Err(error.into()),
    }
}

fn load_looper_run(
    conn: &Connection,
    looper_run_id: &str,
) -> Result<Option<LooperRunRecord>, LooperStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            looper_run_id,
            task_id,
            source_run_id,
            status,
            execution_policy_json,
            current_subtask_id,
            progress_json,
            error_json,
            started_at,
            updated_at,
            completed_at
         FROM looper_run
         WHERE looper_run_id = ?1",
    )?;
    let row = stmt
        .query_row([looper_run_id], read_looper_run_sqlite_row)
        .optional()?;
    row.map(decode_looper_run_row).transpose()
}

fn load_required_looper_run(
    conn: &Connection,
    looper_run_id: &str,
) -> Result<LooperRunRecord, LooperStoreError> {
    load_looper_run(conn, looper_run_id)?.ok_or_else(|| LooperStoreError::RunNotFound {
        looper_run_id: looper_run_id.to_string(),
    })
}

fn list_looper_runs(
    conn: &Connection,
    query: &LooperRunListQuery,
) -> Result<Vec<LooperRunRecord>, LooperStoreError> {
    let mut sql = String::from(
        "SELECT
            looper_run_id,
            task_id,
            source_run_id,
            status,
            execution_policy_json,
            current_subtask_id,
            progress_json,
            error_json,
            started_at,
            updated_at,
            completed_at
         FROM looper_run",
    );
    let mut predicates = Vec::new();
    let mut params = Vec::new();

    if let Some(task_id) = query.task_id.as_ref() {
        predicates.push("task_id = ?".to_string());
        params.push(SqlValue::from(task_id.to_string()));
    }

    if let Some(source_run_id) = query.source_run_id.as_deref() {
        predicates.push("source_run_id = ?".to_string());
        params.push(SqlValue::from(source_run_id.to_string()));
    }

    if let Some(status) = query.status {
        predicates.push("status = ?".to_string());
        params.push(SqlValue::from(status.to_string()));
    }

    if let Some(execution_mode) = query.execution_mode {
        predicates.push("json_extract(execution_policy_json, '$.mode') = ?".to_string());
        params.push(SqlValue::from(execution_mode.to_string()));
    }

    if !predicates.is_empty() {
        sql.push_str(" WHERE ");
        sql.push_str(&predicates.join(" AND "));
    }

    sql.push_str(" ORDER BY updated_at DESC, looper_run_id DESC");

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), read_looper_run_sqlite_row)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(decode_looper_run_row(row?)?);
    }
    Ok(records)
}

fn ensure_run_updated(rows: usize, looper_run_id: &LooperRunId) -> Result<(), LooperStoreError> {
    if rows == 0 {
        Err(LooperStoreError::RunNotFound {
            looper_run_id: looper_run_id.to_string(),
        })
    } else {
        Ok(())
    }
}

fn insert_looper_subtask(
    conn: &Connection,
    record: &LooperSubtaskRecord,
) -> Result<(), LooperStoreError> {
    match conn.execute(
        "INSERT INTO looper_subtask (
            looper_subtask_id,
            looper_run_id,
            subtask_id,
            status,
            dispatch_id,
            result_json,
            error_json,
            updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            record.looper_subtask_id.as_ref(),
            record.looper_run_id.as_ref(),
            record.subtask_id.as_ref(),
            record.status.as_str(),
            record.dispatch_id.as_deref(),
            serialize_optional_json(record.result.as_ref())?,
            serialize_optional_json(record.error.as_ref())?,
            record.updated_at.as_str(),
        ],
    ) {
        Ok(_) => Ok(()),
        Err(error)
            if matches!(
                error.sqlite_error_code(),
                Some(rusqlite::ErrorCode::ConstraintViolation)
            ) =>
        {
            Err(LooperStoreError::LooperSubtaskAlreadyExists {
                looper_run_id: record.looper_run_id.to_string(),
                subtask_id: record.subtask_id.to_string(),
            })
        }
        Err(error) => Err(error.into()),
    }
}

fn load_looper_subtask(
    conn: &Connection,
    looper_subtask_id: &str,
) -> Result<Option<LooperSubtaskRecord>, LooperStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            looper_subtask_id,
            looper_run_id,
            subtask_id,
            status,
            dispatch_id,
            result_json,
            error_json,
            updated_at
         FROM looper_subtask
         WHERE looper_subtask_id = ?1",
    )?;
    let row = stmt
        .query_row([looper_subtask_id], read_looper_subtask_sqlite_row)
        .optional()?;
    row.map(decode_looper_subtask_row).transpose()
}

fn load_required_looper_subtask(
    conn: &Connection,
    looper_subtask_id: &str,
) -> Result<LooperSubtaskRecord, LooperStoreError> {
    load_looper_subtask(conn, looper_subtask_id)?.ok_or_else(|| {
        LooperStoreError::LooperSubtaskNotFound {
            looper_subtask_id: looper_subtask_id.to_string(),
        }
    })
}

fn ensure_looper_subtask_updated(
    rows: usize,
    looper_subtask_id: &LooperSubtaskId,
) -> Result<(), LooperStoreError> {
    if rows == 0 {
        Err(LooperStoreError::LooperSubtaskNotFound {
            looper_subtask_id: looper_subtask_id.to_string(),
        })
    } else {
        Ok(())
    }
}

fn serialize_json_field(
    field: &'static str,
    value: &impl serde::Serialize,
) -> Result<String, LooperStoreError> {
    serde_json::to_string(value).map_err(|error| LooperStoreError::InvalidJsonField {
        field,
        message: error.to_string(),
    })
}

fn serialize_optional_json(value: Option<&JsonValue>) -> Result<Option<String>, LooperStoreError> {
    value
        .map(|json| serde_json::to_string(json).map_err(LooperStoreError::from))
        .transpose()
}

fn read_looper_run_sqlite_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LooperRunRow> {
    Ok(LooperRunRow {
        looper_run_id: row.get(0)?,
        task_id: row.get(1)?,
        source_run_id: row.get(2)?,
        status: row.get(3)?,
        execution_policy_json: row.get(4)?,
        current_subtask_id: row.get(5)?,
        progress_json: row.get(6)?,
        error_json: row.get(7)?,
        started_at: row.get(8)?,
        updated_at: row.get(9)?,
        completed_at: row.get(10)?,
    })
}

fn read_looper_subtask_sqlite_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LooperSubtaskRow> {
    Ok(LooperSubtaskRow {
        looper_subtask_id: row.get(0)?,
        looper_run_id: row.get(1)?,
        subtask_id: row.get(2)?,
        status: row.get(3)?,
        dispatch_id: row.get(4)?,
        result_json: row.get(5)?,
        error_json: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn decode_looper_run_row(row: LooperRunRow) -> Result<LooperRunRecord, LooperStoreError> {
    Ok(LooperRunRecord {
        looper_run_id: LooperRunId::new(row.looper_run_id),
        task_id: TaskId::new(row.task_id),
        source_run_id: row.source_run_id,
        status: LooperRunStatus::from_str(&row.status).map_err(|_| {
            LooperStoreError::InvalidRunStatus {
                status: row.status.clone(),
            }
        })?,
        execution_policy: parse_execution_policy_json(
            &serde_json::from_str(&row.execution_policy_json).map_err(|error| {
                LooperStoreError::InvalidJsonField {
                    field: "execution_policy_json",
                    message: error.to_string(),
                }
            })?,
        )?,
        current_subtask_id: row.current_subtask_id.map(SubtaskId::new),
        progress: parse_progress_json(&serde_json::from_str(&row.progress_json).map_err(
            |error| LooperStoreError::InvalidJsonField {
                field: "progress_json",
                message: error.to_string(),
            },
        )?)?,
        error: row
            .error_json
            .map(|value| {
                serde_json::from_str(&value).map_err(|error| LooperStoreError::InvalidJsonField {
                    field: "error_json",
                    message: error.to_string(),
                })
            })
            .transpose()?,
        started_at: row.started_at,
        updated_at: row.updated_at,
        completed_at: row.completed_at,
    })
}

fn decode_looper_subtask_row(
    row: LooperSubtaskRow,
) -> Result<LooperSubtaskRecord, LooperStoreError> {
    Ok(LooperSubtaskRecord {
        looper_subtask_id: LooperSubtaskId::new(row.looper_subtask_id),
        looper_run_id: LooperRunId::new(row.looper_run_id),
        subtask_id: SubtaskId::new(row.subtask_id),
        status: LooperSubtaskStatus::from_str(&row.status).map_err(|_| {
            LooperStoreError::InvalidSubtaskStatus {
                status: row.status.clone(),
            }
        })?,
        dispatch_id: row.dispatch_id,
        result: row
            .result_json
            .map(|value| {
                serde_json::from_str(&value).map_err(|error| LooperStoreError::InvalidJsonField {
                    field: "result_json",
                    message: error.to_string(),
                })
            })
            .transpose()?,
        error: row
            .error_json
            .map(|value| {
                serde_json::from_str(&value).map_err(|error| LooperStoreError::InvalidJsonField {
                    field: "error_json",
                    message: error.to_string(),
                })
            })
            .transpose()?,
        updated_at: row.updated_at,
    })
}

fn initial_looper_subtask_status(status: SubtaskStatus) -> LooperSubtaskStatus {
    match status {
        SubtaskStatus::Completed => LooperSubtaskStatus::Completed,
        SubtaskStatus::Failed => LooperSubtaskStatus::Failed,
        SubtaskStatus::Cancelled => LooperSubtaskStatus::Cancelled,
        SubtaskStatus::Planned | SubtaskStatus::Ready | SubtaskStatus::InProgress => {
            LooperSubtaskStatus::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openfang_types::task::{
        ActorKind, AssigneeRef, Complexity, OwnerRef, Priority, SubtaskKind, TaskSource, TaskStatus,
    };
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    use crate::task::{SubtaskRepository, TaskRepository, TASK_SUBTASK_MIGRATION_SQL};

    fn configure_test_connection(conn: &Connection) {
        conn.execute_batch(
            "
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            ",
        )
        .expect("configure sqlite pragmas");
        conn.busy_timeout(Duration::from_millis(5_000))
            .expect("set busy timeout");
    }

    fn migrated_in_memory_connection() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open in-memory compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(TASK_SUBTASK_MIGRATION_SQL)
            .expect("apply task/subtask migration");
        conn.execute_batch(LOOPER_RUNTIME_MIGRATION_SQL)
            .expect("apply looper migration");
        Arc::new(Mutex::new(conn))
    }

    fn migrated_file_connection(path: &Path) -> Arc<Mutex<Connection>> {
        let conn = Connection::open(path).expect("open file-backed compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(TASK_SUBTASK_MIGRATION_SQL)
            .expect("apply task/subtask migration");
        conn.execute_batch(LOOPER_RUNTIME_MIGRATION_SQL)
            .expect("apply looper migration");
        Arc::new(Mutex::new(conn))
    }

    fn sample_task(task_id: &str, slug: &str) -> openfang_types::task::TaskRecord {
        openfang_types::task::TaskRecord {
            task_id: TaskId::new(task_id),
            slug: slug.to_string(),
            source: TaskSource::Workflow {
                workflow_id: "sdlc".to_string(),
                run_id: "run_123".to_string(),
            },
            title: "Prepare PRD".to_string(),
            description: "Write the PRD".to_string(),
            status: TaskStatus::Planned,
            priority: Priority::High,
            complexity: Complexity::Medium,
            position: 1,
            owner: OwnerRef {
                kind: ActorKind::AgentGroup,
                ref_id: "sdlc".to_string(),
            },
            created_by: OwnerRef {
                kind: ActorKind::Agent,
                ref_id: "planner".to_string(),
            },
            repository_refs: vec![],
            label_refs: vec![],
            artifact_refs: vec![],
            doc_refs: vec![],
            file_refs: vec![],
            metadata: serde_json::json!({}),
            created_at: "2026-03-25T10:00:00Z".to_string(),
            updated_at: "2026-03-25T10:00:00Z".to_string(),
            completed_at: None,
        }
    }

    fn sample_subtask(subtask_id: &str, task_id: &TaskId, position: i64) -> SubtaskRecord {
        SubtaskRecord {
            subtask_id: SubtaskId::new(subtask_id),
            task_id: task_id.clone(),
            title: format!("Subtask {subtask_id}"),
            description: "Write the next section".to_string(),
            kind: SubtaskKind::DocChange,
            status: SubtaskStatus::Ready,
            complexity: Complexity::Medium,
            position,
            assignee: Some(AssigneeRef {
                kind: ActorKind::Agent,
                ref_id: "prd-writer".to_string(),
            }),
            depends_on: Vec::new(),
            parallelizable: false,
            input: serde_json::json!({}),
            result: None,
            metadata: serde_json::json!({}),
            created_at: "2026-03-25T10:01:00Z".to_string(),
            updated_at: "2026-03-25T10:01:00Z".to_string(),
            completed_at: None,
        }
    }

    fn seed_task_graph(
        conn: Arc<Mutex<Connection>>,
    ) -> (
        TaskRepository,
        SubtaskRepository,
        LooperRunRepository,
        LooperSubtaskRepository,
    ) {
        (
            TaskRepository::new(Arc::clone(&conn)),
            SubtaskRepository::new(Arc::clone(&conn)),
            LooperRunRepository::new(Arc::clone(&conn)),
            LooperSubtaskRepository::new(conn),
        )
    }

    fn sample_new_looper_run(task_id: &TaskId, mode: &str, max_parallelism: i64) -> NewLooperRun {
        NewLooperRun {
            looper_run_id: LooperRunId::new(Uuid::new_v4().to_string()),
            task_id: task_id.clone(),
            source_run_id: Some("run_123".to_string()),
            status: LooperRunStatus::Pending,
            execution_policy_json: serde_json::json!({
                "mode": mode,
                "max_parallelism": max_parallelism,
                "selection": "priority",
            }),
            current_subtask_id: None,
            progress_json: serde_json::json!({
                "total": 3,
                "completed": 0,
                "failed": 0,
            }),
            error_json: None,
            started_at: "2026-03-25T10:00:00Z".to_string(),
            updated_at: "2026-03-25T10:00:00Z".to_string(),
            completed_at: None,
        }
    }

    #[test]
    fn looper_run_repository_should_round_trip_explicit_policy_json() {
        let conn = migrated_in_memory_connection();
        let (task_repository, _, looper_run_repository, _) = seed_task_graph(conn);
        let task = sample_task("task_001", "task-001");
        task_repository.create(&task).expect("create task");

        let created = looper_run_repository
            .create(&sample_new_looper_run(&task.task_id, "sequential", 1))
            .expect("create looper run");
        let loaded = looper_run_repository
            .find_by_id(&created.looper_run_id)
            .expect("find by id")
            .expect("looper run exists");

        assert_eq!(loaded.execution_policy, created.execution_policy);
        assert_eq!(loaded, created);
    }

    #[test]
    fn looper_run_repository_should_reject_missing_mode_field() {
        let conn = migrated_in_memory_connection();
        let (task_repository, _, looper_run_repository, _) = seed_task_graph(conn);
        let task = sample_task("task_002", "task-002");
        task_repository.create(&task).expect("create task");

        let mut input = sample_new_looper_run(&task.task_id, "sequential", 1);
        input.execution_policy_json = serde_json::json!({
            "max_parallelism": 1,
            "selection": "priority",
        });

        let error = looper_run_repository
            .create(&input)
            .expect_err("missing mode should fail");

        assert_eq!(
            error.to_string(),
            LooperStoreError::MissingExecutionPolicyField { field: "mode" }.to_string()
        );
    }

    #[test]
    fn looper_run_repository_should_reject_zero_max_parallelism() {
        let conn = migrated_in_memory_connection();
        let (task_repository, _, looper_run_repository, _) = seed_task_graph(conn);
        let task = sample_task("task_003", "task-003");
        task_repository.create(&task).expect("create task");

        let error = looper_run_repository
            .create(&sample_new_looper_run(&task.task_id, "parallel", 0))
            .expect_err("zero max_parallelism should fail");

        assert_eq!(
            error.to_string(),
            LooperStoreError::InvalidMaxParallelism { value: 0 }.to_string()
        );
    }

    #[test]
    fn looper_subtask_repository_should_create_execution_view_for_run() {
        let conn = migrated_in_memory_connection();
        let (task_repository, subtask_repository, looper_run_repository, looper_subtask_repository) =
            seed_task_graph(conn);
        let task = sample_task("task_004", "task-004");
        task_repository.create(&task).expect("create task");
        let subtask = sample_subtask("subtask_001", &task.task_id, 1);
        subtask_repository.create(&subtask).expect("create subtask");
        let looper_run = looper_run_repository
            .create(&sample_new_looper_run(&task.task_id, "parallel", 2))
            .expect("create looper run");

        let created = looper_subtask_repository
            .create_for_run(&looper_run, &subtask)
            .expect("create looper subtask");
        let loaded = looper_subtask_repository
            .find_by_looper_run(&looper_run.looper_run_id)
            .expect("list looper subtasks");

        assert_eq!(loaded, vec![created.clone()]);
        assert_eq!(created.status, LooperSubtaskStatus::Pending);
    }

    #[test]
    fn looper_run_repository_should_filter_by_execution_mode() {
        let conn = migrated_in_memory_connection();
        let (task_repository, _, looper_run_repository, _) = seed_task_graph(conn);
        let task = sample_task("task_005", "task-005");
        task_repository.create(&task).expect("create task");

        let sequential = looper_run_repository
            .create(&sample_new_looper_run(&task.task_id, "sequential", 1))
            .expect("create sequential looper run");
        let parallel = looper_run_repository
            .create(&sample_new_looper_run(&task.task_id, "parallel", 3))
            .expect("create parallel looper run");

        let items = looper_run_repository
            .list(&LooperRunListQuery {
                execution_mode: Some(LooperExecutionMode::Parallel),
                ..LooperRunListQuery::default()
            })
            .expect("list looper runs");

        assert_eq!(items, vec![parallel]);
        assert_ne!(items[0].looper_run_id, sequential.looper_run_id);
    }

    #[test]
    fn looper_rows_should_round_trip_after_reopen() {
        let dir = tempdir().expect("create temp dir");
        let db_path = dir.path().join("compozy.db");

        {
            let conn = migrated_file_connection(&db_path);
            let (
                task_repository,
                subtask_repository,
                looper_run_repository,
                looper_subtask_repository,
            ) = seed_task_graph(conn);
            let task = sample_task("task_file", "task-file");
            task_repository.create(&task).expect("create task");
            let subtask = sample_subtask("subtask_file", &task.task_id, 1);
            subtask_repository.create(&subtask).expect("create subtask");
            let looper_run = looper_run_repository
                .create(&sample_new_looper_run(&task.task_id, "parallel", 2))
                .expect("create looper run");
            let created = looper_subtask_repository
                .create_for_run(&looper_run, &subtask)
                .expect("create looper subtask");
            looper_subtask_repository
                .set_dispatch(
                    &created.looper_subtask_id,
                    Some("dispatch_001"),
                    "2026-03-25T10:05:00Z",
                )
                .expect("set dispatch");
        }

        let conn = migrated_file_connection(&db_path);
        let (_, _, looper_run_repository, looper_subtask_repository) = seed_task_graph(conn);
        let run = looper_run_repository
            .list(&LooperRunListQuery::default())
            .expect("list runs")
            .pop()
            .expect("looper run exists");
        let subtasks = looper_subtask_repository
            .find_by_looper_run(&run.looper_run_id)
            .expect("list looper subtasks");

        assert_eq!(subtasks.len(), 1);
        assert_eq!(subtasks[0].dispatch_id.as_deref(), Some("dispatch_001"));
    }
}
