//! Typed `compozy.db` repository for durable agent dispatch state.

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

/// SQL for migration `0008_agent_dispatch`.
pub const AGENT_DISPATCH_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260324_008_agent_dispatch.sql");

/// Durable dispatch modes for agent delegation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchKind {
    /// `call` blocks the workflow step until a result is available.
    Call,
    /// `send` fires work without waiting for a result.
    Send,
    /// `spawn` creates or binds to a long-lived agent identity.
    Spawn,
}

impl DispatchKind {
    /// Returns the canonical SQLite / JSON string encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Send => "send",
            Self::Spawn => "spawn",
        }
    }

    fn parse_db_text(value: &str) -> Result<Self, DispatchStoreError> {
        match value {
            "call" => Ok(Self::Call),
            "send" => Ok(Self::Send),
            "spawn" => Ok(Self::Spawn),
            other => Err(DispatchStoreError::InvalidDispatchKind {
                kind: other.to_string(),
            }),
        }
    }
}

impl std::fmt::Display for DispatchKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DispatchKind {
    type Err = DispatchStoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value)
    }
}

/// Durable lifecycle states for `agent_dispatch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    /// The dispatch row exists but execution has not started yet.
    Pending,
    /// The dispatch is actively executing.
    Running,
    /// The dispatch is paused on a human-in-the-loop interaction.
    WaitingHitl,
    /// The dispatch completed successfully.
    Completed,
    /// The dispatch failed.
    Failed,
    /// The dispatch was cancelled.
    Cancelled,
}

impl DispatchStatus {
    /// Returns the canonical SQLite / JSON string encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::WaitingHitl => "waiting_hitl",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether the status is terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    fn parse_db_text(value: &str) -> Result<Self, DispatchStoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "waiting_hitl" => Ok(Self::WaitingHitl),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => Err(DispatchStoreError::InvalidDispatchStatus {
                status: other.to_string(),
            }),
        }
    }
}

impl std::fmt::Display for DispatchStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for DispatchStatus {
    type Err = DispatchStoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value)
    }
}

/// Full durable dispatch row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DispatchRecord {
    /// Stable dispatch identifier.
    pub dispatch_id: String,
    /// Owning workflow run identifier.
    pub run_id: String,
    /// Step identifier that produced the dispatch, if any.
    pub step_id: Option<String>,
    /// Dispatch mode.
    pub kind: DispatchKind,
    /// Target agent definition identifier.
    pub target_agent: String,
    /// Current durable lifecycle state.
    pub status: DispatchStatus,
    /// Captured dispatch input payload.
    pub input_json: JsonValue,
    /// Result payload, when available.
    pub result_json: Option<JsonValue>,
    /// Error payload, when available.
    pub error_json: Option<JsonValue>,
    /// Retry / re-execution attempt counter.
    pub attempt: u32,
    /// Parent dispatch identifier for lineage reconstruction.
    pub parent_dispatch_id: Option<String>,
    /// Stable spawned agent identifier for `spawn` dispatches.
    pub spawned_agent_id: Option<String>,
    /// Provider driver name reserved for runtime session resume.
    pub provider_driver: Option<String>,
    /// Provider / session identifier reserved for runtime session resume.
    pub session_id: Option<String>,
    /// Provider-specific opaque resume token.
    pub provider_resume_token: Option<String>,
    /// Dispatch start timestamp (RFC 3339).
    pub started_at: String,
    /// Last update timestamp (RFC 3339).
    pub updated_at: String,
    /// Completion timestamp for terminal states.
    pub completed_at: Option<String>,
}

/// Summary view used by run-scoped list endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchSummaryRecord {
    /// Stable dispatch identifier.
    pub dispatch_id: String,
    /// Owning workflow run identifier.
    pub run_id: String,
    /// Step identifier that produced the dispatch, if any.
    pub step_id: Option<String>,
    /// Dispatch mode.
    pub kind: DispatchKind,
    /// Target agent definition identifier.
    pub target_agent: String,
    /// Current durable lifecycle state.
    pub status: DispatchStatus,
    /// Last update timestamp (RFC 3339).
    pub updated_at: String,
}

impl From<&DispatchRecord> for DispatchSummaryRecord {
    fn from(record: &DispatchRecord) -> Self {
        Self {
            dispatch_id: record.dispatch_id.clone(),
            run_id: record.run_id.clone(),
            step_id: record.step_id.clone(),
            kind: record.kind,
            target_agent: record.target_agent.clone(),
            status: record.status,
            updated_at: record.updated_at.clone(),
        }
    }
}

/// Typed failures from the durable dispatch repository.
#[derive(Debug, Error)]
pub enum DispatchStoreError {
    /// Failed to acquire the shared `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested dispatch record does not exist.
    #[error("dispatch '{dispatch_id}' was not found")]
    DispatchNotFound {
        /// Missing dispatch identifier.
        dispatch_id: String,
    },
    /// A stored dispatch kind string was invalid.
    #[error("invalid dispatch kind '{kind}'")]
    InvalidDispatchKind {
        /// Stored invalid text value.
        kind: String,
    },
    /// A stored dispatch status string was invalid.
    #[error("invalid dispatch status '{status}'")]
    InvalidDispatchStatus {
        /// Stored invalid text value.
        status: String,
    },
    /// A status transition violated the repository state machine.
    #[error("dispatch '{dispatch_id}' transition {from} -> {to} is not allowed")]
    InvalidStatusTransition {
        /// Dispatch being updated.
        dispatch_id: String,
        /// Current status.
        from: DispatchStatus,
        /// Requested next status.
        to: DispatchStatus,
    },
    /// A concurrent update changed the current state before this write.
    #[error("dispatch '{dispatch_id}' expected current state '{expected}', got '{actual}'")]
    UnexpectedDispatchState {
        /// Dispatch being updated.
        dispatch_id: String,
        /// Expected current status.
        expected: DispatchStatus,
        /// Actual current status observed in SQLite.
        actual: DispatchStatus,
    },
    /// A mutable update attempted to rewrite an immutable field.
    #[error("dispatch '{dispatch_id}' immutable field '{field}' cannot change")]
    ImmutableFieldChanged {
        /// Dispatch being updated.
        dispatch_id: String,
        /// Field that changed unexpectedly.
        field: &'static str,
    },
    /// A retry path changed the attempt counter incorrectly.
    #[error(
        "dispatch '{dispatch_id}' retry attempt must increment by exactly one (expected {expected}, got {actual})"
    )]
    InvalidAttemptIncrement {
        /// Dispatch being updated.
        dispatch_id: String,
        /// Expected attempt counter.
        expected: u32,
        /// Actual attempt counter.
        actual: u32,
    },
    /// A stored integer could not be represented as `u32`.
    #[error("dispatch attempt value {value} is outside the supported u32 range")]
    InvalidAttemptValue {
        /// Invalid SQLite integer.
        value: i64,
    },
    /// Terminal timestamps were inconsistent with the next status.
    #[error("dispatch '{dispatch_id}' terminal timestamp is inconsistent with status '{status}'")]
    InvalidCompletionState {
        /// Dispatch being updated.
        dispatch_id: String,
        /// Status being written.
        status: DispatchStatus,
    },
    /// A payload update targeted a dispatch outside active execution states.
    #[error("dispatch '{dispatch_id}' cannot update '{field}' while in status '{status}'")]
    InvalidPayloadUpdateState {
        /// Dispatch being updated.
        dispatch_id: String,
        /// Payload field being written.
        field: &'static str,
        /// Current durable status.
        status: DispatchStatus,
    },
}

/// Async storage contract for durable dispatch persistence.
#[async_trait]
pub trait DispatchRepository: Send + Sync {
    /// Inserts the initial durable dispatch row.
    async fn create(&self, record: &DispatchRecord) -> Result<(), DispatchStoreError>;

    /// Loads one dispatch by its stable identifier.
    async fn find_by_id(
        &self,
        dispatch_id: &str,
    ) -> Result<Option<DispatchRecord>, DispatchStoreError>;

    /// Lists all dispatches for one workflow run.
    async fn find_by_run(&self, run_id: &str) -> Result<Vec<DispatchRecord>, DispatchStoreError>;

    /// Lists all child dispatches for one parent dispatch.
    async fn find_children(
        &self,
        parent_dispatch_id: &str,
    ) -> Result<Vec<DispatchRecord>, DispatchStoreError>;

    /// Returns run-scoped summary rows for operational list surfaces.
    async fn list_summaries_by_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<DispatchSummaryRecord>, DispatchStoreError>;

    /// Applies a validated status transition and any associated mutable fields.
    async fn update_status(
        &self,
        current: &DispatchRecord,
        next: &DispatchRecord,
    ) -> Result<DispatchRecord, DispatchStoreError>;

    /// Updates only the result payload for a dispatch.
    async fn update_result(
        &self,
        dispatch_id: &str,
        result_json: Option<&JsonValue>,
        updated_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError>;

    /// Updates only the error payload for a dispatch.
    async fn update_error(
        &self,
        dispatch_id: &str,
        error_json: Option<&JsonValue>,
        updated_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError>;

    /// Marks a dispatch completed and writes its final result atomically.
    async fn mark_completed(
        &self,
        current: &DispatchRecord,
        result_json: Option<&JsonValue>,
        completed_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError>;

    /// Marks a dispatch failed and writes its final error atomically.
    async fn mark_failed(
        &self,
        current: &DispatchRecord,
        error_json: &JsonValue,
        completed_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError>;

    /// Increments the attempt counter and resets the dispatch to `pending`.
    async fn increment_attempt(
        &self,
        current: &DispatchRecord,
        updated_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError>;
}

/// SQLite-backed dispatch repository.
#[derive(Clone)]
pub struct SqliteDispatchRepository {
    conn: Arc<Mutex<Connection>>,
}

/// Backward-compatible store alias.
pub type DispatchStore = SqliteDispatchRepository;

impl SqliteDispatchRepository {
    /// Creates the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Synchronous summary helper used by older run-scoped readers.
    pub fn list_summaries_by_run_blocking(
        &self,
        run_id: &str,
    ) -> Result<Vec<DispatchSummaryRecord>, DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_dispatch_summaries_by_run(&conn, run_id)
    }
}

#[async_trait]
impl DispatchRepository for SqliteDispatchRepository {
    async fn create(&self, record: &DispatchRecord) -> Result<(), DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        insert_dispatch(&conn, record)
    }

    async fn find_by_id(
        &self,
        dispatch_id: &str,
    ) -> Result<Option<DispatchRecord>, DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_dispatch(&conn, dispatch_id)
    }

    async fn find_by_run(&self, run_id: &str) -> Result<Vec<DispatchRecord>, DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_dispatches_by_run(&conn, run_id)
    }

    async fn find_children(
        &self,
        parent_dispatch_id: &str,
    ) -> Result<Vec<DispatchRecord>, DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_child_dispatches(&conn, parent_dispatch_id)
    }

    async fn list_summaries_by_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<DispatchSummaryRecord>, DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_dispatch_summaries_by_run(&conn, run_id)
    }

    async fn update_status(
        &self,
        current: &DispatchRecord,
        next: &DispatchRecord,
    ) -> Result<DispatchRecord, DispatchStoreError> {
        validate_status_transition(current, next)?;
        let conn = lock_conn(&self.conn)?;
        let rows = update_dispatch_row(&conn, current, next)?;
        if rows == 0 {
            return Err(resolve_update_conflict(
                &conn,
                &current.dispatch_id,
                current.status,
            ));
        }
        Ok(next.clone())
    }

    async fn update_result(
        &self,
        dispatch_id: &str,
        result_json: Option<&JsonValue>,
        updated_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        let current = load_required_dispatch(&conn, dispatch_id)?;
        validate_payload_update(&current, "result_json")?;
        let rows = conn.execute(
            "UPDATE agent_dispatch
             SET result_json = ?1,
                 updated_at = ?2
             WHERE dispatch_id = ?3
               AND status = ?4",
            params![
                serialize_optional_json(result_json)?,
                updated_at,
                dispatch_id,
                current.status.as_str(),
            ],
        )?;
        if rows == 0 {
            return Err(resolve_update_conflict(&conn, dispatch_id, current.status));
        }
        load_required_dispatch(&conn, dispatch_id)
    }

    async fn update_error(
        &self,
        dispatch_id: &str,
        error_json: Option<&JsonValue>,
        updated_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError> {
        let conn = lock_conn(&self.conn)?;
        let current = load_required_dispatch(&conn, dispatch_id)?;
        validate_payload_update(&current, "error_json")?;
        let rows = conn.execute(
            "UPDATE agent_dispatch
             SET error_json = ?1,
                 updated_at = ?2
             WHERE dispatch_id = ?3
               AND status = ?4",
            params![
                serialize_optional_json(error_json)?,
                updated_at,
                dispatch_id,
                current.status.as_str(),
            ],
        )?;
        if rows == 0 {
            return Err(resolve_update_conflict(&conn, dispatch_id, current.status));
        }
        load_required_dispatch(&conn, dispatch_id)
    }

    async fn mark_completed(
        &self,
        current: &DispatchRecord,
        result_json: Option<&JsonValue>,
        completed_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError> {
        let mut next = current.clone();
        next.status = DispatchStatus::Completed;
        next.result_json = result_json.cloned();
        next.error_json = None;
        next.updated_at = completed_at.to_string();
        next.completed_at = Some(completed_at.to_string());
        self.update_status(current, &next).await
    }

    async fn mark_failed(
        &self,
        current: &DispatchRecord,
        error_json: &JsonValue,
        completed_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError> {
        let mut next = current.clone();
        next.status = DispatchStatus::Failed;
        next.error_json = Some(error_json.clone());
        next.updated_at = completed_at.to_string();
        next.completed_at = Some(completed_at.to_string());
        self.update_status(current, &next).await
    }

    async fn increment_attempt(
        &self,
        current: &DispatchRecord,
        updated_at: &str,
    ) -> Result<DispatchRecord, DispatchStoreError> {
        let mut next = current.clone();
        next.status = DispatchStatus::Pending;
        next.attempt = current.attempt + 1;
        next.result_json = None;
        next.error_json = None;
        next.spawned_agent_id = None;
        next.provider_driver = None;
        next.session_id = None;
        next.provider_resume_token = None;
        next.updated_at = updated_at.to_string();
        next.completed_at = None;
        self.update_status(current, &next).await
    }
}

fn lock_conn(
    conn: &Arc<Mutex<Connection>>,
) -> Result<MutexGuard<'_, Connection>, DispatchStoreError> {
    conn.lock()
        .map_err(|error| DispatchStoreError::ConnectionLock(error.to_string()))
}

fn insert_dispatch(conn: &Connection, record: &DispatchRecord) -> Result<(), DispatchStoreError> {
    conn.execute(
        "INSERT INTO agent_dispatch (
            dispatch_id,
            run_id,
            step_id,
            kind,
            target_agent,
            status,
            input_json,
            result_json,
            error_json,
            attempt,
            parent_dispatch_id,
            spawned_agent_id,
            provider_driver,
            session_id,
            provider_resume_token,
            started_at,
            updated_at,
            completed_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
        params![
            record.dispatch_id.as_str(),
            record.run_id.as_str(),
            record.step_id.as_deref(),
            record.kind.as_str(),
            record.target_agent.as_str(),
            record.status.as_str(),
            serde_json::to_string(&record.input_json)?,
            serialize_optional_json(record.result_json.as_ref())?,
            serialize_optional_json(record.error_json.as_ref())?,
            i64::from(record.attempt),
            record.parent_dispatch_id.as_deref(),
            record.spawned_agent_id.as_deref(),
            record.provider_driver.as_deref(),
            record.session_id.as_deref(),
            record.provider_resume_token.as_deref(),
            record.started_at.as_str(),
            record.updated_at.as_str(),
            record.completed_at.as_deref(),
        ],
    )?;
    Ok(())
}

pub(crate) fn load_dispatch(
    conn: &Connection,
    dispatch_id: &str,
) -> Result<Option<DispatchRecord>, DispatchStoreError> {
    conn.query_row(
        "SELECT
            dispatch_id,
            run_id,
            step_id,
            kind,
            target_agent,
            status,
            input_json,
            result_json,
            error_json,
            attempt,
            parent_dispatch_id,
            spawned_agent_id,
            provider_driver,
            session_id,
            provider_resume_token,
            started_at,
            updated_at,
            completed_at
         FROM agent_dispatch
         WHERE dispatch_id = ?1",
        [dispatch_id],
        read_dispatch_row,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn load_required_dispatch(
    conn: &Connection,
    dispatch_id: &str,
) -> Result<DispatchRecord, DispatchStoreError> {
    load_dispatch(conn, dispatch_id)?.ok_or_else(|| DispatchStoreError::DispatchNotFound {
        dispatch_id: dispatch_id.to_string(),
    })
}

fn list_dispatches_by_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<DispatchRecord>, DispatchStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            dispatch_id,
            run_id,
            step_id,
            kind,
            target_agent,
            status,
            input_json,
            result_json,
            error_json,
            attempt,
            parent_dispatch_id,
            spawned_agent_id,
            provider_driver,
            session_id,
            provider_resume_token,
            started_at,
            updated_at,
            completed_at
         FROM agent_dispatch
         WHERE run_id = ?1
         ORDER BY updated_at DESC, dispatch_id DESC",
    )?;
    let rows = stmt.query_map([run_id], read_dispatch_row)?;
    collect_rows(rows)
}

fn list_child_dispatches(
    conn: &Connection,
    parent_dispatch_id: &str,
) -> Result<Vec<DispatchRecord>, DispatchStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            dispatch_id,
            run_id,
            step_id,
            kind,
            target_agent,
            status,
            input_json,
            result_json,
            error_json,
            attempt,
            parent_dispatch_id,
            spawned_agent_id,
            provider_driver,
            session_id,
            provider_resume_token,
            started_at,
            updated_at,
            completed_at
         FROM agent_dispatch
         WHERE parent_dispatch_id = ?1
         ORDER BY updated_at DESC, dispatch_id DESC",
    )?;
    let rows = stmt.query_map([parent_dispatch_id], read_dispatch_row)?;
    collect_rows(rows)
}

pub(crate) fn list_dispatch_summaries_by_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<DispatchSummaryRecord>, DispatchStoreError> {
    let mut stmt = conn.prepare(
        "SELECT dispatch_id, run_id, step_id, kind, target_agent, status, updated_at
         FROM agent_dispatch
         WHERE run_id = ?1
         ORDER BY updated_at DESC, dispatch_id DESC",
    )?;
    let rows = stmt.query_map([run_id], read_dispatch_summary_row)?;
    collect_rows(rows)
}

pub(crate) fn update_dispatch_row(
    conn: &Connection,
    current: &DispatchRecord,
    next: &DispatchRecord,
) -> Result<usize, DispatchStoreError> {
    conn.execute(
        "UPDATE agent_dispatch
         SET status = ?1,
             result_json = ?2,
             error_json = ?3,
             attempt = ?4,
             spawned_agent_id = ?5,
             provider_driver = ?6,
             session_id = ?7,
             provider_resume_token = ?8,
             updated_at = ?9,
             completed_at = ?10
         WHERE dispatch_id = ?11
           AND status = ?12",
        params![
            next.status.as_str(),
            serialize_optional_json(next.result_json.as_ref())?,
            serialize_optional_json(next.error_json.as_ref())?,
            i64::from(next.attempt),
            next.spawned_agent_id.as_deref(),
            next.provider_driver.as_deref(),
            next.session_id.as_deref(),
            next.provider_resume_token.as_deref(),
            next.updated_at.as_str(),
            next.completed_at.as_deref(),
            next.dispatch_id.as_str(),
            current.status.as_str(),
        ],
    )
    .map_err(Into::into)
}

pub(crate) fn resolve_update_conflict(
    conn: &Connection,
    dispatch_id: &str,
    expected: DispatchStatus,
) -> DispatchStoreError {
    match load_dispatch(conn, dispatch_id) {
        Ok(Some(record)) => DispatchStoreError::UnexpectedDispatchState {
            dispatch_id: dispatch_id.to_string(),
            expected,
            actual: record.status,
        },
        Ok(None) => DispatchStoreError::DispatchNotFound {
            dispatch_id: dispatch_id.to_string(),
        },
        Err(error) => error,
    }
}

fn validate_status_transition(
    current: &DispatchRecord,
    next: &DispatchRecord,
) -> Result<(), DispatchStoreError> {
    ensure_immutable_field(
        &current.dispatch_id,
        current.dispatch_id == next.dispatch_id,
        "dispatch_id",
    )?;
    ensure_immutable_field(
        &current.dispatch_id,
        current.run_id == next.run_id,
        "run_id",
    )?;
    ensure_immutable_field(
        &current.dispatch_id,
        current.step_id == next.step_id,
        "step_id",
    )?;
    ensure_immutable_field(&current.dispatch_id, current.kind == next.kind, "kind")?;
    ensure_immutable_field(
        &current.dispatch_id,
        current.target_agent == next.target_agent,
        "target_agent",
    )?;
    ensure_immutable_field(
        &current.dispatch_id,
        current.input_json == next.input_json,
        "input_json",
    )?;
    ensure_immutable_field(
        &current.dispatch_id,
        current.parent_dispatch_id == next.parent_dispatch_id,
        "parent_dispatch_id",
    )?;
    ensure_immutable_field(
        &current.dispatch_id,
        current.started_at == next.started_at,
        "started_at",
    )?;

    let retry_transition = matches!(
        (current.status, next.status),
        (DispatchStatus::Failed, DispatchStatus::Pending)
            | (DispatchStatus::Cancelled, DispatchStatus::Pending)
    );

    if retry_transition {
        let expected_attempt = current.attempt + 1;
        if next.attempt != expected_attempt {
            return Err(DispatchStoreError::InvalidAttemptIncrement {
                dispatch_id: current.dispatch_id.clone(),
                expected: expected_attempt,
                actual: next.attempt,
            });
        }
    } else if next.attempt != current.attempt {
        return Err(DispatchStoreError::InvalidAttemptIncrement {
            dispatch_id: current.dispatch_id.clone(),
            expected: current.attempt,
            actual: next.attempt,
        });
    }

    if current.status == next.status {
        return Err(DispatchStoreError::InvalidStatusTransition {
            dispatch_id: current.dispatch_id.clone(),
            from: current.status,
            to: next.status,
        });
    }

    if next.status.is_terminal() != next.completed_at.is_some() {
        return Err(DispatchStoreError::InvalidCompletionState {
            dispatch_id: current.dispatch_id.clone(),
            status: next.status,
        });
    }

    let legal = matches!(
        (current.status, next.status),
        (DispatchStatus::Pending, DispatchStatus::Running)
            | (DispatchStatus::Pending, DispatchStatus::Failed)
            | (DispatchStatus::Pending, DispatchStatus::Cancelled)
            | (DispatchStatus::Running, DispatchStatus::WaitingHitl)
            | (DispatchStatus::Running, DispatchStatus::Completed)
            | (DispatchStatus::Running, DispatchStatus::Failed)
            | (DispatchStatus::Running, DispatchStatus::Cancelled)
            | (DispatchStatus::WaitingHitl, DispatchStatus::Running)
            | (DispatchStatus::WaitingHitl, DispatchStatus::Failed)
            | (DispatchStatus::WaitingHitl, DispatchStatus::Cancelled)
            | (DispatchStatus::Failed, DispatchStatus::Pending)
            | (DispatchStatus::Cancelled, DispatchStatus::Pending)
    );

    if !legal {
        return Err(DispatchStoreError::InvalidStatusTransition {
            dispatch_id: current.dispatch_id.clone(),
            from: current.status,
            to: next.status,
        });
    }

    Ok(())
}

fn validate_payload_update(
    current: &DispatchRecord,
    field: &'static str,
) -> Result<(), DispatchStoreError> {
    if matches!(
        current.status,
        DispatchStatus::Running | DispatchStatus::WaitingHitl
    ) {
        return Ok(());
    }

    Err(DispatchStoreError::InvalidPayloadUpdateState {
        dispatch_id: current.dispatch_id.clone(),
        field,
        status: current.status,
    })
}

fn ensure_immutable_field(
    dispatch_id: &str,
    unchanged: bool,
    field: &'static str,
) -> Result<(), DispatchStoreError> {
    if !unchanged {
        return Err(DispatchStoreError::ImmutableFieldChanged {
            dispatch_id: dispatch_id.to_string(),
            field,
        });
    }
    Ok(())
}

fn serialize_optional_json(
    value: Option<&JsonValue>,
) -> Result<Option<String>, DispatchStoreError> {
    value
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<T>, DispatchStoreError>
where
    F: FnMut(&rusqlite::Row<'_>) -> Result<T, rusqlite::Error>,
{
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn read_dispatch_row(row: &rusqlite::Row<'_>) -> Result<DispatchRecord, rusqlite::Error> {
    let kind = row.get::<_, String>(3)?;
    let kind = kind.parse::<DispatchKind>().map_err(invalid_text_error)?;
    let status = row.get::<_, String>(5)?;
    let status = status
        .parse::<DispatchStatus>()
        .map_err(invalid_text_error)?;
    let input_json = parse_json_column(row.get::<_, String>(6)?)?;
    let result_json = parse_optional_json_column(row.get::<_, Option<String>>(7)?)?;
    let error_json = parse_optional_json_column(row.get::<_, Option<String>>(8)?)?;
    let attempt = i64_to_u32(row.get::<_, i64>(9)?).map_err(invalid_dispatch_error)?;

    Ok(DispatchRecord {
        dispatch_id: row.get(0)?,
        run_id: row.get(1)?,
        step_id: row.get(2)?,
        kind,
        target_agent: row.get(4)?,
        status,
        input_json,
        result_json,
        error_json,
        attempt,
        parent_dispatch_id: row.get(10)?,
        spawned_agent_id: row.get(11)?,
        provider_driver: row.get(12)?,
        session_id: row.get(13)?,
        provider_resume_token: row.get(14)?,
        started_at: row.get(15)?,
        updated_at: row.get(16)?,
        completed_at: row.get(17)?,
    })
}

fn read_dispatch_summary_row(
    row: &rusqlite::Row<'_>,
) -> Result<DispatchSummaryRecord, rusqlite::Error> {
    let kind = row.get::<_, String>(3)?;
    let kind = kind.parse::<DispatchKind>().map_err(invalid_text_error)?;
    let status = row.get::<_, String>(5)?;
    let status = status
        .parse::<DispatchStatus>()
        .map_err(invalid_text_error)?;

    Ok(DispatchSummaryRecord {
        dispatch_id: row.get(0)?,
        run_id: row.get(1)?,
        step_id: row.get(2)?,
        kind,
        target_agent: row.get(4)?,
        status,
        updated_at: row.get(6)?,
    })
}

fn parse_json_column(value: String) -> Result<JsonValue, rusqlite::Error> {
    serde_json::from_str(&value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn parse_optional_json_column(value: Option<String>) -> Result<Option<JsonValue>, rusqlite::Error> {
    value.map(parse_json_column).transpose()
}

fn invalid_text_error(error: DispatchStoreError) -> rusqlite::Error {
    invalid_dispatch_error(error)
}

fn invalid_dispatch_error(error: DispatchStoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn i64_to_u32(value: i64) -> Result<u32, DispatchStoreError> {
    u32::try_from(value).map_err(|_| DispatchStoreError::InvalidAttemptValue { value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    fn configure_test_connection(conn: &Connection) {
        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .expect("enable WAL mode");
        conn.busy_timeout(Duration::from_millis(5_000))
            .expect("set busy timeout");
    }

    fn migrated_in_memory_connection() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open in-memory compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(AGENT_DISPATCH_MIGRATION_SQL)
            .expect("apply agent_dispatch migration");
        Arc::new(Mutex::new(conn))
    }

    fn migrated_file_connection(path: &Path) -> Arc<Mutex<Connection>> {
        let conn = Connection::open(path).expect("open file-backed compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(AGENT_DISPATCH_MIGRATION_SQL)
            .expect("apply agent_dispatch migration");
        Arc::new(Mutex::new(conn))
    }

    fn sample_dispatch(dispatch_id: &str) -> DispatchRecord {
        DispatchRecord {
            dispatch_id: dispatch_id.to_string(),
            run_id: "run-123".to_string(),
            step_id: Some("write-prd".to_string()),
            kind: DispatchKind::Call,
            target_agent: "prd-writer".to_string(),
            status: DispatchStatus::Pending,
            input_json: serde_json::json!({
                "issue": {
                    "id": "ISSUE-123",
                },
            }),
            result_json: None,
            error_json: None,
            attempt: 1,
            parent_dispatch_id: None,
            spawned_agent_id: None,
            provider_driver: None,
            session_id: None,
            provider_resume_token: None,
            started_at: "2026-03-24T01:00:00Z".to_string(),
            updated_at: "2026-03-24T01:00:00Z".to_string(),
            completed_at: None,
        }
    }

    #[tokio::test]
    async fn dispatch_record_should_persist_all_required_fields() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let mut record = sample_dispatch("dispatch-roundtrip");
        record.kind = DispatchKind::Spawn;
        record.status = DispatchStatus::Running;
        record.result_json = Some(serde_json::json!({ "ok": true }));
        record.error_json = Some(serde_json::json!({ "message": "warn" }));
        record.parent_dispatch_id = Some("dispatch-parent".to_string());
        record.spawned_agent_id = Some("agent-spawned".to_string());
        record.provider_driver = Some("codex".to_string());
        record.session_id = Some("session-42".to_string());
        record.provider_resume_token = Some("resume-token".to_string());
        record.updated_at = "2026-03-24T01:05:00Z".to_string();

        repository.create(&record).await.expect("create dispatch");

        let loaded = repository
            .find_by_id(&record.dispatch_id)
            .await
            .expect("load dispatch")
            .expect("dispatch should exist");

        assert_eq!(loaded, record);
    }

    #[tokio::test]
    async fn dispatch_parent_child_linkage_should_persist_and_be_queryable() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let parent = sample_dispatch("dispatch-parent");
        let mut child_a = sample_dispatch("dispatch-child-a");
        let mut child_b = sample_dispatch("dispatch-child-b");
        let mut unrelated = sample_dispatch("dispatch-unrelated");
        child_a.parent_dispatch_id = Some(parent.dispatch_id.clone());
        child_b.parent_dispatch_id = Some(parent.dispatch_id.clone());
        unrelated.parent_dispatch_id = Some("dispatch-other".to_string());

        for record in [&parent, &child_a, &child_b, &unrelated] {
            repository.create(record).await.expect("create dispatch");
        }

        let children = repository
            .find_children(&parent.dispatch_id)
            .await
            .expect("load child dispatches");
        let child_ids = children
            .into_iter()
            .map(|record| record.dispatch_id)
            .collect::<Vec<_>>();

        assert_eq!(child_ids, vec![child_b.dispatch_id, child_a.dispatch_id]);
    }

    #[tokio::test]
    async fn dispatch_status_transitions_should_enforce_legality() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let pending = sample_dispatch("dispatch-status");
        repository.create(&pending).await.expect("create dispatch");

        let mut running = pending.clone();
        running.status = DispatchStatus::Running;
        running.updated_at = "2026-03-24T01:01:00Z".to_string();
        let running = repository
            .update_status(&pending, &running)
            .await
            .expect("pending -> running should succeed");

        let mut waiting = running.clone();
        waiting.status = DispatchStatus::WaitingHitl;
        waiting.updated_at = "2026-03-24T01:02:00Z".to_string();
        let waiting = repository
            .update_status(&running, &waiting)
            .await
            .expect("running -> waiting_hitl should succeed");

        let mut resumed = waiting.clone();
        resumed.status = DispatchStatus::Running;
        resumed.updated_at = "2026-03-24T01:03:00Z".to_string();
        let resumed = repository
            .update_status(&waiting, &resumed)
            .await
            .expect("waiting_hitl -> running should succeed");

        let completed = repository
            .mark_completed(
                &resumed,
                Some(&serde_json::json!({ "answer": "done" })),
                "2026-03-24T01:04:00Z",
            )
            .await
            .expect("running -> completed should succeed");

        let mut illegal_back_to_running = completed.clone();
        illegal_back_to_running.status = DispatchStatus::Running;
        illegal_back_to_running.updated_at = "2026-03-24T01:05:00Z".to_string();
        illegal_back_to_running.completed_at = None;

        let illegal = repository
            .update_status(&completed, &illegal_back_to_running)
            .await;

        assert!(matches!(
            illegal,
            Err(DispatchStoreError::InvalidStatusTransition { .. })
        ));

        let failed = repository
            .mark_failed(
                &sample_dispatch("dispatch-failed-illegal"),
                &serde_json::json!({ "message": "boom" }),
                "2026-03-24T01:05:00Z",
            )
            .await;

        assert!(matches!(
            failed,
            Err(DispatchStoreError::DispatchNotFound { .. })
        ));

        let failed_source = sample_dispatch("dispatch-failed-transition");
        repository
            .create(&failed_source)
            .await
            .expect("create failed transition source");
        let failed_source = repository
            .mark_failed(
                &failed_source,
                &serde_json::json!({ "message": "boom" }),
                "2026-03-24T01:06:00Z",
            )
            .await
            .expect("mark failed transition source");
        let mut illegal_waiting = failed_source.clone();
        illegal_waiting.status = DispatchStatus::WaitingHitl;
        illegal_waiting.updated_at = "2026-03-24T01:07:00Z".to_string();
        illegal_waiting.completed_at = None;

        let illegal_failed_transition = repository
            .update_status(&failed_source, &illegal_waiting)
            .await;

        assert!(matches!(
            illegal_failed_transition,
            Err(DispatchStoreError::InvalidStatusTransition { .. })
        ));
    }

    #[tokio::test]
    async fn dispatch_attempt_counter_should_increment_on_retry() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let mut record = sample_dispatch("dispatch-attempt");
        record.kind = DispatchKind::Spawn;
        record.spawned_agent_id = Some("agent-retry".to_string());
        record.provider_driver = Some("codex".to_string());
        record.session_id = Some("session-retry".to_string());
        record.provider_resume_token = Some("resume-retry".to_string());
        repository.create(&record).await.expect("create dispatch");

        let failed = repository
            .mark_failed(
                &record,
                &serde_json::json!({ "message": "boom" }),
                "2026-03-24T01:02:00Z",
            )
            .await
            .expect("mark failed");
        let retried = repository
            .increment_attempt(&failed, "2026-03-24T01:03:00Z")
            .await
            .expect("increment attempt");

        assert_eq!(retried.attempt, 2);
        assert_eq!(retried.status, DispatchStatus::Pending);
        assert_eq!(retried.completed_at, None);
        assert_eq!(retried.error_json, None);
        assert_eq!(retried.spawned_agent_id, None);
        assert_eq!(retried.provider_driver, None);
        assert_eq!(retried.session_id, None);
        assert_eq!(retried.provider_resume_token, None);
    }

    #[tokio::test]
    async fn dispatch_payload_updates_should_require_active_status() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let record = sample_dispatch("dispatch-payload-updates");
        repository.create(&record).await.expect("create dispatch");

        let pending_result = repository
            .update_result(
                &record.dispatch_id,
                Some(&serde_json::json!({ "answer": "pending" })),
                "2026-03-24T01:01:00Z",
            )
            .await;
        assert!(matches!(
            pending_result,
            Err(DispatchStoreError::InvalidPayloadUpdateState {
                field: "result_json",
                status: DispatchStatus::Pending,
                ..
            })
        ));

        let mut running = record.clone();
        running.status = DispatchStatus::Running;
        running.updated_at = "2026-03-24T01:02:00Z".to_string();
        repository
            .update_status(&record, &running)
            .await
            .expect("mark running");

        let with_result = repository
            .update_result(
                &record.dispatch_id,
                Some(&serde_json::json!({ "answer": "done" })),
                "2026-03-24T01:03:00Z",
            )
            .await
            .expect("store result payload while running");
        assert_eq!(
            with_result.result_json,
            Some(serde_json::json!({ "answer": "done" }))
        );
        assert_eq!(with_result.status, DispatchStatus::Running);

        let with_error = repository
            .update_error(
                &record.dispatch_id,
                Some(&serde_json::json!({ "message": "needs review" })),
                "2026-03-24T01:04:00Z",
            )
            .await
            .expect("store error payload while running");
        assert_eq!(
            with_error.error_json,
            Some(serde_json::json!({ "message": "needs review" }))
        );
        assert_eq!(with_error.status, DispatchStatus::Running);

        let completed = repository
            .mark_completed(
                &with_error,
                Some(&serde_json::json!({ "answer": "final" })),
                "2026-03-24T01:05:00Z",
            )
            .await
            .expect("complete dispatch");
        let completed_error = repository
            .update_error(
                &record.dispatch_id,
                Some(&serde_json::json!({ "message": "too late" })),
                "2026-03-24T01:06:00Z",
            )
            .await;
        assert!(matches!(
            completed_error,
            Err(DispatchStoreError::InvalidPayloadUpdateState {
                field: "error_json",
                status: DispatchStatus::Completed,
                ..
            })
        ));
        assert_eq!(completed.status, DispatchStatus::Completed);
    }

    #[tokio::test]
    async fn dispatch_kind_spawn_should_store_spawned_agent_id() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let mut record = sample_dispatch("dispatch-spawn");
        record.kind = DispatchKind::Spawn;
        repository.create(&record).await.expect("create dispatch");

        let mut running = record.clone();
        running.status = DispatchStatus::Running;
        running.updated_at = "2026-03-24T01:02:00Z".to_string();
        let running = repository
            .update_status(&record, &running)
            .await
            .expect("mark running");
        let mut completed = running.clone();
        completed.status = DispatchStatus::Completed;
        completed.spawned_agent_id = Some("agent-007".to_string());
        completed.updated_at = "2026-03-24T01:03:00Z".to_string();
        completed.completed_at = Some("2026-03-24T01:03:00Z".to_string());
        let completed = repository
            .update_status(&running, &completed)
            .await
            .expect("complete spawn dispatch");

        assert_eq!(completed.spawned_agent_id.as_deref(), Some("agent-007"));
    }

    #[tokio::test]
    async fn dispatch_find_by_run_should_return_all_run_dispatches() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let mut first = sample_dispatch("dispatch-run-a");
        let mut second = sample_dispatch("dispatch-run-b");
        let mut third = sample_dispatch("dispatch-run-c");
        first.updated_at = "2026-03-24T01:01:00Z".to_string();
        second.updated_at = "2026-03-24T01:02:00Z".to_string();
        third.run_id = "run-other".to_string();
        third.updated_at = "2026-03-24T01:03:00Z".to_string();

        for record in [&first, &second, &third] {
            repository.create(record).await.expect("create dispatch");
        }

        let run_records = repository
            .find_by_run(&first.run_id)
            .await
            .expect("find by run");
        let dispatch_ids = run_records
            .into_iter()
            .map(|record| record.dispatch_id)
            .collect::<Vec<_>>();

        assert_eq!(dispatch_ids, vec![second.dispatch_id, first.dispatch_id]);
    }

    #[tokio::test]
    async fn dispatch_of_kind_send_should_not_require_result() {
        let repository = SqliteDispatchRepository::new(migrated_in_memory_connection());
        let mut record = sample_dispatch("dispatch-send");
        record.kind = DispatchKind::Send;
        repository.create(&record).await.expect("create dispatch");

        let mut running = record.clone();
        running.status = DispatchStatus::Running;
        running.updated_at = "2026-03-24T01:01:00Z".to_string();
        let running = repository
            .update_status(&record, &running)
            .await
            .expect("mark running");
        let completed = repository
            .mark_completed(&running, None, "2026-03-24T01:02:00Z")
            .await
            .expect("complete send dispatch");

        assert_eq!(completed.kind, DispatchKind::Send);
        assert_eq!(completed.status, DispatchStatus::Completed);
        assert_eq!(completed.result_json, None);
    }

    #[tokio::test]
    async fn dispatch_repository_should_survive_connection_restart() {
        let temp = tempdir().expect("create tempdir");
        let db_path = temp.path().join("compozy.db");
        let repository = SqliteDispatchRepository::new(migrated_file_connection(&db_path));
        let record = sample_dispatch("dispatch-restart");
        repository.create(&record).await.expect("create dispatch");
        drop(repository);

        let reopened = SqliteDispatchRepository::new(migrated_file_connection(&db_path));
        let loaded = reopened
            .find_by_id(&record.dispatch_id)
            .await
            .expect("reload dispatch")
            .expect("dispatch should exist");

        assert_eq!(loaded, record);
    }

    #[tokio::test]
    async fn dispatch_repository_should_handle_concurrent_status_updates() {
        let temp = tempdir().expect("create tempdir");
        let db_path = temp.path().join("compozy.db");
        let repository_a = SqliteDispatchRepository::new(migrated_file_connection(&db_path));
        let repository_b = SqliteDispatchRepository::new(migrated_file_connection(&db_path));
        let record = sample_dispatch("dispatch-concurrency");
        repository_a.create(&record).await.expect("create dispatch");

        let current_a = repository_a
            .find_by_id(&record.dispatch_id)
            .await
            .expect("load dispatch for repo A")
            .expect("dispatch should exist");
        let current_b = repository_b
            .find_by_id(&record.dispatch_id)
            .await
            .expect("load dispatch for repo B")
            .expect("dispatch should exist");

        let mut next_a = current_a.clone();
        next_a.status = DispatchStatus::Running;
        next_a.updated_at = "2026-03-24T01:01:00Z".to_string();

        let mut next_b = current_b.clone();
        next_b.status = DispatchStatus::Cancelled;
        next_b.updated_at = "2026-03-24T01:02:00Z".to_string();
        next_b.completed_at = Some("2026-03-24T01:02:00Z".to_string());

        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let wait_a = Arc::clone(&barrier);
        let wait_b = Arc::clone(&barrier);

        let task_a = tokio::spawn(async move {
            wait_a.wait().await;
            repository_a.update_status(&current_a, &next_a).await
        });
        let task_b = tokio::spawn(async move {
            wait_b.wait().await;
            repository_b.update_status(&current_b, &next_b).await
        });

        barrier.wait().await;
        let result_a = task_a.await.expect("join task A");
        let result_b = task_b.await.expect("join task B");
        let successes = [result_a.is_ok(), result_b.is_ok()]
            .into_iter()
            .filter(|ok| *ok)
            .count();
        let failures = [result_a, result_b]
            .into_iter()
            .filter(|result| {
                matches!(
                    result,
                    Err(DispatchStoreError::UnexpectedDispatchState { .. })
                )
            })
            .count();
        let final_record = SqliteDispatchRepository::new(migrated_file_connection(&db_path))
            .find_by_id(&record.dispatch_id)
            .await
            .expect("reload dispatch")
            .expect("dispatch should exist");

        assert_eq!(successes, 1);
        assert_eq!(failures, 1);
        assert!(matches!(
            final_record.status,
            DispatchStatus::Running | DispatchStatus::Cancelled
        ));
    }
}
