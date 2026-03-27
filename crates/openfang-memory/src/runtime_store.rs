//! Typed runtime.db stores for agent and schedule operational state.

use chrono::{Duration, Utc};
use openfang_types::agent::{AgentId, AgentMode, AgentState, SessionId};
use openfang_types::error::{OpenFangError, OpenFangResult};
use openfang_types::message::{Message, Role};
use rusqlite::{params, Connection, OptionalExtension};
use std::sync::{Arc, Mutex, MutexGuard};
use uuid::Uuid;

/// SQL for migration `0002_agent_runtime_core`.
pub const AGENT_RUNTIME_CORE_MIGRATION_SQL: &str =
    include_str!("../migrations/runtime/20260321_002_agent_runtime_core.sql");

/// SQL for migration `0003_agent_sessions_and_messages`.
pub const AGENT_SESSIONS_AND_MESSAGES_MIGRATION_SQL: &str =
    include_str!("../migrations/runtime/20260321_003_agent_sessions_and_messages.sql");

/// SQL for migration `0004_schedule_runtime_core`.
pub const SCHEDULE_RUNTIME_CORE_MIGRATION_SQL: &str =
    include_str!("../migrations/runtime/20260321_004_schedule_runtime_core.sql");

/// SQL for migration `0005_trigger_runtime_core`.
pub const TRIGGER_RUNTIME_CORE_MIGRATION_SQL: &str =
    include_str!("../migrations/runtime/20260325_005_trigger_runtime_core.sql");

/// Shared runtime.db store handles.
#[derive(Clone)]
pub struct RuntimeStoreSet {
    /// Store for durable agent runtime projections.
    pub agent_runtime: AgentRuntimeStore,
    /// Store for durable agent session metadata.
    pub agent_session: AgentSessionStore,
    /// Store for durable agent message history.
    pub agent_message: AgentMessageStore,
    /// Store for schedule runtime projections.
    pub schedule_runtime: ScheduleRuntimeStore,
    /// Store for schedule execution receipts.
    pub schedule_execution: ScheduleExecutionStore,
    /// Store for trigger runtime projections.
    pub trigger_runtime: TriggerRuntimeStore,
}

impl RuntimeStoreSet {
    /// Create the full runtime store set from a shared runtime.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            agent_runtime: AgentRuntimeStore::new(Arc::clone(&conn)),
            agent_session: AgentSessionStore::new(Arc::clone(&conn)),
            agent_message: AgentMessageStore::new(Arc::clone(&conn)),
            schedule_runtime: ScheduleRuntimeStore::new(Arc::clone(&conn)),
            trigger_runtime: TriggerRuntimeStore::new(Arc::clone(&conn)),
            schedule_execution: ScheduleExecutionStore::new(conn),
        }
    }
}

/// Durable runtime projection for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRuntimeRecord {
    /// Stable agent ID shared with file-backed definitions.
    pub agent_id: AgentId,
    /// Whether the agent is currently loaded.
    pub loaded: bool,
    /// Durable operational state.
    pub state: AgentState,
    /// Durable operational mode.
    pub mode: AgentMode,
    /// Health signal used by runtime endpoints.
    pub healthy: bool,
    /// Current active session ID, if any.
    pub active_session_id: Option<SessionId>,
    /// Number of active dispatches owned by this agent.
    pub active_dispatches: u32,
    /// Last known activity timestamp (RFC 3339).
    pub last_active_at: Option<String>,
    /// Last projection update timestamp (RFC 3339).
    pub updated_at: String,
}

/// Durable metadata for one direct-agent session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionRecord {
    /// Session identifier.
    pub session_id: SessionId,
    /// Owning agent identifier.
    pub agent_id: AgentId,
    /// Optional human-readable label.
    pub label: Option<String>,
    /// Whether this is the active session for the agent.
    pub active: bool,
    /// Current number of persisted messages.
    pub message_count: u32,
    /// Number of durable dispatches tied to this session.
    pub dispatch_count: u32,
    /// Session creation timestamp (RFC 3339).
    pub created_at: String,
    /// Session update timestamp (RFC 3339).
    pub updated_at: String,
    /// Compaction timestamp, if a summary replaced older messages.
    pub compacted_at: Option<String>,
}

/// Durable runtime message record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentMessageRecord {
    /// Durable message identifier.
    pub message_id: String,
    /// Owning agent identifier.
    pub agent_id: AgentId,
    /// Session identifier.
    pub session_id: SessionId,
    /// Message direction (`inbound`, `outbound`, or `system`).
    pub direction: String,
    /// Full message payload as JSON.
    pub payload_json: String,
    /// Message processing status.
    pub status: String,
    /// Message creation timestamp (RFC 3339).
    pub created_at: String,
    /// Completion timestamp, if any.
    pub completed_at: Option<String>,
}

/// Durable schedule runtime projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleRuntimeRecord {
    /// Stable schedule ID.
    pub schedule_id: String,
    /// Whether the schedule is enabled.
    pub enabled: bool,
    /// Timestamp of the last execution, if any.
    pub last_run: Option<String>,
    /// Timestamp of the next planned execution, if any.
    pub next_run: Option<String>,
    /// Last execution status.
    pub last_status: Option<String>,
    /// Consecutive execution error count.
    pub consecutive_errors: u32,
    /// Whether the schedule is removed after one success.
    pub one_shot: bool,
    /// Last projection update timestamp (RFC 3339).
    pub updated_at: String,
}

/// Durable schedule execution receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleExecutionRecord {
    /// Execution receipt identifier.
    pub execution_id: String,
    /// Stable schedule ID.
    pub schedule_id: String,
    /// Time the schedule fired (RFC 3339).
    pub fired_at: String,
    /// Execution status (`ok`, `error`, etc.).
    pub status: String,
    /// Optional effect payload.
    pub effect_json: Option<String>,
    /// Optional execution error message.
    pub error: Option<String>,
}

/// Durable trigger runtime projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerRuntimeRecord {
    /// Stable trigger definition identifier.
    pub trigger_id: String,
    /// Whether the trigger is enabled for active matching.
    pub enabled: bool,
    /// Number of successful dispatches recorded so far.
    pub fire_count: u64,
    /// Maximum number of fires before auto-disable. Zero means unlimited.
    pub max_fires: u64,
    /// Cooldown window in seconds between dispatches.
    pub cooldown_secs: u64,
    /// Timestamp of the last successful fire, if any.
    pub last_fired_at: Option<String>,
    /// Last projection update timestamp (RFC 3339).
    pub updated_at: String,
}

/// Store for `agent_runtime`.
#[derive(Clone)]
pub struct AgentRuntimeStore {
    conn: Arc<Mutex<Connection>>,
}

/// Store for `agent_session`.
#[derive(Clone)]
pub struct AgentSessionStore {
    conn: Arc<Mutex<Connection>>,
}

/// Store for `agent_message`.
#[derive(Clone)]
pub struct AgentMessageStore {
    conn: Arc<Mutex<Connection>>,
}

/// Store for `schedule_runtime`.
#[derive(Clone)]
pub struct ScheduleRuntimeStore {
    conn: Arc<Mutex<Connection>>,
}

/// Store for `schedule_execution`.
#[derive(Clone)]
pub struct ScheduleExecutionStore {
    conn: Arc<Mutex<Connection>>,
}

/// Store for `trigger_runtime`.
#[derive(Clone)]
pub struct TriggerRuntimeStore {
    conn: Arc<Mutex<Connection>>,
}

impl AgentRuntimeStore {
    /// Create a runtime store from a shared runtime.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or update an agent runtime projection.
    pub fn upsert_agent_runtime(&self, record: &AgentRuntimeRecord) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO agent_runtime (
                agent_id,
                loaded,
                state,
                mode,
                healthy,
                active_session_id,
                active_dispatches,
                last_active_at,
                updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(agent_id) DO UPDATE SET
                loaded = excluded.loaded,
                state = excluded.state,
                mode = excluded.mode,
                healthy = excluded.healthy,
                active_session_id = excluded.active_session_id,
                active_dispatches = excluded.active_dispatches,
                last_active_at = excluded.last_active_at,
                updated_at = excluded.updated_at",
            params![
                record.agent_id.to_string(),
                bool_to_sql(record.loaded),
                encode_agent_state(record.state),
                encode_agent_mode(record.mode),
                bool_to_sql(record.healthy),
                record.active_session_id.as_ref().map(ToString::to_string),
                i64::from(record.active_dispatches),
                record.last_active_at.as_deref(),
                record.updated_at.as_str(),
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Load one agent runtime projection by ID.
    pub fn get_agent_runtime(
        &self,
        agent_id: AgentId,
    ) -> OpenFangResult<Option<AgentRuntimeRecord>> {
        let conn = lock_conn(&self.conn)?;
        conn.prepare(
            "SELECT
                agent_id,
                loaded,
                state,
                mode,
                healthy,
                active_session_id,
                active_dispatches,
                last_active_at,
                updated_at
             FROM agent_runtime
             WHERE agent_id = ?1",
        )
        .and_then(|mut stmt| {
            stmt.query_row([agent_id.to_string()], read_agent_runtime_row)
                .optional()
        })
        .map_err(sqlite_err)
    }

    /// List all agent runtime projections.
    pub fn list_agent_runtimes(&self) -> OpenFangResult<Vec<AgentRuntimeRecord>> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT
                    agent_id,
                    loaded,
                    state,
                    mode,
                    healthy,
                    active_session_id,
                    active_dispatches,
                    last_active_at,
                    updated_at
                 FROM agent_runtime
                 ORDER BY agent_id",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], read_agent_runtime_row)
            .map_err(sqlite_err)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(sqlite_err)?);
        }
        Ok(records)
    }

    /// Delete one runtime projection by agent ID.
    pub fn remove_agent_runtime(&self, agent_id: AgentId) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "DELETE FROM agent_runtime WHERE agent_id = ?1",
            [agent_id.to_string()],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }
}

impl AgentSessionStore {
    /// Create a session store from a shared runtime.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or update a durable agent session projection.
    pub fn upsert_agent_session(&self, record: &AgentSessionRecord) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO agent_session (
                session_id,
                agent_id,
                label,
                active,
                message_count,
                dispatch_count,
                created_at,
                updated_at,
                compacted_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(session_id) DO UPDATE SET
                agent_id = excluded.agent_id,
                label = excluded.label,
                active = excluded.active,
                message_count = excluded.message_count,
                dispatch_count = excluded.dispatch_count,
                created_at = agent_session.created_at,
                updated_at = excluded.updated_at,
                compacted_at = excluded.compacted_at",
            params![
                record.session_id.to_string(),
                record.agent_id.to_string(),
                record.label.as_deref(),
                bool_to_sql(record.active),
                i64::from(record.message_count),
                i64::from(record.dispatch_count),
                record.created_at.as_str(),
                record.updated_at.as_str(),
                record.compacted_at.as_deref(),
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Load one agent session record by session ID.
    pub fn get_agent_session(
        &self,
        session_id: SessionId,
    ) -> OpenFangResult<Option<AgentSessionRecord>> {
        let conn = lock_conn(&self.conn)?;
        conn.prepare(
            "SELECT
                session_id,
                agent_id,
                label,
                active,
                message_count,
                dispatch_count,
                created_at,
                updated_at,
                compacted_at
             FROM agent_session
             WHERE session_id = ?1",
        )
        .and_then(|mut stmt| {
            stmt.query_row([session_id.to_string()], read_agent_session_row)
                .optional()
        })
        .map_err(sqlite_err)
    }

    /// List all sessions for one agent.
    pub fn list_agent_sessions_for_agent(
        &self,
        agent_id: AgentId,
    ) -> OpenFangResult<Vec<AgentSessionRecord>> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT
                    session_id,
                    agent_id,
                    label,
                    active,
                    message_count,
                    dispatch_count,
                    created_at,
                    updated_at,
                    compacted_at
                 FROM agent_session
                 WHERE agent_id = ?1
                 ORDER BY created_at",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([agent_id.to_string()], read_agent_session_row)
            .map_err(sqlite_err)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(sqlite_err)?);
        }
        Ok(records)
    }

    /// Mark a specific session as active and all other sessions for the agent as inactive.
    pub fn mark_active_session(
        &self,
        agent_id: AgentId,
        session_id: SessionId,
    ) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "UPDATE agent_session
             SET active = CASE WHEN session_id = ?2 THEN 1 ELSE 0 END,
                 updated_at = ?3
             WHERE agent_id = ?1",
            params![
                agent_id.to_string(),
                session_id.to_string(),
                Utc::now().to_rfc3339(),
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Delete one durable session projection.
    pub fn remove_agent_session(&self, session_id: SessionId) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "DELETE FROM agent_session WHERE session_id = ?1",
            [session_id.to_string()],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Delete all durable session projections owned by an agent.
    pub fn remove_agent_sessions_for_agent(&self, agent_id: AgentId) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "DELETE FROM agent_session WHERE agent_id = ?1",
            [agent_id.to_string()],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }
}

impl AgentMessageStore {
    /// Create a message store from a shared runtime.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or replace a single runtime message record.
    pub fn insert_message(&self, record: &AgentMessageRecord) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO agent_message (
                message_id,
                agent_id,
                session_id,
                direction,
                payload_json,
                status,
                created_at,
                completed_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(message_id) DO UPDATE SET
                agent_id = excluded.agent_id,
                session_id = excluded.session_id,
                direction = excluded.direction,
                payload_json = excluded.payload_json,
                status = excluded.status,
                created_at = excluded.created_at,
                completed_at = excluded.completed_at",
            params![
                record.message_id.as_str(),
                record.agent_id.to_string(),
                record.session_id.to_string(),
                record.direction.as_str(),
                record.payload_json.as_str(),
                record.status.as_str(),
                record.created_at.as_str(),
                record.completed_at.as_deref(),
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Replace the full message projection for one session from the legacy session history.
    pub fn replace_messages_for_session(
        &self,
        agent_id: AgentId,
        session_id: SessionId,
        messages: &[Message],
    ) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        let transaction = conn.unchecked_transaction().map_err(sqlite_err)?;
        transaction
            .execute(
                "DELETE FROM agent_message WHERE session_id = ?1",
                [session_id.to_string()],
            )
            .map_err(sqlite_err)?;

        for (index, message) in messages.iter().enumerate() {
            let payload_json = serde_json::to_string(message)
                .map_err(|error| OpenFangError::Serialization(error.to_string()))?;
            let direction = direction_for_message(message);
            let message_id = deterministic_message_id(&session_id, index, &direction);
            let created_at = (Utc::now() + Duration::microseconds(index as i64)).to_rfc3339();

            transaction
                .execute(
                    "INSERT INTO agent_message (
                        message_id,
                        agent_id,
                        session_id,
                        direction,
                        payload_json,
                        status,
                        created_at,
                        completed_at
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        message_id,
                        agent_id.to_string(),
                        session_id.to_string(),
                        direction,
                        payload_json,
                        "completed",
                        created_at,
                        Option::<String>::None,
                    ],
                )
                .map_err(sqlite_err)?;
        }

        transaction.commit().map_err(sqlite_err)?;
        Ok(())
    }

    /// List all projected messages for one session ordered by creation time.
    pub fn list_messages_for_session(
        &self,
        session_id: SessionId,
    ) -> OpenFangResult<Vec<AgentMessageRecord>> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT
                    message_id,
                    agent_id,
                    session_id,
                    direction,
                    payload_json,
                    status,
                    created_at,
                    completed_at
                 FROM agent_message
                 WHERE session_id = ?1
                 ORDER BY created_at, message_id",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([session_id.to_string()], read_agent_message_row)
            .map_err(sqlite_err)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(sqlite_err)?);
        }
        Ok(records)
    }

    /// Delete all projected messages for one session.
    pub fn delete_messages_for_session(&self, session_id: SessionId) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "DELETE FROM agent_message WHERE session_id = ?1",
            [session_id.to_string()],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Delete all projected messages owned by an agent.
    pub fn delete_messages_for_agent(&self, agent_id: AgentId) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "DELETE FROM agent_message WHERE agent_id = ?1",
            [agent_id.to_string()],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }
}

impl ScheduleRuntimeStore {
    /// Create a schedule runtime store from a shared runtime.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or update one schedule runtime projection.
    pub fn upsert_schedule_runtime(&self, record: &ScheduleRuntimeRecord) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO schedule_runtime (
                schedule_id,
                enabled,
                last_run,
                next_run,
                last_status,
                consecutive_errors,
                one_shot,
                updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            ON CONFLICT(schedule_id) DO UPDATE SET
                enabled = excluded.enabled,
                last_run = excluded.last_run,
                next_run = excluded.next_run,
                last_status = excluded.last_status,
                consecutive_errors = excluded.consecutive_errors,
                one_shot = excluded.one_shot,
                updated_at = excluded.updated_at",
            params![
                record.schedule_id.as_str(),
                bool_to_sql(record.enabled),
                record.last_run.as_deref(),
                record.next_run.as_deref(),
                record.last_status.as_deref(),
                i64::from(record.consecutive_errors),
                bool_to_sql(record.one_shot),
                record.updated_at.as_str(),
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Load one schedule runtime projection by ID.
    pub fn get_schedule_runtime(
        &self,
        schedule_id: &str,
    ) -> OpenFangResult<Option<ScheduleRuntimeRecord>> {
        let conn = lock_conn(&self.conn)?;
        conn.prepare(
            "SELECT
                schedule_id,
                enabled,
                last_run,
                next_run,
                last_status,
                consecutive_errors,
                one_shot,
                updated_at
             FROM schedule_runtime
             WHERE schedule_id = ?1",
        )
        .and_then(|mut stmt| {
            stmt.query_row([schedule_id], read_schedule_runtime_row)
                .optional()
        })
        .map_err(sqlite_err)
    }

    /// List all schedule runtime projections.
    pub fn list_schedule_runtimes(&self) -> OpenFangResult<Vec<ScheduleRuntimeRecord>> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT
                    schedule_id,
                    enabled,
                    last_run,
                    next_run,
                    last_status,
                    consecutive_errors,
                    one_shot,
                    updated_at
                 FROM schedule_runtime
                 ORDER BY schedule_id",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], read_schedule_runtime_row)
            .map_err(sqlite_err)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(sqlite_err)?);
        }
        Ok(records)
    }

    /// Delete one schedule runtime projection.
    pub fn remove_schedule_runtime(&self, schedule_id: &str) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "DELETE FROM schedule_runtime WHERE schedule_id = ?1",
            [schedule_id],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }
}

impl ScheduleExecutionStore {
    /// Create a schedule execution store from a shared runtime.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert one schedule execution receipt.
    pub fn record_execution(&self, record: &ScheduleExecutionRecord) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO schedule_execution (
                execution_id,
                schedule_id,
                fired_at,
                status,
                effect_json,
                error
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.execution_id.as_str(),
                record.schedule_id.as_str(),
                record.fired_at.as_str(),
                record.status.as_str(),
                record.effect_json.as_deref(),
                record.error.as_deref(),
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// List execution receipts for one schedule ordered by fired time.
    pub fn list_executions_for_schedule(
        &self,
        schedule_id: &str,
    ) -> OpenFangResult<Vec<ScheduleExecutionRecord>> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT
                    execution_id,
                    schedule_id,
                    fired_at,
                    status,
                    effect_json,
                    error
                 FROM schedule_execution
                 WHERE schedule_id = ?1
                 ORDER BY fired_at",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([schedule_id], read_schedule_execution_row)
            .map_err(sqlite_err)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(sqlite_err)?);
        }
        Ok(records)
    }
}

impl TriggerRuntimeStore {
    /// Create a trigger runtime store from a shared runtime.db connection.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }

    /// Insert or update one trigger runtime projection.
    pub fn upsert_trigger_runtime(&self, record: &TriggerRuntimeRecord) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "INSERT INTO trigger_runtime (
                trigger_id,
                enabled,
                fire_count,
                max_fires,
                cooldown_secs,
                last_fired_at,
                updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(trigger_id) DO UPDATE SET
                enabled = excluded.enabled,
                fire_count = excluded.fire_count,
                max_fires = excluded.max_fires,
                cooldown_secs = excluded.cooldown_secs,
                last_fired_at = excluded.last_fired_at,
                updated_at = excluded.updated_at",
            params![
                record.trigger_id.as_str(),
                bool_to_sql(record.enabled),
                u64_to_i64(record.fire_count)?,
                u64_to_i64(record.max_fires)?,
                u64_to_i64(record.cooldown_secs)?,
                record.last_fired_at.as_deref(),
                record.updated_at.as_str(),
            ],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }

    /// Load one trigger runtime projection by ID.
    pub fn get_trigger_runtime(
        &self,
        trigger_id: &str,
    ) -> OpenFangResult<Option<TriggerRuntimeRecord>> {
        let conn = lock_conn(&self.conn)?;
        conn.prepare(
            "SELECT
                trigger_id,
                enabled,
                fire_count,
                max_fires,
                cooldown_secs,
                last_fired_at,
                updated_at
             FROM trigger_runtime
             WHERE trigger_id = ?1",
        )
        .and_then(|mut stmt| {
            stmt.query_row([trigger_id], read_trigger_runtime_row)
                .optional()
        })
        .map_err(sqlite_err)
    }

    /// List all trigger runtime projections.
    pub fn list_trigger_runtimes(&self) -> OpenFangResult<Vec<TriggerRuntimeRecord>> {
        let conn = lock_conn(&self.conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT
                    trigger_id,
                    enabled,
                    fire_count,
                    max_fires,
                    cooldown_secs,
                    last_fired_at,
                    updated_at
                 FROM trigger_runtime
                 ORDER BY trigger_id",
            )
            .map_err(sqlite_err)?;
        let rows = stmt
            .query_map([], read_trigger_runtime_row)
            .map_err(sqlite_err)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(sqlite_err)?);
        }
        Ok(records)
    }

    /// Delete one trigger runtime projection.
    pub fn remove_trigger_runtime(&self, trigger_id: &str) -> OpenFangResult<()> {
        let conn = lock_conn(&self.conn)?;
        conn.execute(
            "DELETE FROM trigger_runtime WHERE trigger_id = ?1",
            [trigger_id],
        )
        .map_err(sqlite_err)?;
        Ok(())
    }
}

fn lock_conn(conn: &Arc<Mutex<Connection>>) -> OpenFangResult<MutexGuard<'_, Connection>> {
    conn.lock().map_err(|error| {
        OpenFangError::Internal(format!(
            "Failed to acquire runtime.db connection lock: {error}"
        ))
    })
}

fn sqlite_err(error: rusqlite::Error) -> OpenFangError {
    OpenFangError::Memory(error.to_string())
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

fn parse_agent_id(value: String) -> Result<AgentId, rusqlite::Error> {
    Uuid::parse_str(&value).map(AgentId).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

fn parse_session_id(value: Option<String>) -> Result<Option<SessionId>, rusqlite::Error> {
    value
        .map(|value| {
            Uuid::parse_str(&value)
                .map(SessionId::from_uuid)
                .map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })
        })
        .transpose()
}

fn parse_required_session_id(value: &str) -> Result<SessionId, rusqlite::Error> {
    Uuid::parse_str(value)
        .map(SessionId::from_uuid)
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn i64_to_u32(value: i64) -> Result<u32, rusqlite::Error> {
    u32::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn i64_to_u64(value: i64) -> Result<u64, rusqlite::Error> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn u64_to_i64(value: u64) -> OpenFangResult<i64> {
    i64::try_from(value).map_err(|error| {
        OpenFangError::Internal(format!(
            "runtime.db integer overflow while storing unsigned value: {error}"
        ))
    })
}

fn encode_agent_state(value: AgentState) -> &'static str {
    match value {
        AgentState::Created => "created",
        AgentState::Running => "running",
        AgentState::Suspended => "suspended",
        AgentState::Terminated => "terminated",
        AgentState::Crashed => "crashed",
    }
}

fn decode_agent_state(value: &str) -> Result<AgentState, rusqlite::Error> {
    match value {
        "created" => Ok(AgentState::Created),
        "running" => Ok(AgentState::Running),
        "suspended" => Ok(AgentState::Suspended),
        "terminated" => Ok(AgentState::Terminated),
        "crashed" => Ok(AgentState::Crashed),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("invalid agent state '{other}'").into(),
        )),
    }
}

fn encode_agent_mode(value: AgentMode) -> &'static str {
    match value {
        AgentMode::Observe => "observe",
        AgentMode::Assist => "assist",
        AgentMode::Full => "full",
    }
}

fn decode_agent_mode(value: &str) -> Result<AgentMode, rusqlite::Error> {
    match value {
        "observe" => Ok(AgentMode::Observe),
        "assist" => Ok(AgentMode::Assist),
        "full" => Ok(AgentMode::Full),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("invalid agent mode '{other}'").into(),
        )),
    }
}

fn direction_for_message(message: &Message) -> String {
    match message.role {
        Role::User => "inbound".to_string(),
        Role::Assistant => "outbound".to_string(),
        Role::System => "system".to_string(),
    }
}

fn deterministic_message_id(session_id: &SessionId, index: usize, direction: &str) -> String {
    let key = format!("{session_id}:{index}:{direction}");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()).to_string()
}

fn read_agent_runtime_row(row: &rusqlite::Row<'_>) -> Result<AgentRuntimeRecord, rusqlite::Error> {
    let active_session_id: Option<String> = row.get(5)?;
    Ok(AgentRuntimeRecord {
        agent_id: parse_agent_id(row.get(0)?)?,
        loaded: sql_to_bool(row.get::<_, i64>(1)?),
        state: decode_agent_state(&row.get::<_, String>(2)?)?,
        mode: decode_agent_mode(&row.get::<_, String>(3)?)?,
        healthy: sql_to_bool(row.get::<_, i64>(4)?),
        active_session_id: parse_session_id(active_session_id)?,
        active_dispatches: i64_to_u32(row.get(6)?)?,
        last_active_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn read_agent_session_row(row: &rusqlite::Row<'_>) -> Result<AgentSessionRecord, rusqlite::Error> {
    let session_id: String = row.get(0)?;
    Ok(AgentSessionRecord {
        session_id: parse_required_session_id(&session_id)?,
        agent_id: parse_agent_id(row.get(1)?)?,
        label: row.get(2)?,
        active: sql_to_bool(row.get::<_, i64>(3)?),
        message_count: i64_to_u32(row.get(4)?)?,
        dispatch_count: i64_to_u32(row.get(5)?)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
        compacted_at: row.get(8)?,
    })
}

fn read_agent_message_row(row: &rusqlite::Row<'_>) -> Result<AgentMessageRecord, rusqlite::Error> {
    let agent_id: String = row.get(1)?;
    let session_id: String = row.get(2)?;
    Ok(AgentMessageRecord {
        message_id: row.get(0)?,
        agent_id: parse_agent_id(agent_id)?,
        session_id: parse_required_session_id(&session_id)?,
        direction: row.get(3)?,
        payload_json: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        completed_at: row.get(7)?,
    })
}

fn read_schedule_runtime_row(
    row: &rusqlite::Row<'_>,
) -> Result<ScheduleRuntimeRecord, rusqlite::Error> {
    Ok(ScheduleRuntimeRecord {
        schedule_id: row.get(0)?,
        enabled: sql_to_bool(row.get::<_, i64>(1)?),
        last_run: row.get(2)?,
        next_run: row.get(3)?,
        last_status: row.get(4)?,
        consecutive_errors: i64_to_u32(row.get(5)?)?,
        one_shot: sql_to_bool(row.get::<_, i64>(6)?),
        updated_at: row.get(7)?,
    })
}

fn read_schedule_execution_row(
    row: &rusqlite::Row<'_>,
) -> Result<ScheduleExecutionRecord, rusqlite::Error> {
    Ok(ScheduleExecutionRecord {
        execution_id: row.get(0)?,
        schedule_id: row.get(1)?,
        fired_at: row.get(2)?,
        status: row.get(3)?,
        effect_json: row.get(4)?,
        error: row.get(5)?,
    })
}

fn read_trigger_runtime_row(
    row: &rusqlite::Row<'_>,
) -> Result<TriggerRuntimeRecord, rusqlite::Error> {
    Ok(TriggerRuntimeRecord {
        trigger_id: row.get(0)?,
        enabled: sql_to_bool(row.get::<_, i64>(1)?),
        fire_count: i64_to_u64(row.get(2)?)?,
        max_fires: i64_to_u64(row.get(3)?)?,
        cooldown_secs: i64_to_u64(row.get(4)?)?,
        last_fired_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn runtime_conn() -> Arc<Mutex<Connection>> {
        let conn = Connection::open_in_memory().expect("open in-memory runtime.db");
        conn.execute_batch(AGENT_RUNTIME_CORE_MIGRATION_SQL)
            .expect("apply agent runtime schema");
        conn.execute_batch(AGENT_SESSIONS_AND_MESSAGES_MIGRATION_SQL)
            .expect("apply agent session/message schema");
        conn.execute_batch(SCHEDULE_RUNTIME_CORE_MIGRATION_SQL)
            .expect("apply schedule runtime schema");
        conn.execute_batch(TRIGGER_RUNTIME_CORE_MIGRATION_SQL)
            .expect("apply trigger runtime schema");
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn agent_runtime_store_should_upsert_and_retrieve() {
        let stores = RuntimeStoreSet::new(runtime_conn());
        let record = AgentRuntimeRecord {
            agent_id: AgentId::new(),
            loaded: true,
            state: AgentState::Running,
            mode: AgentMode::Full,
            healthy: true,
            active_session_id: Some(SessionId::new()),
            active_dispatches: 2,
            last_active_at: Some("2026-03-23T01:02:03Z".to_string()),
            updated_at: "2026-03-23T01:02:03Z".to_string(),
        };

        stores
            .agent_runtime
            .upsert_agent_runtime(&record)
            .expect("upsert agent runtime");
        let loaded = stores
            .agent_runtime
            .get_agent_runtime(record.agent_id)
            .expect("load agent runtime")
            .expect("agent runtime exists");

        assert_eq!(loaded, record);
    }

    #[test]
    fn agent_session_store_should_create_and_list_sessions() {
        let stores = RuntimeStoreSet::new(runtime_conn());
        let first_agent = AgentId::new();
        let second_agent = AgentId::new();
        let now = "2026-03-23T10:00:00Z".to_string();

        let first_session = AgentSessionRecord {
            session_id: SessionId::new(),
            agent_id: first_agent,
            label: Some("alpha".to_string()),
            active: true,
            message_count: 1,
            dispatch_count: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
            compacted_at: None,
        };
        let second_session = AgentSessionRecord {
            session_id: SessionId::new(),
            agent_id: second_agent,
            label: Some("beta".to_string()),
            active: true,
            message_count: 3,
            dispatch_count: 0,
            created_at: now.clone(),
            updated_at: now,
            compacted_at: None,
        };

        stores
            .agent_session
            .upsert_agent_session(&first_session)
            .expect("store first session");
        stores
            .agent_session
            .upsert_agent_session(&second_session)
            .expect("store second session");

        let first_sessions = stores
            .agent_session
            .list_agent_sessions_for_agent(first_agent)
            .expect("list first sessions");
        let second_sessions = stores
            .agent_session
            .list_agent_sessions_for_agent(second_agent)
            .expect("list second sessions");

        assert_eq!(first_sessions, vec![first_session]);
        assert_eq!(second_sessions, vec![second_session]);
    }

    #[test]
    fn agent_message_store_should_record_direction_and_status() {
        let stores = RuntimeStoreSet::new(runtime_conn());
        let agent_id = AgentId::new();
        let session_id = SessionId::new();

        let inbound = AgentMessageRecord {
            message_id: "message-inbound".to_string(),
            agent_id,
            session_id: session_id.clone(),
            direction: "inbound".to_string(),
            payload_json: "{\"text\":\"hello\"}".to_string(),
            status: "completed".to_string(),
            created_at: "2026-03-23T10:00:00Z".to_string(),
            completed_at: None,
        };
        let outbound = AgentMessageRecord {
            message_id: "message-outbound".to_string(),
            agent_id,
            session_id: session_id.clone(),
            direction: "outbound".to_string(),
            payload_json: "{\"text\":\"hi\"}".to_string(),
            status: "completed".to_string(),
            created_at: "2026-03-23T10:01:00Z".to_string(),
            completed_at: Some("2026-03-23T10:01:01Z".to_string()),
        };

        stores
            .agent_message
            .insert_message(&inbound)
            .expect("store inbound message");
        stores
            .agent_message
            .insert_message(&outbound)
            .expect("store outbound message");

        let messages = stores
            .agent_message
            .list_messages_for_session(session_id)
            .expect("list projected messages");

        assert_eq!(messages, vec![inbound, outbound]);
    }

    #[test]
    fn schedule_runtime_store_should_update_last_run_and_next_run() {
        let stores = RuntimeStoreSet::new(runtime_conn());
        let schedule_id = "daily-report".to_string();
        let initial = ScheduleRuntimeRecord {
            schedule_id: schedule_id.clone(),
            enabled: true,
            last_run: None,
            next_run: Some("2026-03-23T12:00:00Z".to_string()),
            last_status: None,
            consecutive_errors: 0,
            one_shot: false,
            updated_at: "2026-03-23T11:00:00Z".to_string(),
        };
        let updated = ScheduleRuntimeRecord {
            schedule_id: schedule_id.clone(),
            enabled: true,
            last_run: Some("2026-03-23T12:00:00Z".to_string()),
            next_run: Some("2026-03-24T12:00:00Z".to_string()),
            last_status: Some("ok".to_string()),
            consecutive_errors: 0,
            one_shot: false,
            updated_at: "2026-03-23T12:00:01Z".to_string(),
        };

        stores
            .schedule_runtime
            .upsert_schedule_runtime(&initial)
            .expect("store initial schedule runtime");
        stores
            .schedule_runtime
            .upsert_schedule_runtime(&updated)
            .expect("store updated schedule runtime");

        let loaded = stores
            .schedule_runtime
            .get_schedule_runtime(&schedule_id)
            .expect("load schedule runtime")
            .expect("schedule runtime exists");

        assert_eq!(loaded, updated);
    }

    #[test]
    fn schedule_execution_store_should_record_execution_receipt() {
        let stores = RuntimeStoreSet::new(runtime_conn());
        let first = ScheduleExecutionRecord {
            execution_id: "exec-1".to_string(),
            schedule_id: "daily-report".to_string(),
            fired_at: "2026-03-23T12:00:00Z".to_string(),
            status: "ok".to_string(),
            effect_json: Some("{\"delivered\":true}".to_string()),
            error: None,
        };
        let second = ScheduleExecutionRecord {
            execution_id: "exec-2".to_string(),
            schedule_id: "daily-report".to_string(),
            fired_at: "2026-03-23T13:00:00Z".to_string(),
            status: "error".to_string(),
            effect_json: None,
            error: Some("timeout".to_string()),
        };

        stores
            .schedule_execution
            .record_execution(&second)
            .expect("store second execution");
        stores
            .schedule_execution
            .record_execution(&first)
            .expect("store first execution");

        let executions = stores
            .schedule_execution
            .list_executions_for_schedule("daily-report")
            .expect("list execution receipts");

        assert_eq!(executions, vec![first, second]);
    }

    #[test]
    fn trigger_runtime_store_should_round_trip_records() {
        let stores = RuntimeStoreSet::new(runtime_conn());
        let initial = TriggerRuntimeRecord {
            trigger_id: "nightly-build-failed".to_string(),
            enabled: true,
            fire_count: 1,
            max_fires: 5,
            cooldown_secs: 60,
            last_fired_at: Some("2026-03-25T01:00:00Z".to_string()),
            updated_at: "2026-03-25T01:00:01Z".to_string(),
        };
        let updated = TriggerRuntimeRecord {
            trigger_id: initial.trigger_id.clone(),
            enabled: false,
            fire_count: 2,
            max_fires: 5,
            cooldown_secs: 300,
            last_fired_at: Some("2026-03-25T02:00:00Z".to_string()),
            updated_at: "2026-03-25T02:00:01Z".to_string(),
        };

        stores
            .trigger_runtime
            .upsert_trigger_runtime(&initial)
            .expect("store initial trigger runtime");
        stores
            .trigger_runtime
            .upsert_trigger_runtime(&updated)
            .expect("store updated trigger runtime");

        let loaded = stores
            .trigger_runtime
            .get_trigger_runtime(&initial.trigger_id)
            .expect("load trigger runtime")
            .expect("trigger runtime exists");
        let listed = stores
            .trigger_runtime
            .list_trigger_runtimes()
            .expect("list trigger runtimes");

        assert_eq!(loaded, updated);
        assert_eq!(listed, vec![updated]);
    }
}
