//! Typed `compozy.db` repository for durable HITL request state.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use openfang_types::error::OpenFangError;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::sync::{Arc, Mutex, MutexGuard};
use thiserror::Error;

/// SQL for migration `0009_hitl_request`.
pub const HITL_REQUEST_MIGRATION_SQL: &str =
    include_str!("../migrations/compozy/20260324_009_hitl_request.sql");

/// Durable HITL kinds stored in `hitl_request.kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitlKind {
    /// Structured clarification question issued by a running agent step.
    Clarification,
    /// Explicit approval request reserved for future product flows.
    Approval,
    /// Choice among enumerated options reserved for future product flows.
    Choice,
    /// Freeform human response reserved for future product flows.
    Freeform,
}

impl HitlKind {
    /// Returns the canonical SQLite / JSON string encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clarification => "clarification",
            Self::Approval => "approval",
            Self::Choice => "choice",
            Self::Freeform => "freeform",
        }
    }

    fn parse_db_text(value: &str) -> Result<Self, HitlStoreError> {
        match value {
            "clarification" => Ok(Self::Clarification),
            "approval" => Ok(Self::Approval),
            "choice" => Ok(Self::Choice),
            "freeform" => Ok(Self::Freeform),
            other => Err(HitlStoreError::InvalidHitlKind {
                kind: other.to_string(),
            }),
        }
    }
}

impl std::fmt::Display for HitlKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for HitlKind {
    type Err = HitlStoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value)
    }
}

/// Durable HITL lifecycle states stored in `hitl_request.status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitlStatus {
    /// The request is awaiting a human answer.
    Pending,
    /// The request has been answered.
    Answered,
    /// The request was explicitly cancelled.
    Cancelled,
    /// The request deadline elapsed without an answer.
    TimedOut,
}

impl HitlStatus {
    /// Returns the canonical SQLite / JSON string encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Answered => "answered",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }

    /// Whether the status is terminal.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Answered | Self::Cancelled | Self::TimedOut)
    }

    fn parse_db_text(value: &str) -> Result<Self, HitlStoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "answered" => Ok(Self::Answered),
            "cancelled" => Ok(Self::Cancelled),
            "timed_out" => Ok(Self::TimedOut),
            other => Err(HitlStoreError::InvalidHitlStatus {
                status: other.to_string(),
            }),
        }
    }
}

impl std::fmt::Display for HitlStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for HitlStatus {
    type Err = HitlStoreError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse_db_text(value)
    }
}

/// Immutable input required to create a new durable HITL request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewHitlRequest {
    /// Stable HITL request identifier.
    pub hitl_request_id: String,
    /// Owning workflow run identifier.
    pub run_id: String,
    /// Workflow step that issued the request.
    pub step_id: String,
    /// Dispatch that issued the request, if any.
    pub dispatch_id: Option<String>,
    /// Product-level HITL interaction kind.
    pub kind: HitlKind,
    /// Human-facing question text.
    pub question: String,
    /// Structured opaque context payload.
    pub context_json: JsonValue,
    /// Request creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Optional timeout deadline.
    pub timeout_at: Option<DateTime<Utc>>,
}

/// Full durable HITL row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitlRecord {
    /// Stable HITL request identifier.
    pub hitl_request_id: String,
    /// Owning workflow run identifier.
    pub run_id: String,
    /// Workflow step that issued the request.
    pub step_id: String,
    /// Dispatch that issued the request, if any.
    pub dispatch_id: Option<String>,
    /// Product-level HITL interaction kind.
    pub kind: HitlKind,
    /// Current durable lifecycle state.
    pub status: HitlStatus,
    /// Human-facing question text.
    pub question: String,
    /// Structured opaque context payload.
    pub context_json: JsonValue,
    /// Structured human response payload, if answered.
    pub response_json: Option<JsonValue>,
    /// Sequence number scoped to `(run_id, step_id, dispatch_id)`.
    pub sequence_no: u32,
    /// Request creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Timestamp when the request was answered.
    pub answered_at: Option<DateTime<Utc>>,
    /// Optional timeout deadline.
    pub timeout_at: Option<DateTime<Utc>>,
}

/// Filter + pagination input for HITL list surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HitlListQuery {
    /// Maximum number of items to return.
    pub limit: usize,
    /// Cursor offset encoded as a row offset.
    pub offset: usize,
    /// Optional workflow run filter.
    pub run_id: Option<String>,
    /// Optional dispatch filter.
    pub dispatch_id: Option<String>,
    /// Optional status filter.
    pub status: Option<HitlStatus>,
    /// Optional kind filter.
    pub kind: Option<HitlKind>,
}

impl Default for HitlListQuery {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
            run_id: None,
            dispatch_id: None,
            status: None,
            kind: None,
        }
    }
}

/// Paginated durable HITL list page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitlListPage {
    /// Page items in durable sort order.
    pub items: Vec<HitlRecord>,
    /// Next cursor offset, if more items exist.
    pub next_cursor: Option<String>,
}

/// Typed failures from the durable HITL repository.
#[derive(Debug, Error)]
pub enum HitlStoreError {
    /// Failed to acquire the shared `compozy.db` connection lock.
    #[error("failed to acquire compozy.db connection lock: {0}")]
    ConnectionLock(String),
    /// SQLite returned an error for the requested operation.
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    /// JSON serialization or deserialization failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// The requested HITL record does not exist.
    #[error("hitl request '{hitl_request_id}' was not found")]
    HitlRequestNotFound {
        /// Missing HITL request identifier.
        hitl_request_id: String,
    },
    /// A stored kind string was invalid.
    #[error("invalid hitl kind '{kind}'")]
    InvalidHitlKind {
        /// Stored invalid text value.
        kind: String,
    },
    /// A stored status string was invalid.
    #[error("invalid hitl status '{status}'")]
    InvalidHitlStatus {
        /// Stored invalid text value.
        status: String,
    },
    /// A transition violated the repository state machine.
    #[error("hitl request '{hitl_request_id}' transition {from} -> {to} is not allowed")]
    InvalidStatusTransition {
        /// HITL request being updated.
        hitl_request_id: String,
        /// Current status.
        from: HitlStatus,
        /// Requested next status.
        to: HitlStatus,
    },
    /// A stored timestamp could not be parsed as RFC 3339.
    #[error("invalid RFC 3339 timestamp for '{column}': {value}")]
    InvalidTimestamp {
        /// Column with invalid text.
        column: &'static str,
        /// Stored invalid text value.
        value: String,
    },
    /// A stored integer could not be represented as `u32`.
    #[error("hitl sequence value {value} is outside the supported u32 range")]
    InvalidSequenceValue {
        /// Invalid SQLite integer.
        value: i64,
    },
}

impl From<HitlStoreError> for OpenFangError {
    fn from(error: HitlStoreError) -> Self {
        OpenFangError::Memory(error.to_string())
    }
}

/// Async storage contract for durable HITL persistence.
#[async_trait]
pub trait HitlRepository: Send + Sync {
    /// Inserts a new durable HITL row and assigns its next sequence number.
    async fn create(&self, request: NewHitlRequest) -> Result<HitlRecord, HitlStoreError>;

    /// Loads one HITL request by its stable identifier.
    async fn find_by_id(&self, hitl_request_id: &str)
        -> Result<Option<HitlRecord>, HitlStoreError>;

    /// Lists pending HITL requests for one workflow run.
    async fn find_pending_for_run(&self, run_id: &str) -> Result<Vec<HitlRecord>, HitlStoreError>;

    /// Lists all HITL requests linked to one dispatch.
    async fn find_by_dispatch(&self, dispatch_id: &str) -> Result<Vec<HitlRecord>, HitlStoreError>;

    /// Lists HITL rows matching the provided filters and pagination.
    async fn list(&self, query: &HitlListQuery) -> Result<HitlListPage, HitlStoreError>;

    /// Marks a request answered and writes the response atomically.
    async fn answer(
        &self,
        hitl_request_id: &str,
        response_json: &JsonValue,
        answered_at: DateTime<Utc>,
    ) -> Result<HitlRecord, HitlStoreError>;

    /// Cancels a pending request.
    async fn cancel(&self, hitl_request_id: &str) -> Result<HitlRecord, HitlStoreError>;

    /// Marks a pending request timed out.
    async fn mark_timed_out(&self, hitl_request_id: &str) -> Result<HitlRecord, HitlStoreError>;
}

/// SQLite-backed HITL repository.
#[derive(Clone)]
pub struct SqliteHitlRepository {
    conn: Arc<Mutex<Connection>>,
}

/// Backward-compatible store alias.
pub type HitlStore = SqliteHitlRepository;

impl SqliteHitlRepository {
    /// Creates the repository from a shared `compozy.db` connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl HitlRepository for SqliteHitlRepository {
    async fn create(&self, request: NewHitlRequest) -> Result<HitlRecord, HitlStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let sequence_no = next_sequence_no(
            &transaction,
            &request.run_id,
            &request.step_id,
            request.dispatch_id.as_deref(),
        )?;

        let record = HitlRecord {
            hitl_request_id: request.hitl_request_id,
            run_id: request.run_id,
            step_id: request.step_id,
            dispatch_id: request.dispatch_id,
            kind: request.kind,
            status: HitlStatus::Pending,
            question: request.question,
            context_json: request.context_json,
            response_json: None,
            sequence_no,
            created_at: request.created_at,
            answered_at: None,
            timeout_at: request.timeout_at,
        };
        insert_hitl_request(&transaction, &record)?;
        transaction.commit()?;

        Ok(record)
    }

    async fn find_by_id(
        &self,
        hitl_request_id: &str,
    ) -> Result<Option<HitlRecord>, HitlStoreError> {
        let conn = lock_conn(&self.conn)?;
        load_hitl_request(&conn, hitl_request_id)
    }

    async fn find_pending_for_run(&self, run_id: &str) -> Result<Vec<HitlRecord>, HitlStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_pending_hitl_requests_for_run(&conn, run_id)
    }

    async fn find_by_dispatch(&self, dispatch_id: &str) -> Result<Vec<HitlRecord>, HitlStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_hitl_requests_by_dispatch(&conn, dispatch_id)
    }

    async fn list(&self, query: &HitlListQuery) -> Result<HitlListPage, HitlStoreError> {
        let conn = lock_conn(&self.conn)?;
        list_hitl_requests(&conn, query)
    }

    async fn answer(
        &self,
        hitl_request_id: &str,
        response_json: &JsonValue,
        answered_at: DateTime<Utc>,
    ) -> Result<HitlRecord, HitlStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_required_hitl_request(&transaction, hitl_request_id)?;
        ensure_pending_transition(&current, HitlStatus::Answered)?;
        let response_json_text = serde_json::to_string(response_json)?;
        let answered_at_text = answered_at.to_rfc3339();

        let next = HitlRecord {
            status: HitlStatus::Answered,
            response_json: Some(response_json.clone()),
            answered_at: Some(answered_at),
            ..current
        };
        transaction.execute(
            "UPDATE hitl_request
             SET status = ?1,
                 response_json = ?2,
                 answered_at = ?3
             WHERE hitl_request_id = ?4",
            params![
                next.status.as_str(),
                response_json_text,
                answered_at_text,
                next.hitl_request_id.as_str(),
            ],
        )?;
        transaction.commit()?;

        Ok(next)
    }

    async fn cancel(&self, hitl_request_id: &str) -> Result<HitlRecord, HitlStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_required_hitl_request(&transaction, hitl_request_id)?;
        ensure_pending_transition(&current, HitlStatus::Cancelled)?;

        let next = HitlRecord {
            status: HitlStatus::Cancelled,
            ..current
        };
        transaction.execute(
            "UPDATE hitl_request
             SET status = ?1
             WHERE hitl_request_id = ?2",
            params![next.status.as_str(), next.hitl_request_id.as_str()],
        )?;
        transaction.commit()?;

        Ok(next)
    }

    async fn mark_timed_out(&self, hitl_request_id: &str) -> Result<HitlRecord, HitlStoreError> {
        let mut conn = lock_conn(&self.conn)?;
        let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = load_required_hitl_request(&transaction, hitl_request_id)?;
        ensure_pending_transition(&current, HitlStatus::TimedOut)?;

        let next = HitlRecord {
            status: HitlStatus::TimedOut,
            ..current
        };
        transaction.execute(
            "UPDATE hitl_request
             SET status = ?1
             WHERE hitl_request_id = ?2",
            params![next.status.as_str(), next.hitl_request_id.as_str()],
        )?;
        transaction.commit()?;

        Ok(next)
    }
}

fn lock_conn(conn: &Arc<Mutex<Connection>>) -> Result<MutexGuard<'_, Connection>, HitlStoreError> {
    conn.lock()
        .map_err(|error| HitlStoreError::ConnectionLock(error.to_string()))
}

pub(crate) fn insert_hitl_request(
    conn: &Connection,
    record: &HitlRecord,
) -> Result<(), HitlStoreError> {
    conn.execute(
        "INSERT INTO hitl_request (
            hitl_request_id,
            run_id,
            step_id,
            dispatch_id,
            kind,
            status,
            question,
            context_json,
            response_json,
            sequence_no,
            created_at,
            answered_at,
            timeout_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            record.hitl_request_id.as_str(),
            record.run_id.as_str(),
            record.step_id.as_str(),
            record.dispatch_id.as_deref(),
            record.kind.as_str(),
            record.status.as_str(),
            record.question.as_str(),
            serde_json::to_string(&record.context_json)?,
            serialize_optional_json(record.response_json.as_ref())?,
            i64::from(record.sequence_no),
            record.created_at.to_rfc3339(),
            serialize_optional_datetime(record.answered_at.as_ref()),
            serialize_optional_datetime(record.timeout_at.as_ref()),
        ],
    )?;
    Ok(())
}

pub(crate) fn load_hitl_request(
    conn: &Connection,
    hitl_request_id: &str,
) -> Result<Option<HitlRecord>, HitlStoreError> {
    conn.query_row(
        "SELECT
            hitl_request_id,
            run_id,
            step_id,
            dispatch_id,
            kind,
            status,
            question,
            context_json,
            response_json,
            sequence_no,
            created_at,
            answered_at,
            timeout_at
         FROM hitl_request
         WHERE hitl_request_id = ?1",
        [hitl_request_id],
        read_hitl_row,
    )
    .optional()
    .map_err(Into::into)
}

pub(crate) fn load_required_hitl_request(
    conn: &Connection,
    hitl_request_id: &str,
) -> Result<HitlRecord, HitlStoreError> {
    load_hitl_request(conn, hitl_request_id)?.ok_or_else(|| HitlStoreError::HitlRequestNotFound {
        hitl_request_id: hitl_request_id.to_string(),
    })
}

fn list_pending_hitl_requests_for_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Vec<HitlRecord>, HitlStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            hitl_request_id,
            run_id,
            step_id,
            dispatch_id,
            kind,
            status,
            question,
            context_json,
            response_json,
            sequence_no,
            created_at,
            answered_at,
            timeout_at
         FROM hitl_request
         WHERE run_id = ?1
           AND status = 'pending'
         ORDER BY created_at ASC, step_id ASC, sequence_no ASC, hitl_request_id ASC",
    )?;
    let rows = stmt.query_map([run_id], read_hitl_row)?;
    collect_rows(rows)
}

fn list_hitl_requests_by_dispatch(
    conn: &Connection,
    dispatch_id: &str,
) -> Result<Vec<HitlRecord>, HitlStoreError> {
    let mut stmt = conn.prepare(
        "SELECT
            hitl_request_id,
            run_id,
            step_id,
            dispatch_id,
            kind,
            status,
            question,
            context_json,
            response_json,
            sequence_no,
            created_at,
            answered_at,
            timeout_at
         FROM hitl_request
         WHERE dispatch_id = ?1
         ORDER BY run_id ASC, step_id ASC, sequence_no ASC, hitl_request_id ASC",
    )?;
    let rows = stmt.query_map([dispatch_id], read_hitl_row)?;
    collect_rows(rows)
}

fn list_hitl_requests(
    conn: &Connection,
    query: &HitlListQuery,
) -> Result<HitlListPage, HitlStoreError> {
    let run_id = query.run_id.as_deref();
    let dispatch_id = query.dispatch_id.as_deref();
    let status = query.status.map(HitlStatus::as_str);
    let kind = query.kind.map(HitlKind::as_str);
    let limit = query.limit.saturating_add(1) as i64;
    let offset = query.offset as i64;
    let mut stmt = conn.prepare(
        "SELECT
            hitl_request_id,
            run_id,
            step_id,
            dispatch_id,
            kind,
            status,
            question,
            context_json,
            response_json,
            sequence_no,
            created_at,
            answered_at,
            timeout_at
         FROM hitl_request
         WHERE (?1 IS NULL OR run_id = ?1)
           AND (?2 IS NULL OR dispatch_id = ?2)
           AND (?3 IS NULL OR status = ?3)
           AND (?4 IS NULL OR kind = ?4)
         ORDER BY created_at ASC, step_id ASC, sequence_no ASC, hitl_request_id ASC
         LIMIT ?5 OFFSET ?6",
    )?;
    let rows = stmt.query_map(
        params![run_id, dispatch_id, status, kind, limit, offset],
        read_hitl_row,
    )?;
    let mut items = collect_rows(rows)?;
    let next_cursor = if items.len() > query.limit {
        items.truncate(query.limit);
        Some((query.offset + query.limit).to_string())
    } else {
        None
    };
    Ok(HitlListPage { items, next_cursor })
}

pub(crate) fn next_sequence_no(
    conn: &Connection,
    run_id: &str,
    step_id: &str,
    dispatch_id: Option<&str>,
) -> Result<u32, HitlStoreError> {
    let next_value = match dispatch_id {
        Some(dispatch_id) => conn.query_row(
            "SELECT COALESCE(MAX(sequence_no), 0) + 1
             FROM hitl_request
             WHERE run_id = ?1
               AND step_id = ?2
               AND dispatch_id = ?3",
            params![run_id, step_id, dispatch_id],
            |row| row.get::<_, i64>(0),
        )?,
        None => conn.query_row(
            "SELECT COALESCE(MAX(sequence_no), 0) + 1
             FROM hitl_request
             WHERE run_id = ?1
               AND step_id = ?2
               AND dispatch_id IS NULL",
            params![run_id, step_id],
            |row| row.get::<_, i64>(0),
        )?,
    };

    i64_to_u32(next_value)
}

pub(crate) fn ensure_pending_transition(
    current: &HitlRecord,
    next: HitlStatus,
) -> Result<(), HitlStoreError> {
    if current.status != HitlStatus::Pending {
        return Err(HitlStoreError::InvalidStatusTransition {
            hitl_request_id: current.hitl_request_id.clone(),
            from: current.status,
            to: next,
        });
    }

    Ok(())
}

fn serialize_optional_json(value: Option<&JsonValue>) -> Result<Option<String>, HitlStoreError> {
    value
        .map(serde_json::to_string)
        .transpose()
        .map_err(Into::into)
}

fn serialize_optional_datetime(value: Option<&DateTime<Utc>>) -> Option<String> {
    value.map(DateTime::to_rfc3339)
}

fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<T>, HitlStoreError>
where
    F: FnMut(&rusqlite::Row<'_>) -> Result<T, rusqlite::Error>,
{
    let mut records = Vec::new();
    for row in rows {
        records.push(row?);
    }
    Ok(records)
}

fn read_hitl_row(row: &rusqlite::Row<'_>) -> Result<HitlRecord, rusqlite::Error> {
    let kind = row.get::<_, String>(4)?;
    let kind = kind.parse::<HitlKind>().map_err(invalid_hitl_error)?;
    let status = row.get::<_, String>(5)?;
    let status = status.parse::<HitlStatus>().map_err(invalid_hitl_error)?;
    let context_json = parse_json_column(row.get::<_, String>(7)?)?;
    let response_json = parse_optional_json_column(row.get::<_, Option<String>>(8)?)?;
    let sequence_no = i64_to_u32(row.get::<_, i64>(9)?).map_err(invalid_hitl_error)?;

    Ok(HitlRecord {
        hitl_request_id: row.get(0)?,
        run_id: row.get(1)?,
        step_id: row.get(2)?,
        dispatch_id: row.get(3)?,
        kind,
        status,
        question: row.get(6)?,
        context_json,
        response_json,
        sequence_no,
        created_at: parse_required_datetime(row.get::<_, String>(10)?, "created_at")?,
        answered_at: parse_optional_datetime(row.get::<_, Option<String>>(11)?, "answered_at")?,
        timeout_at: parse_optional_datetime(row.get::<_, Option<String>>(12)?, "timeout_at")?,
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

fn parse_required_datetime(
    value: String,
    column: &'static str,
) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::parse_from_rfc3339(&value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .map_err(|_| invalid_hitl_error(HitlStoreError::InvalidTimestamp { column, value }))
}

fn parse_optional_datetime(
    value: Option<String>,
    column: &'static str,
) -> Result<Option<DateTime<Utc>>, rusqlite::Error> {
    value
        .map(|timestamp| parse_required_datetime(timestamp, column))
        .transpose()
}

fn invalid_hitl_error(error: HitlStoreError) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}

fn i64_to_u32(value: i64) -> Result<u32, HitlStoreError> {
    u32::try_from(value).map_err(|_| HitlStoreError::InvalidSequenceValue { value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dispatch::{
        DispatchKind, DispatchRecord, DispatchRepository, DispatchStatus, SqliteDispatchRepository,
        AGENT_DISPATCH_MIGRATION_SQL,
    };
    use crate::workflow_store::{
        WorkflowRunRecord, WorkflowRunRepository, WorkflowRunStatus,
        WORKFLOW_RUN_CORE_MIGRATION_SQL,
    };
    use pretty_assertions::assert_eq;
    use rusqlite::Connection;
    use std::path::Path;
    use std::time::Duration;
    use tempfile::tempdir;

    fn timestamp(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .expect("valid RFC 3339 timestamp")
            .with_timezone(&Utc)
    }

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
        conn.execute_batch(WORKFLOW_RUN_CORE_MIGRATION_SQL)
            .expect("apply workflow_run migration");
        conn.execute_batch(AGENT_DISPATCH_MIGRATION_SQL)
            .expect("apply agent_dispatch migration");
        conn.execute_batch(HITL_REQUEST_MIGRATION_SQL)
            .expect("apply hitl_request migration");
        Arc::new(Mutex::new(conn))
    }

    fn migrated_file_connection(path: &Path) -> Arc<Mutex<Connection>> {
        let conn = Connection::open(path).expect("open file-backed compozy.db");
        configure_test_connection(&conn);
        conn.execute_batch(WORKFLOW_RUN_CORE_MIGRATION_SQL)
            .expect("apply workflow_run migration");
        conn.execute_batch(AGENT_DISPATCH_MIGRATION_SQL)
            .expect("apply agent_dispatch migration");
        conn.execute_batch(HITL_REQUEST_MIGRATION_SQL)
            .expect("apply hitl_request migration");
        Arc::new(Mutex::new(conn))
    }

    fn sample_run(run_id: &str) -> WorkflowRunRecord {
        WorkflowRunRecord {
            run_id: run_id.to_string(),
            workflow_id: "workflow-prd".to_string(),
            workflow_version: "1.0.0".to_string(),
            status: WorkflowRunStatus::Running,
            input_json: serde_json::json!({ "artifact": "prd" }).to_string(),
            vars_json: serde_json::json!({}).to_string(),
            current_step_id: Some("write-prd".to_string()),
            waiting_kind: None,
            waiting_ref: None,
            active_dispatch_id: None,
            active_hitl_request_id: None,
            labels_json: serde_json::json!(["prd"]).to_string(),
            metadata_json: serde_json::json!({ "source": "test" }).to_string(),
            error_json: None,
            started_at: "2026-03-24T02:00:00Z".to_string(),
            updated_at: "2026-03-24T02:00:00Z".to_string(),
            completed_at: None,
        }
    }

    fn sample_dispatch(dispatch_id: &str, run_id: &str, step_id: &str) -> DispatchRecord {
        DispatchRecord {
            dispatch_id: dispatch_id.to_string(),
            run_id: run_id.to_string(),
            step_id: Some(step_id.to_string()),
            kind: DispatchKind::Call,
            target_agent: "prd-writer".to_string(),
            status: DispatchStatus::Running,
            input_json: serde_json::json!({ "section": "overview" }),
            result_json: None,
            error_json: None,
            attempt: 1,
            parent_dispatch_id: None,
            spawned_agent_id: None,
            provider_driver: Some("codex".to_string()),
            session_id: Some("session-hitl".to_string()),
            provider_resume_token: None,
            started_at: "2026-03-24T02:00:00Z".to_string(),
            updated_at: "2026-03-24T02:00:00Z".to_string(),
            completed_at: None,
        }
    }

    fn seed_run(conn: Arc<Mutex<Connection>>, run_id: &str) {
        WorkflowRunRepository::new(Arc::clone(&conn))
            .insert_run(&sample_run(run_id))
            .expect("insert workflow run");
    }

    async fn seed_dispatch_for_existing_run(
        conn: Arc<Mutex<Connection>>,
        run_id: &str,
        step_id: &str,
        dispatch_id: &str,
    ) {
        SqliteDispatchRepository::new(conn)
            .create(&sample_dispatch(dispatch_id, run_id, step_id))
            .await
            .expect("insert dispatch");
    }

    async fn seed_run_and_dispatch(
        conn: Arc<Mutex<Connection>>,
        run_id: &str,
        step_id: &str,
        dispatch_id: &str,
    ) {
        seed_run(Arc::clone(&conn), run_id);
        seed_dispatch_for_existing_run(conn, run_id, step_id, dispatch_id).await;
    }

    fn sample_request(
        hitl_request_id: &str,
        run_id: &str,
        step_id: &str,
        dispatch_id: Option<&str>,
    ) -> NewHitlRequest {
        NewHitlRequest {
            hitl_request_id: hitl_request_id.to_string(),
            run_id: run_id.to_string(),
            step_id: step_id.to_string(),
            dispatch_id: dispatch_id.map(str::to_string),
            kind: HitlKind::Clarification,
            question: "Should the PRD prioritize B2B admins or end users first?".to_string(),
            context_json: serde_json::json!({
                "artifact_id": "artifact-001",
                "artifact_type": "prd",
            }),
            created_at: timestamp("2026-03-24T02:05:00Z"),
            timeout_at: Some(timestamp("2026-03-24T03:05:00Z")),
        }
    }

    #[tokio::test]
    async fn hitl_record_should_persist_all_required_fields() {
        let conn = migrated_in_memory_connection();
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-roundtrip",
            "write-prd",
            "dispatch-hitl-roundtrip",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);
        let record = repository
            .create(sample_request(
                "hitl-roundtrip",
                "run-hitl-roundtrip",
                "write-prd",
                Some("dispatch-hitl-roundtrip"),
            ))
            .await
            .expect("create hitl request");
        let answered = repository
            .answer(
                &record.hitl_request_id,
                &serde_json::json!({
                    "type": "text",
                    "value": "B2B admins first",
                }),
                timestamp("2026-03-24T02:06:00Z"),
            )
            .await
            .expect("answer hitl request");

        let loaded = repository
            .find_by_id(&record.hitl_request_id)
            .await
            .expect("load hitl request")
            .expect("hitl request should exist");

        assert_eq!(loaded, answered);
        assert_eq!(loaded.sequence_no, 1);
        assert_eq!(loaded.status, HitlStatus::Answered);
        assert_eq!(
            loaded.dispatch_id.as_deref(),
            Some("dispatch-hitl-roundtrip")
        );
        assert_eq!(
            loaded.response_json,
            Some(serde_json::json!({
                "type": "text",
                "value": "B2B admins first",
            }))
        );
        assert_eq!(loaded.answered_at, Some(timestamp("2026-03-24T02:06:00Z")));
        assert_eq!(loaded.timeout_at, Some(timestamp("2026-03-24T03:05:00Z")));
    }

    #[tokio::test]
    async fn hitl_sequence_numbers_should_be_ordered_within_step() {
        let conn = migrated_in_memory_connection();
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-sequence",
            "write-prd",
            "dispatch-hitl-sequence",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);

        let first = repository
            .create(sample_request(
                "hitl-sequence-1",
                "run-hitl-sequence",
                "write-prd",
                Some("dispatch-hitl-sequence"),
            ))
            .await
            .expect("create first hitl request");
        let second = repository
            .create(sample_request(
                "hitl-sequence-2",
                "run-hitl-sequence",
                "write-prd",
                Some("dispatch-hitl-sequence"),
            ))
            .await
            .expect("create second hitl request");
        let third = repository
            .create(sample_request(
                "hitl-sequence-3",
                "run-hitl-sequence",
                "write-prd",
                Some("dispatch-hitl-sequence"),
            ))
            .await
            .expect("create third hitl request");

        assert_eq!(
            vec![first.sequence_no, second.sequence_no, third.sequence_no],
            vec![1, 2, 3]
        );
    }

    #[tokio::test]
    async fn hitl_sequence_numbers_should_restart_across_steps() {
        let conn = migrated_in_memory_connection();
        seed_run(Arc::clone(&conn), "run-hitl-step-reset");
        seed_dispatch_for_existing_run(
            Arc::clone(&conn),
            "run-hitl-step-reset",
            "write-prd",
            "dispatch-hitl-step-a",
        )
        .await;
        seed_dispatch_for_existing_run(
            Arc::clone(&conn),
            "run-hitl-step-reset",
            "review-prd",
            "dispatch-hitl-step-b",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);

        let step_a = repository
            .create(sample_request(
                "hitl-step-a-1",
                "run-hitl-step-reset",
                "write-prd",
                Some("dispatch-hitl-step-a"),
            ))
            .await
            .expect("create step A hitl request");
        let step_b = repository
            .create(sample_request(
                "hitl-step-b-1",
                "run-hitl-step-reset",
                "review-prd",
                Some("dispatch-hitl-step-b"),
            ))
            .await
            .expect("create step B hitl request");

        assert_eq!(step_a.sequence_no, 1);
        assert_eq!(step_b.sequence_no, 1);
    }

    #[tokio::test]
    async fn hitl_answer_should_write_response_and_timestamp_atomically() {
        let conn = migrated_in_memory_connection();
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-answer",
            "write-prd",
            "dispatch-hitl-answer",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);
        let created = repository
            .create(sample_request(
                "hitl-answer",
                "run-hitl-answer",
                "write-prd",
                Some("dispatch-hitl-answer"),
            ))
            .await
            .expect("create hitl request");

        let answered = repository
            .answer(
                &created.hitl_request_id,
                &serde_json::json!({
                    "type": "choice",
                    "value": "b2b_admins_first",
                }),
                timestamp("2026-03-24T02:07:00Z"),
            )
            .await
            .expect("answer hitl request");

        assert_eq!(answered.status, HitlStatus::Answered);
        assert_eq!(
            answered.response_json,
            Some(serde_json::json!({
                "type": "choice",
                "value": "b2b_admins_first",
            }))
        );
        assert_eq!(
            answered.answered_at,
            Some(timestamp("2026-03-24T02:07:00Z"))
        );
    }

    #[tokio::test]
    async fn hitl_answer_should_fail_on_non_pending_request() {
        let conn = migrated_in_memory_connection();
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-answer-fail",
            "write-prd",
            "dispatch-hitl-answer-fail",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);
        let created = repository
            .create(sample_request(
                "hitl-answer-fail",
                "run-hitl-answer-fail",
                "write-prd",
                Some("dispatch-hitl-answer-fail"),
            ))
            .await
            .expect("create hitl request");
        let cancelled = repository
            .cancel(&created.hitl_request_id)
            .await
            .expect("cancel hitl request");

        let error = repository
            .answer(
                &cancelled.hitl_request_id,
                &serde_json::json!({
                    "type": "text",
                    "value": "too late",
                }),
                timestamp("2026-03-24T02:08:00Z"),
            )
            .await;

        assert!(matches!(
            error,
            Err(HitlStoreError::InvalidStatusTransition {
                from: HitlStatus::Cancelled,
                to: HitlStatus::Answered,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn hitl_status_terminal_states_should_not_transition() {
        let conn = migrated_in_memory_connection();
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-terminal",
            "write-prd",
            "dispatch-hitl-terminal",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);
        let created = repository
            .create(sample_request(
                "hitl-terminal",
                "run-hitl-terminal",
                "write-prd",
                Some("dispatch-hitl-terminal"),
            ))
            .await
            .expect("create hitl request");
        let answered = repository
            .answer(
                &created.hitl_request_id,
                &serde_json::json!({
                    "type": "text",
                    "value": "Proceed",
                }),
                timestamp("2026-03-24T02:09:00Z"),
            )
            .await
            .expect("answer hitl request");

        let error = repository.cancel(&answered.hitl_request_id).await;

        assert!(matches!(
            error,
            Err(HitlStoreError::InvalidStatusTransition {
                from: HitlStatus::Answered,
                to: HitlStatus::Cancelled,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn hitl_find_pending_for_run_should_return_only_pending() {
        let conn = migrated_in_memory_connection();
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-pending",
            "write-prd",
            "dispatch-hitl-pending",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);
        let pending = repository
            .create(sample_request(
                "hitl-pending-1",
                "run-hitl-pending",
                "write-prd",
                Some("dispatch-hitl-pending"),
            ))
            .await
            .expect("create pending hitl request");
        let answered = repository
            .create(sample_request(
                "hitl-pending-2",
                "run-hitl-pending",
                "write-prd",
                Some("dispatch-hitl-pending"),
            ))
            .await
            .expect("create answerable hitl request");
        repository
            .answer(
                &answered.hitl_request_id,
                &serde_json::json!({
                    "type": "text",
                    "value": "done",
                }),
                timestamp("2026-03-24T02:10:00Z"),
            )
            .await
            .expect("answer second hitl request");

        let pending_records = repository
            .find_pending_for_run(&pending.run_id)
            .await
            .expect("list pending hitl requests");

        assert_eq!(pending_records.len(), 1);
        assert_eq!(pending_records[0].hitl_request_id, pending.hitl_request_id);
        assert_eq!(pending_records[0].status, HitlStatus::Pending);
    }

    #[tokio::test]
    async fn hitl_find_by_dispatch_should_scope_correctly() {
        let conn = migrated_in_memory_connection();
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-dispatch-a",
            "write-prd",
            "dispatch-hitl-a",
        )
        .await;
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-dispatch-b",
            "write-prd",
            "dispatch-hitl-b",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);
        let first = repository
            .create(sample_request(
                "hitl-dispatch-a-1",
                "run-hitl-dispatch-a",
                "write-prd",
                Some("dispatch-hitl-a"),
            ))
            .await
            .expect("create first dispatch-scoped request");
        let second = repository
            .create(sample_request(
                "hitl-dispatch-a-2",
                "run-hitl-dispatch-a",
                "write-prd",
                Some("dispatch-hitl-a"),
            ))
            .await
            .expect("create second dispatch-scoped request");
        repository
            .create(sample_request(
                "hitl-dispatch-b-1",
                "run-hitl-dispatch-b",
                "write-prd",
                Some("dispatch-hitl-b"),
            ))
            .await
            .expect("create unrelated dispatch-scoped request");

        let scoped = repository
            .find_by_dispatch("dispatch-hitl-a")
            .await
            .expect("find hitl requests by dispatch");
        let ids = scoped
            .into_iter()
            .map(|record| record.hitl_request_id)
            .collect::<Vec<_>>();

        assert_eq!(ids, vec![first.hitl_request_id, second.hitl_request_id]);
    }

    #[tokio::test]
    async fn hitl_repository_should_survive_connection_restart() {
        let temp = tempdir().expect("create tempdir");
        let db_path = temp.path().join("compozy.db");
        let conn = migrated_file_connection(&db_path);
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-restart",
            "write-prd",
            "dispatch-hitl-restart",
        )
        .await;
        let repository = SqliteHitlRepository::new(conn);
        let created = repository
            .create(sample_request(
                "hitl-restart",
                "run-hitl-restart",
                "write-prd",
                Some("dispatch-hitl-restart"),
            ))
            .await
            .expect("create hitl request");
        drop(repository);

        let reopened = SqliteHitlRepository::new(migrated_file_connection(&db_path));
        let loaded = reopened
            .find_by_id(&created.hitl_request_id)
            .await
            .expect("reload hitl request")
            .expect("hitl request should exist");

        assert_eq!(loaded, created);
    }

    #[tokio::test]
    async fn hitl_repository_sequence_assignment_should_be_race_safe() {
        let temp = tempdir().expect("create tempdir");
        let db_path = temp.path().join("compozy.db");
        let conn = migrated_file_connection(&db_path);
        seed_run_and_dispatch(
            Arc::clone(&conn),
            "run-hitl-race",
            "write-prd",
            "dispatch-hitl-race",
        )
        .await;
        drop(conn);

        let repository_a = SqliteHitlRepository::new(migrated_file_connection(&db_path));
        let repository_b = SqliteHitlRepository::new(migrated_file_connection(&db_path));
        let request_a = sample_request(
            "hitl-race-a",
            "run-hitl-race",
            "write-prd",
            Some("dispatch-hitl-race"),
        );
        let request_b = sample_request(
            "hitl-race-b",
            "run-hitl-race",
            "write-prd",
            Some("dispatch-hitl-race"),
        );

        let barrier = Arc::new(tokio::sync::Barrier::new(3));
        let wait_a = Arc::clone(&barrier);
        let wait_b = Arc::clone(&barrier);

        let task_a = tokio::spawn(async move {
            wait_a.wait().await;
            repository_a.create(request_a).await
        });
        let task_b = tokio::spawn(async move {
            wait_b.wait().await;
            repository_b.create(request_b).await
        });

        barrier.wait().await;
        let record_a = task_a.await.expect("join create task A").expect("create A");
        let record_b = task_b.await.expect("join create task B").expect("create B");

        let mut sequence_numbers = vec![record_a.sequence_no, record_b.sequence_no];
        sequence_numbers.sort_unstable();

        assert_eq!(sequence_numbers, vec![1, 2]);
    }

    #[tokio::test]
    async fn hitl_requests_without_dispatch_should_sequence_independently() {
        let conn = migrated_in_memory_connection();
        seed_run(Arc::clone(&conn), "run-hitl-no-dispatch");
        let repository = SqliteHitlRepository::new(conn);
        let first = repository
            .create(sample_request(
                "hitl-no-dispatch-1",
                "run-hitl-no-dispatch",
                "write-prd",
                None,
            ))
            .await
            .expect("create first no-dispatch request");
        let second = repository
            .create(sample_request(
                "hitl-no-dispatch-2",
                "run-hitl-no-dispatch",
                "write-prd",
                None,
            ))
            .await
            .expect("create second no-dispatch request");

        assert_eq!(first.sequence_no, 1);
        assert_eq!(second.sequence_no, 2);
    }
}
